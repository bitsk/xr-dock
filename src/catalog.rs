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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub startup_wm_class: String,
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
    let categories = entries
        .get("Categories")
        .map(|s| {
            s.split(';')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let startup_wm_class = entries.get("StartupWMClass").cloned().unwrap_or_default();
    Some(App {
        id,
        name,
        icon,
        categories,
        startup_wm_class,
    })
}

pub fn pinned(apps: &[App], requested: &[String]) -> Vec<App> {
    let mut result: Vec<App> = Vec::new();
    if !requested.is_empty() {
        for id in requested {
            if let Some(app) = apps.iter().find(|a| a.id == *id) {
                if !result.iter().any(|a| a.id == app.id) {
                    result.push(app.clone());
                }
                if result.len() == 6 {
                    break;
                }
            }
        }
        return result;
    }
    // Role order follows the control-library artwork. Every slot must resolve
    // to a real visible desktop entry; missing roles contribute no placeholder.
    let roles: &[(&[&str], &[&str], &str)] = &[
        (
            &[
                "org.openkylin.XRFiles",
                "xr-files",
                "peony",
                "org.gnome.nautilus",
                "org.gnome.files",
                "dolphin",
                "thunar",
                "caja",
                "pcmanfm",
            ],
            &["FileManager"],
            "ukui-xr-files",
        ),
        (
            &[
                "firefox",
                "firefox-esr",
                "org.mozilla.firefox",
                "net.mozilla.firefox",
                "chromium",
                "chromium-browser",
                "google-chrome",
                "falkon",
            ],
            &["WebBrowser"],
            "ukui-xr-browser",
        ),
        (
            &[
                "pluma",
                "xed",
                "gedit",
                "org.gnome.texteditor",
                "mousepad",
                "kate",
                "wps-office-wps",
                "libreoffice-writer",
                "atril",
                "org.gnome.evince",
            ],
            &["WordProcessor", "TextEditor", "Office"],
            "ukui-xr-document",
        ),
        (
            &[
                "kylin-video",
                "org.ukui.kylin-video",
                "mpv",
                "vlc",
                "org.gnome.totem",
                "totem",
                "parole",
            ],
            &["Video"],
            "ukui-xr-media",
        ),
        (
            &[
                "qterminal",
                "org.ukui.qterminal",
                "org.kde.konsole",
                "konsole",
                "org.gnome.terminal",
                "gnome-terminal",
                "mate-terminal",
                "xfce4-terminal",
            ],
            &["TerminalEmulator"],
            "ukui-xr-terminal",
        ),
        (
            &[
                "ukui-control-center",
                "org.ukui.control-center",
                "org.gnome.controlcenter",
                "gnome-control-center",
                "systemsettings",
                "systemsettings5",
            ],
            &["Settings"],
            "ukui-xr-settings",
        ),
    ];
    for (preferred, categories, icon) in roles {
        let available = |app: &&App| !result.iter().any(|a| a.id == app.id);
        let chosen = preferred
            .iter()
            .find_map(|id| {
                apps.iter().filter(available).find(|app| {
                    app.id
                        .strip_suffix(".desktop")
                        .unwrap_or(&app.id)
                        .eq_ignore_ascii_case(id)
                })
            })
            .or_else(|| {
                apps.iter().filter(available).find(|app| {
                    app.categories
                        .iter()
                        .any(|c| categories.iter().any(|wanted| c == wanted))
                })
            });
        if let Some(app) = chosen {
            let mut pin = app.clone();
            pin.icon = (*icon).into();
            result.push(pin);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_prefers_xr_files_ahead_of_legacy_2d_peony() {
        let apps = vec![
            App {
                id: "peony.desktop".into(),
                name: "文件管理器".into(),
                icon: "system-file-manager".into(),
                categories: vec!["FileManager".into()],
                startup_wm_class: "peony".into(),
            },
            App {
                id: "org.openkylin.XRFiles.desktop".into(),
                name: "空间文件".into(),
                icon: "system-file-manager".into(),
                categories: vec!["System".into(), "FileManager".into()],
                startup_wm_class: "xr-files".into(),
            },
        ];
        let pins = pinned(&apps, &[]);
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].id, "org.openkylin.XRFiles.desktop");
        assert_eq!(pins[0].icon, "ukui-xr-files");
    }
}
