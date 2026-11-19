//! `package.json` scripts, run through npm, yarn, pnpm or bun.

use std::path::Path;

use serde_json::Value;

use super::{Detected, Kind, NODE_TOOLS, Runner, Task, read};

/// Lock files and the package manager they belong to, in order of preference.
const LOCK_FILES: &[(&str, &str)] = &[
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("package-lock.json", "npm"),
    ("npm-shrinkwrap.json", "npm"),
];

pub fn detect(dir: &Path) -> Detected {
    let file = dir.join("package.json");
    if !file.is_file() {
        return None;
    }
    let package = match read(&file).and_then(|content| {
        serde_json::from_str::<Value>(&content).map_err(|e| format!("{}: {e}", file.display()))
    }) {
        Ok(package) => package,
        Err(e) => return Some(Err(e)),
    };
    Some(Ok(Runner {
        kind: Kind::Node,
        tool: package_manager(&package, dir),
        tasks: scripts(&package),
        file,
    }))
}

fn scripts(package: &Value) -> Vec<Task> {
    let Some(scripts) = package.get("scripts").and_then(Value::as_object) else {
        return Vec::new();
    };
    scripts
        .iter()
        .map(|(name, command)| Task::new(name, command.as_str().map(str::to_string)))
        .collect()
}

/// The `packageManager` field wins, then the nearest lock file (which lives in
/// the workspace root for monorepos), then npm.
fn package_manager(package: &Value, dir: &Path) -> String {
    let declared = package
        .get("packageManager")
        .and_then(Value::as_str)
        .and_then(|pm| pm.split('@').next())
        .filter(|name| NODE_TOOLS.contains(name));
    if let Some(name) = declared {
        return name.to_string();
    }
    for ancestor in dir.ancestors() {
        for (lock, tool) in LOCK_FILES {
            if ancestor.join(lock).is_file() {
                return tool.to_string();
            }
        }
    }
    "npm".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_keep_their_order() {
        let package: Value = serde_json::from_str(
            r#"{"name": "x", "scripts": {"start": "node .", "build": "tsc", "a": "b"}}"#,
        )
        .unwrap();
        let tasks = scripts(&package);
        assert_eq!(
            tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            vec!["start", "build", "a"]
        );
        assert_eq!(tasks[1].description.as_deref(), Some("tsc"));
    }

    #[test]
    fn no_scripts() {
        let package: Value = serde_json::from_str(r#"{"name": "x"}"#).unwrap();
        assert!(scripts(&package).is_empty());
    }

    #[test]
    fn package_manager_field() {
        let dir = Path::new("/nonexistent-r-test-dir");
        let package: Value =
            serde_json::from_str(r#"{"packageManager": "pnpm@9.1.0+sha512.abc"}"#).unwrap();
        assert_eq!(package_manager(&package, dir), "pnpm");
        let package: Value = serde_json::from_str(r#"{"packageManager": "weird@1"}"#).unwrap();
        assert_eq!(package_manager(&package, dir), "npm");
    }
}
