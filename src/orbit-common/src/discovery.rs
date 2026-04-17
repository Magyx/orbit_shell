use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::{
    config::Config,
    xdg::{SYSTEM_MODULES_DIR, default_user_modules_dir},
};

#[derive(Debug, Clone)]
pub struct DiscoveredModule {
    pub name: String,
    pub path: PathBuf,
    pub enabled: bool,
}

// FIX: TOCTOU and parent dir check, these are security risks
//
/// Returns `Ok(())` when `path` passes our security requirements:
///
/// * The file must be owned by the current effective user.
/// * The file must not be group- or world-writable (prevents another user or a
///   compromised process from swapping in a malicious `.so`).
///
/// System-directory files (`/usr/lib/orbit/modules/*`) are trusted
/// unconditionally — they are installed by the package manager, root-owned,
/// and not writeable by ordinary users.
fn check_user_file_permissions(path: &Path) -> Result<(), String> {
    let meta = fs::metadata(path).map_err(|e| format!("could not stat {}: {e}", path.display()))?;

    let euid = unsafe { libc::geteuid() };
    if meta.uid() != euid {
        return Err(format!(
            "{} is not owned by the current user (uid {euid}, file owner uid {})",
            path.display(),
            meta.uid()
        ));
    }

    // S_IWGRP = 0o020, S_IWOTH = 0o002
    let mode = meta.mode();
    if mode & 0o022 != 0 {
        return Err(format!(
            "{} is group- or world-writable (mode {mode:#o}); refusing to load",
            path.display()
        ));
    }

    Ok(())
}

fn scan_dir(
    dir: &Path,
    is_user: bool,
    map: &mut std::collections::HashMap<String, (PathBuf, bool)>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "so").unwrap_or(false)
            && let Some(stem) = path
                .file_stem()
                .and_then(|s| s.to_str())
                // Strip a leading "lib" prefix that the linker adds
                // (libbar.so → bar).
                .map(|s| s.strip_prefix("lib").unwrap_or(s))
        {
            map.insert(stem.to_owned(), (path, is_user));
        }
    }
}

/// Scan both the system and user modules directories, returning one entry per
/// uniquely-named `.so` file.  User modules shadow system modules of the same
/// name.
///
/// Only user-directory entries are permission-checked.  System-directory
/// entries are trusted by virtue of being installed by the package manager.
///
/// The `modules_dir_override` field of `config` (if set) replaces the default
/// user path (`<config_home>/modules`) but never affects the system path.
pub fn discover_modules(config_home: &Path, config: &Config) -> Vec<DiscoveredModule> {
    let user_dir: PathBuf = config
        .modules_dir_override
        .clone()
        .unwrap_or_else(|| default_user_modules_dir(config_home));

    let mut by_name: std::collections::HashMap<String, (PathBuf, bool /* is_user */)> =
        std::collections::HashMap::new();

    scan_dir(Path::new(SYSTEM_MODULES_DIR), false, &mut by_name);

    if user_dir.is_dir() {
        scan_dir(&user_dir, /*is_user=*/ true, &mut by_name);
    } else if config.modules_dir_override.is_some() {
        // The override path was specified but doesn't exist — warn and skip.
        tracing::warn!(
            path = %user_dir.display(),
            "modules_dir override does not exist or is not a directory"
        );
    }

    let mut items: Vec<(String, PathBuf, bool)> = by_name
        .into_iter()
        .map(|(name, (path, is_user))| (name, path, is_user))
        .collect();
    items.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

    let mut result = Vec::with_capacity(items.len());

    for (name, path, is_user) in items {
        if is_user && let Err(e) = check_user_file_permissions(&path) {
            tracing::warn!(module = %name, error = %e, "skipping module");
            continue;
        }

        let enabled = config.enabled(&name);
        result.push(DiscoveredModule {
            name,
            path,
            enabled,
        });
    }

    result
}
