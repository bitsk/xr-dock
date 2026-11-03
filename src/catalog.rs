// SPDX-License-Identifier: GPL-3.0-or-later
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub icon: String,
}

pub fn directories() -> Vec<PathBuf> {
    let home = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    let mut dirs = vec![home.join("applications")];
    for p in env::split_paths(
        &env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ) {
        dirs.push(p.join("applications"));
    }
    dirs
}

pub fn load(dirs: &[PathBuf]) -> Vec<App> {
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let mut paths = Vec::new();
        collect(dir, dir, 0, &mut paths);
        paths.sort();
        for (id, path) in paths {
            // An overriding Hidden entry also masks lower-priority entries.
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(app) = parse(id, &path) {
                apps.push(app);
            }
        }
    }
    apps.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    apps
}
fn collect(base: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            collect(base, &path, depth + 1, out);
        } else if path.extension().is_some_and(|x| x == "desktop") {
            if let Ok(relative) = path.strip_prefix(base) {
                out.push((relative.to_string_lossy().replace('/', "-"), path));
            }
        }
    }
}
fn parse(id: String, path: &Path) -> Option<App> {
    let text = fs::read_to_string(path).ok()?;
    let mut desktop = false;
    let mut entries = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            desktop = line == "[Desktop Entry]";
            continue;
        }
        if !desktop || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            entries.insert(k.to_owned(), v.to_owned());
        }
    }
    if entries.get("Type").is_none_or(|x| x != "Application")
        || entries.get("Hidden").is_some_and(|x| x == "true")
        || entries.get("NoDisplay").is_some_and(|x| x == "true")
        || (!entries.contains_key("Exec")
            && entries.get("DBusActivatable").is_none_or(|x| x != "true"))
    {
        return None;
    }
    let desktops: Vec<_> = env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_else(|_| "UKUI".into())
        .split(':')
        .map(str::to_owned)
        .collect();
    if entries
        .get("OnlyShowIn")
        .is_some_and(|names| !names.split(';').any(|n| desktops.iter().any(|d| d == n)))
        || entries
            .get("NotShowIn")
            .is_some_and(|names| names.split(';').any(|n| desktops.iter().any(|d| d == n)))
    {
        return None;
    }
    let name = entries
        .get("Name[zh_CN]")
        .or_else(|| entries.get("Name"))
        .cloned()?;
    let icon = entries
        .get("Icon")
        .filter(|s| !s.contains('/') && !s.contains('\\'))
        .cloned()
        .unwrap_or_default();
    Some(App { id, name, icon })
}

pub fn pinned(apps: &[App], requested: &[String]) -> Vec<App> {
    if !requested.is_empty() {
        return requested
            .iter()
            .filter_map(|id| apps.iter().find(|a| a.id == *id).cloned())
            .take(5)
            .collect();
    }
    let mut result = Vec::new();
    for candidates in [
        vec!["peony", "filemanager", "files"],
        vec!["qterminal", "terminal", "konsole"],
        vec!["firefox", "chromium", "browser"],
    ] {
        if let Some(app) = candidates
            .iter()
            .find_map(|key| apps.iter().find(|a| a.id.to_lowercase().contains(key)))
        {
            if !result.iter().any(|a: &App| a.id == app.id) {
                result.push(app.clone());
            }
        }
    }
    result
}
