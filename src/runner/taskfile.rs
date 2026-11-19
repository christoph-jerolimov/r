//! Taskfile (https://taskfile.dev) tasks. Tasks from `includes` are not seen.

use std::path::Path;

use yaml_rust2::{Yaml, YamlLoader};

use super::{Detected, Kind, Runner, Task, find_file, read};

// Same lookup order as `task`.
const FILES: &[&str] = &[
    "Taskfile.yml",
    "taskfile.yml",
    "Taskfile.yaml",
    "taskfile.yaml",
    "Taskfile.dist.yml",
    "taskfile.dist.yml",
    "Taskfile.dist.yaml",
    "taskfile.dist.yaml",
];

pub fn detect(dir: &Path) -> Detected {
    let file = find_file(dir, FILES)?;
    let tasks = read(&file)
        .and_then(|content| parse(&content).map_err(|e| format!("{}: {e}", file.display())));
    Some(tasks.map(|tasks| Runner {
        kind: Kind::Taskfile,
        tool: "task".into(),
        tasks,
        file,
    }))
}

pub fn parse(content: &str) -> Result<Vec<Task>, String> {
    let docs = YamlLoader::load_from_str(content).map_err(|e| e.to_string())?;
    let Some(tasks) = docs.first().and_then(|doc| doc["tasks"].as_hash()) else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for (key, value) in tasks {
        let name = match key {
            Yaml::String(s) => s.clone(),
            Yaml::Integer(i) => i.to_string(),
            _ => continue,
        };
        if value["internal"].as_bool() == Some(true) {
            continue;
        }
        let description = value["desc"]
            .as_str()
            .or_else(|| value["summary"].as_str())
            .map(str::to_string);
        result.push(Task::new(name, description));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_in_order_with_descriptions() {
        let content = r#"
version: '3'

vars:
  GREETING: hello

tasks:
  default:
    cmds:
      - task: build
  build:
    desc: Build the app
    cmds:
      - go build ./...
  helper:
    internal: true
    cmds: [echo hi]
  short: echo short
  list:
    - echo one
  "docs:serve":
    summary: Serve the docs
"#;
        assert_eq!(
            parse(content).unwrap(),
            vec![
                Task::new("default", None),
                Task::new("build", Some("Build the app".into())),
                Task::new("short", None),
                Task::new("list", None),
                Task::new("docs:serve", Some("Serve the docs".into())),
            ]
        );
    }

    #[test]
    fn no_tasks_and_invalid_yaml() {
        assert_eq!(parse("version: '3'\n").unwrap(), vec![]);
        assert_eq!(parse("").unwrap(), vec![]);
        assert!(parse("tasks: [unclosed").is_err());
    }
}
