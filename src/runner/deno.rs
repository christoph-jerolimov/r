//! Deno tasks from `deno.json` / `deno.jsonc`.

use std::path::Path;

use serde_json::Value;

use super::{Detected, Kind, Runner, Task, find_file, read};

const FILES: &[&str] = &["deno.json", "deno.jsonc"];

pub fn detect(dir: &Path) -> Detected {
    let file = find_file(dir, FILES)?;
    let tasks = read(&file)
        .and_then(|content| parse(&content).map_err(|e| format!("{}: {e}", file.display())));
    Some(tasks.map(|tasks| Runner {
        kind: Kind::Deno,
        tool: "deno".into(),
        tasks,
        file,
    }))
}

pub fn parse(content: &str) -> Result<Vec<Task>, String> {
    let config: Value = serde_json::from_str(&strip_jsonc(content)).map_err(|e| e.to_string())?;
    let Some(tasks) = config.get("tasks").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    Ok(tasks
        .iter()
        .map(|(name, task)| {
            // A task is either a command string or `{ command, description }`.
            let description = task
                .as_str()
                .or_else(|| task.get("description").and_then(Value::as_str))
                .or_else(|| task.get("command").and_then(Value::as_str));
            Task::new(name, description.map(str::to_string))
        })
        .collect())
}

/// Turns JSONC into JSON: removes `//` and `/* */` comments and trailing commas.
fn strip_jsonc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(s) = chars.next() {
                    out.push(s);
                    match s {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                // Keep the newline so error positions stay meaningful.
                while chars.peek().is_some_and(|n| *n != '\n') {
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for n in chars.by_ref() {
                    if previous == '*' && n == '/' {
                        break;
                    }
                    previous = n;
                }
            }
            '}' | ']' => {
                // Drop a comma that only whitespace separates from this bracket.
                let kept = out.trim_end().len();
                if out[..kept].ends_with(',') {
                    let whitespace = out[kept..].to_string();
                    out.truncate(kept - 1);
                    out.push_str(&whitespace);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_from_jsonc() {
        let content = r#"{
  // line comment
  "tasks": {
    "dev": "deno run --watch main.ts", /* block */
    "build": { "command": "deno compile main.ts", "description": "Compile" },
    "url": { "command": "curl http://example.com/*not-a-comment*/" },
  },
}"#;
        assert_eq!(
            parse(content).unwrap(),
            vec![
                Task::new("dev", Some("deno run --watch main.ts".into())),
                Task::new("build", Some("Compile".into())),
                Task::new(
                    "url",
                    Some("curl http://example.com/*not-a-comment*/".into())
                ),
            ]
        );
    }

    #[test]
    fn no_tasks_and_invalid() {
        assert_eq!(parse("{}").unwrap(), vec![]);
        assert!(parse("{").is_err());
    }
}
