use std::{path::PathBuf, sync::Arc};

use image::imageops::FilterType;

use crate::{Msg, RawEntry};

fn xdg_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(local) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(local).join("applications"));
    } else if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }

    let sys =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".to_owned());
    for p in sys.split(':') {
        dirs.push(PathBuf::from(p).join("applications"));
    }

    dirs
}

pub async fn scan_desktop_files() -> Msg {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for dir in xdg_data_dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if let Some(raw) = parse_desktop(&text)
                && seen.insert(raw.name.clone())
            {
                out.push(raw);
            }
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Msg::ScannedApps(Arc::new(out))
}

fn parse_desktop(text: &str) -> Option<RawEntry> {
    let mut in_desktop_entry = false;
    let mut name = String::new();
    let mut description = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut no_display = false;
    let mut hidden = false;
    let mut is_app = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.starts_with('#') {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Type" => is_app = val.trim() == "Application",
            "Name" => {
                if name.is_empty() {
                    name = val.trim().to_owned();
                }
            }
            "Comment" => {
                if description.is_empty() {
                    description = val.trim().to_owned();
                }
            }
            "GenericName" => {
                if description.is_empty() {
                    description = val.trim().to_owned();
                }
            }
            "Exec" => exec = strip_field_codes(val.trim()),
            "Icon" => icon = val.trim().to_owned(),
            "Keywords" => {
                keywords = val
                    .trim()
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_lowercase())
                    .collect();
            }
            "NoDisplay" => no_display = val.trim().eq_ignore_ascii_case("true"),
            "Hidden" => hidden = val.trim().eq_ignore_ascii_case("true"),
            _ => {}
        }
    }

    if !is_app || no_display || hidden || name.is_empty() || exec.is_empty() {
        return None;
    }

    Some(RawEntry {
        name,
        description,
        exec,
        icon_name: icon,
        keywords,
    })
}

fn strip_field_codes(exec: &str) -> String {
    let mut out = String::with_capacity(exec.len());
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('%') => {
                    out.push('%');
                    chars.next();
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_owned()
}

fn icon_search_paths(icon_name: &str, size: u32) -> Vec<PathBuf> {
    if icon_name.starts_with('/') {
        return vec![PathBuf::from(icon_name)];
    }

    let size_str = format!("{}x{}", size, size);

    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        bases.push(PathBuf::from(h));
    } else if let Some(h) = std::env::var_os("HOME") {
        bases.push(PathBuf::from(h).join(".local/share"));
    }
    let sys =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for p in sys.split(':') {
        bases.push(PathBuf::from(p));
    }

    let subdirs = [
        format!("icons/hicolor/{}/apps", size_str),
        "icons/hicolor/scalable/apps".to_owned(),
        "icons/hicolor/48x48/apps".to_owned(),
        "icons/hicolor/256x256/apps".to_owned(),
        "icons/Adwaita/32x32/apps".to_owned(),
        "pixmaps".to_owned(),
    ];

    let mut paths = Vec::new();
    for base in &bases {
        for sub in &subdirs {
            let dir = base.join(sub);
            for ext in &["png", "xpm"] {
                paths.push(dir.join(format!("{}.{}", icon_name, ext)));
            }
        }
    }
    paths
}

pub async fn load_icon(index: usize, icon_name: &str, size: u32) -> Msg {
    for path in icon_search_paths(icon_name, size) {
        if !path.exists() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("svg") {
            continue;
        }
        let Ok(reader) = image::ImageReader::open(&path) else {
            continue;
        };
        let Ok(img) = reader.decode() else {
            continue;
        };
        let img = img.resize_to_fill(size, size, FilterType::Lanczos3);
        let rgba = img.to_rgba8();
        return Msg::IconLoaded(index, size, size, Arc::new(rgba.into_raw()));
    }
    Msg::IconLoaded(index, 0, 0, Arc::new(Vec::new()))
}

pub fn search(apps: &[RawEntry], query: &str, max: usize) -> Msg {
    let q = query.to_lowercase();

    let mut scored: Vec<(usize, u8)> = apps
        .iter()
        .enumerate()
        .filter_map(|(i, app)| {
            let name_lower = app.name.to_lowercase();
            if name_lower.starts_with(&q) {
                Some((i, 3u8))
            } else if name_lower.contains(&q) {
                Some((i, 2u8))
            } else if app.description.to_lowercase().contains(&q)
                || app.keywords.iter().any(|k| k.contains(&q))
            {
                Some((i, 1u8))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1).then(apps[a.0].name.cmp(&apps[b.0].name)));
    scored.truncate(max);

    Msg::Results(scored.into_iter().map(|(i, _)| i).collect())
}

pub async fn launch_app(exec: String, mut cmd: Vec<String>) -> Msg {
    let post = match cmd.iter().position(|c| c == "%command%") {
        Some(idx) => cmd.drain(idx..).skip(1).collect::<Vec<_>>(),
        None => Vec::new(),
    };

    if !parse_exec_args(&exec, &mut cmd) {
        return Msg::Launched;
    }

    cmd.extend(post);

    if let Some(program) = cmd.first() {
        let _ = async_process::Command::new(program)
            .args(&cmd[1..])
            .stdin(async_process::Stdio::null())
            .stdout(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .spawn();
    }

    Msg::Launched
}

pub fn parse_exec_args(exec: &str, args: &mut Vec<String>) -> bool {
    let mut has_exec = false;
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut chars = exec.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(&next) = chars.peek() {
                    current.push(next);
                    chars.next();
                }
            }
            '"' | '\'' if !in_quotes => {
                in_quotes = true;
                quote_char = c;
            }
            c if in_quotes && c == quote_char => {
                in_quotes = false;
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                    has_exec = true;
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        args.push(current);
        has_exec = true;
    }
    has_exec
}
