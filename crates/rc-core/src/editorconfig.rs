//! Per-file indentation from EditorConfig. Reads belong on a background worker.
//! Glob normalization and bounded file reads adapted from CrabCraft's code_style.rs
//! (MIT OR Apache-2.0). No Rust-specific formatting settings are imported.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_FILE: u64 = 256 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Indentation {
    pub tabs: Option<bool>,
    pub size: Option<usize>,
    pub tab_width: Option<usize>,
}

/// Resolve the nearest matching properties, stopping at root=true or the filesystem root.
/// Unsupported and invalid values are ignored. Cancellation discards incomplete results.
pub fn for_file(file: &Path, cancelled: &AtomicBool) -> Option<Indentation> {
    let mut configs = Vec::new();
    for dir in file.ancestors().skip(1) {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if let Some(text) = read(&dir.join(".editorconfig")) {
            let (properties, root) = parse(&text, dir, file);
            configs.push(properties);
            if root {
                break;
            }
        }
    }
    let mut properties = BTreeMap::new();
    for config in configs.into_iter().rev() {
        for (key, value) in config {
            if value == "unset" {
                properties.remove(&key);
            } else {
                properties.insert(key, value);
            }
        }
    }
    let get = |key: &str| properties.get(key).map(String::as_str);
    let number = |value: Option<&str>| {
        value
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| (1..=32).contains(n))
    };
    let tabs = match get("indent_style") {
        Some("tab") => Some(true),
        Some("space") => Some(false),
        _ => None,
    };
    let width = number(get("tab_width"));
    let size = if get("indent_size") == Some("tab") {
        width
    } else {
        number(get("indent_size"))
    };
    Some(Indentation {
        tabs,
        size,
        tab_width: width.or(size),
    })
}

fn read(path: &Path) -> Option<String> {
    use std::io::Read;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_FILE + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MAX_FILE).then_some(text)
}

fn parse(text: &str, dir: &Path, file: &Path) -> (BTreeMap<String, String>, bool) {
    let relative = file
        .strip_prefix(dir)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"));
    let mut properties = BTreeMap::new();
    let (mut root, mut preamble, mut matching) = (false, true, false);
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(pattern) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            preamble = false;
            matching = relative
                .as_deref()
                .is_some_and(|r| section_matches(pattern, r));
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().to_ascii_lowercase();
        if preamble {
            if key == "root" {
                root = value == "true";
            }
        } else if matching {
            let valid = match key.as_str() {
                "indent_style" => matches!(value.as_str(), "tab" | "space" | "unset"),
                "indent_size" => matches!(value.as_str(), "tab" | "unset") || valid_size(&value),
                "tab_width" => value == "unset" || valid_size(&value),
                _ => false,
            };
            if valid {
                properties.insert(key, value);
            }
        }
    }
    (properties, root)
}

fn valid_size(value: &str) -> bool {
    value.parse::<usize>().is_ok_and(|n| (1..=32).contains(&n))
}

fn section_matches(section: &str, relative: &str) -> bool {
    let pattern = section
        .strip_prefix('/')
        .map(str::to_string)
        .unwrap_or_else(|| {
            if section.contains('/') {
                section.to_string()
            } else {
                format!("**/{section}")
            }
        });
    let mut normalized = String::with_capacity(pattern.len() + 4);
    let mut rest = pattern.as_str();
    while let Some(at) = rest.find("**") {
        let (before, after) = (&rest[..at], &rest[at + 2..]);
        normalized.push_str(before);
        if !(before.is_empty() || before.ends_with('/')) {
            normalized.push_str("*/");
        }
        normalized.push_str("**");
        if !(after.is_empty() || after.starts_with('/')) {
            normalized.push_str("/*");
        }
        rest = after;
    }
    normalized.push_str(rest);
    globset::GlobBuilder::new(&normalized)
        .literal_separator(true)
        .backslash_escape(true)
        .build()
        .is_ok_and(|g| g.compile_matcher().is_match(relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn resolve(root: &Path, path: &str) -> Indentation {
        for_file(&root.join(path), &AtomicBool::new(false)).unwrap()
    }
    #[test]
    fn nested_csharp_rules_override_and_unset_parent_properties() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".editorconfig",
            "root = true\n[*]\nindent_style = space\nindent_size = 8\ntab_width = 6\n",
        );
        write(
            root,
            "src/.editorconfig",
            "[*.{cs,csx}]\nindent_size = 2\ntab_width = unset\n[Generated*.cs]\nindent_style = tab\nindent_size = tab\ntab_width = 3\n",
        );
        assert_eq!(
            resolve(root, "src/App.cs"),
            Indentation {
                tabs: Some(false),
                size: Some(2),
                tab_width: Some(2)
            }
        );
        assert_eq!(
            resolve(root, "src/GeneratedA.cs"),
            Indentation {
                tabs: Some(true),
                size: Some(3),
                tab_width: Some(3)
            }
        );
        assert_eq!(resolve(root, "README.md").size, Some(8));
    }
    #[test]
    fn root_invalid_values_and_cancellation_are_respected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".editorconfig",
            "root=true\n[*]\nindent_style=space\nindent_size=8\n",
        );
        write(
            root,
            "src/.editorconfig",
            "ROOT=TRUE\n[*.cs]\nindent_size=3\nindent_size=400\nindent_style=sideways\n",
        );
        assert_eq!(
            resolve(root, "src/App.cs"),
            Indentation {
                tabs: None,
                size: Some(3),
                tab_width: Some(3)
            }
        );
        assert!(for_file(&root.join("src/App.cs"), &AtomicBool::new(true)).is_none());
    }
    #[test]
    fn globs_are_case_sensitive_and_path_aware() {
        assert!(section_matches("*.{cs,csx}", "src/App.cs"));
        assert!(!section_matches("*.CS", "App.cs"));
        assert!(section_matches("/src/*.cs", "src/App.cs"));
        assert!(!section_matches("/src/*.cs", "src/nested/App.cs"));
        assert!(section_matches("src/**.cs", "src/nested/App.cs"));
        assert!(section_matches("[AB]?.cs", "A1.cs"));
        assert!(!section_matches("[invalid", "App.cs"));
    }
    #[test]
    fn oversized_and_non_regular_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".editorconfig",
            &"x".repeat(MAX_FILE as usize + 1),
        );
        assert_eq!(resolve(dir.path(), "App.cs"), Indentation::default());
        assert!(read(dir.path()).is_none());
    }
}
