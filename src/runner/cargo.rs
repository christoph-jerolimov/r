//! Cargo. A `Cargo.toml` defines no tasks of its own, so the everyday cargo
//! commands are offered, plus the aliases from `.cargo/config.toml`.

use std::path::Path;

use super::{Detected, Kind, Runner, Task};

const COMMANDS: &[(&str, &str)] = &[
    ("build", "Compile the current package"),
    ("check", "Check the current package for errors"),
    ("test", "Run the tests"),
    ("run", "Run a binary or example"),
    ("bench", "Run the benchmarks"),
    ("doc", "Build the documentation"),
    ("clippy", "Lint with clippy"),
    ("fmt", "Format the code"),
    ("clean", "Remove the target directory"),
];

pub fn detect(dir: &Path) -> Detected {
    let file = dir.join("Cargo.toml");
    if !file.is_file() {
        return None;
    }
    let mut tasks: Vec<Task> = COMMANDS
        .iter()
        .map(|(name, description)| Task::new(*name, Some(description.to_string())))
        .collect();
    for alias in aliases(dir) {
        if !tasks.iter().any(|t| t.name == alias.name) {
            tasks.push(alias);
        }
    }
    Some(Ok(Runner {
        kind: Kind::Cargo,
        tool: "cargo".into(),
        tasks,
        file,
    }))
}

/// Aliases from the nearest `.cargo/config.toml`. A broken config is ignored:
/// cargo itself will complain about it when it runs.
fn aliases(dir: &Path) -> Vec<Task> {
    dir.ancestors()
        .flat_map(|a| [a.join(".cargo/config.toml"), a.join(".cargo/config")])
        .find_map(|config| std::fs::read_to_string(config).ok())
        .map(|content| parse_aliases(&content))
        .unwrap_or_default()
}

fn parse_aliases(content: &str) -> Vec<Task> {
    let Ok(config) = content.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(aliases) = config.get("alias").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    aliases
        .iter()
        .filter_map(|(name, value)| {
            let expansion = match value {
                toml::Value::String(s) => s.clone(),
                toml::Value::Array(parts) => parts
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => return None,
            };
            Some(Task::new(name, Some(format!("alias for {expansion}"))))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_as_strings_and_arrays() {
        let tasks = parse_aliases(
            "[alias]\nxtask = \"run --package xtask --\"\nci = [\"test\", \"--all\"]\n",
        );
        assert!(tasks.contains(&Task::new(
            "xtask",
            Some("alias for run --package xtask --".into())
        )));
        assert!(tasks.contains(&Task::new("ci", Some("alias for test --all".into()))));
    }

    #[test]
    fn broken_or_empty_config() {
        assert!(parse_aliases("not toml [").is_empty());
        assert!(parse_aliases("[build]\njobs = 2\n").is_empty());
    }
}
