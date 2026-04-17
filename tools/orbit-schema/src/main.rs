use std::{
    collections::HashMap,
    ffi::CStr,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, RwLock},
};

use axum::{Json, Router, extract::State, response::IntoResponse, routing::get};
use clap::Parser;
use serde_json::{Value, json};

use orbit_common::{
    config::{Config, ConfigEvent, load_cfg},
    discovery::discover_modules,
    watcher::ConfigWatcher,
    xdg::config_home,
};

/// Serves JSON Schemas for all enabled orbit modules over HTTP
/// so that yaml-language-server can provide config completion.
///
/// Point yaml-language-server at the running instance by adding to your editor
/// config:
///
///   yaml.schemaStore.url: "http://127.0.0.1:7837"
///
/// The catalog is at `/` and schema at `/schema.json`.
#[derive(Parser, Debug)]
#[command(name = "orbit-schema", version, about, verbatim_doc_comment)]
struct Args {
    /// Address to listen on.
    #[arg(long, default_value = "127.0.0.1:7837")]
    listen: SocketAddr,

    /// Path to the orbit config directory.
    /// Defaults to $XDG_CONFIG_HOME/orbit (or ~/.config/orbit).
    #[arg(long)]
    config_dir: Option<PathBuf>,
}

type SchemaMap = Arc<RwLock<HashMap<String, (bool, Value)>>>;

#[derive(Clone)]
struct AppState {
    schemas: SchemaMap,
    base_url: String,
}

type OrbitSchemaFn = unsafe extern "C" fn() -> *const std::ffi::c_char;

/// dlopen `path` and call its `orbit_schema()` export, returning the JSON
/// string it produces.
fn load_schema_from_so(path: &std::path::Path) -> Result<Value, String> {
    // Safety: we are loading a trusted (permission-checked by orbit-common)
    // orbit module .so.  The `orbit_schema` symbol follows a well-known ABI:
    // it returns a pointer to a static null-terminated UTF-8 JSON string that
    // remains valid for the lifetime of the library.  We parse the JSON
    // immediately and then drop the library, so no dangling pointer can escape.
    let lib = orbit_common::loader::LibraryHandle::open(path)?;

    let schema_fn: OrbitSchemaFn = unsafe { lib.get_fn(b"orbit_schema\0")? };

    let ptr = unsafe { schema_fn() };
    if ptr.is_null() {
        return Err(format!("{}: orbit_schema() returned null", path.display()));
    }

    let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().map_err(|e| {
        format!(
            "{}: orbit_schema() returned invalid UTF-8: {e}",
            path.display()
        )
    })?;

    serde_json::from_str(json_str).map_err(|e| {
        format!(
            "{}: orbit_schema() returned invalid JSON: {e}",
            path.display()
        )
    })
}

fn fix_refs(value: &mut Value, module_name: &str) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "$ref" {
                    if let Some(ref_str) = v.as_str() {
                        // If it's an internal reference like "#/$defs/..."
                        if ref_str.starts_with("#/") {
                            let new_ref = ref_str.replacen(
                                "#/",
                                &format!("#/properties/{}/", module_name),
                                1,
                            );
                            *v = json!(new_ref);
                        }
                    }
                } else {
                    fix_refs(v, module_name);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                fix_refs(v, module_name);
            }
        }
        _ => {}
    }
}

fn build_schema_map(
    config_dir: &std::path::Path,
    config: &Config,
) -> HashMap<String, (bool, Value)> {
    let modules = discover_modules(config_dir, config);
    let mut map = HashMap::new();

    for module in modules {
        if !module.enabled {
            map.insert(module.name, (false, Value::Null));
            continue;
        }

        match load_schema_from_so(&module.path) {
            Ok(mut schema) => {
                if let Some(obj) = schema.as_object_mut() {
                    obj.remove("$schema");
                    obj.entry("title").or_insert_with(|| json!(module.name));
                }
                fix_refs(&mut schema, &module.name);

                tracing::info!(module = %module.name, "loaded schema");
                map.insert(module.name, (true, schema));
            }
            Err(e) => {
                tracing::warn!(module = %module.name, error = %e, "failed to load schema");
            }
        }
    }

    map
}

/// `GET /` — SchemaStore-compatible catalog.
///
/// yaml-language-server fetches this once on startup to discover available
/// schemas.
async fn catalog(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "schemas": [{
            "name": "orbit",
            "description": "Orbit shell configuration",
            "url": format!("{}/schema.json", state.base_url),
            "fileMatch": [
                "**/orbit/config.yaml",
                "**/orbit/config.yml"
            ]
        }]
    }))
}

/// `GET /schema.json` — merged schema for the full Orbit config file.
async fn root_schema(State(state): State<AppState>) -> impl IntoResponse {
    let schemas = state.schemas.read().unwrap();

    let mut properties = serde_json::Map::new();
    let mut modules_properties = serde_json::Map::new();

    for (name, (enabled, schema)) in schemas.iter() {
        if *enabled {
            properties.insert(name.clone(), schema.clone());
        }
        modules_properties.insert(
            name.clone(),
            json!({
                "type": "boolean",
                "description": format!("Enable or disable the {name} module")
            }),
        );
    }

    properties.insert(
        "modules".into(),
        json!({
            "type": "object",
            "description": "Enable or disable Orbit modules",
            "properties": modules_properties,
            "additionalProperties": false
        }),
    );

    Json(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Orbit Config",
        "type": "object",
        "properties": properties,
        "additionalProperties": false
    }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orbit_schema=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();
    let config_dir = args.config_dir.unwrap_or_else(config_home);
    let base_url = format!("http://{}", args.listen);

    // Initial config load.
    let config = match load_cfg(&config_dir) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to load orbit config: {e}");
            Config::default()
        }
    };

    // Build initial schema map.
    let schemas: SchemaMap = Arc::new(RwLock::new(build_schema_map(&config_dir, &config)));

    tracing::info!(
        schemas = schemas.read().unwrap().len(),
        "initial schemas loaded"
    );

    let mut watcher = {
        let schemas = Arc::clone(&schemas);

        let mut watcher = ConfigWatcher::new(&config_dir, {
            let config_dir = config_dir.clone();
            move |event| match event {
                ConfigEvent::Reload(new_config) => {
                    tracing::info!("config reloaded, rebuilding schemas");
                    let new_map = build_schema_map(&config_dir, &new_config);
                    let count = new_map.len();
                    *schemas.write().unwrap() = new_map;
                    tracing::info!(schemas = count, "schemas updated");
                }
                ConfigEvent::Err(errs) => {
                    for e in errs {
                        tracing::warn!("config error: {e}");
                    }
                }
            }
        });
        watcher.start();
        watcher
    };

    // Build the Axum router.
    let state = AppState {
        schemas,
        base_url: base_url.clone(),
    };

    let app = Router::new()
        .route("/", get(catalog))
        .route("/schema.json", get(root_schema))
        .with_state(state);

    tracing::info!(%base_url, "orbit-schema listening");

    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .expect("failed to bind");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install CTRL+C handler");
            watcher.stop();
        })
        .await
        .expect("server error");
}
