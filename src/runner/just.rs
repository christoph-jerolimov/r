//! `justfile` recipes.
//!
//! Parsed directly (no `just --dump`) so listing and completion work without
//! spawning a process. Recipes from `import`ed files and modules are not seen.

use std::path::Path;

use super::{Detected, Kind, Runner, Task, find_file, read};

const FILES: &[&str] = &["justfile", "Justfile", ".justfile"];

pub fn detect(dir: &Path) -> Detected {
    let file = find_file(dir, FILES)?;
    Some(read(&file).map(|content| Runner {
        kind: Kind::Just,
        tool: "just".into(),
        tasks: parse(&content),
        file,
    }))
}

pub fn parse(content: &str) -> Vec<Task> {
    let mut tasks: Vec<Task> = Vec::new();
    // State collected from the lines directly above a recipe.
    let mut doc: Option<String> = None;
    let mut private = false;

    for line in content.lines() {
        if line.starts_with([' ', '\t']) {
            continue; // recipe body
        }
        let line = line.trim_end();
        if line.is_empty() {
            doc = None;
            private = false;
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            doc = (!comment.starts_with('!')).then(|| comment.trim().to_string());
            continue;
        }
        if line.starts_with('[') {
            private |= line.contains("private");
            if let Some(text) = doc_attribute(line) {
                doc = Some(text);
            }
            continue;
        }

        let found = if let Some(alias) = line.strip_prefix("alias ") {
            alias.split_once(":=").map(|(name, target)| {
                let description = format!("alias for {}", target.trim());
                (name.trim().to_string(), Some(description))
            })
        } else {
            recipe_name(line).map(|name| (name.to_string(), doc.take()))
        };
        if let Some((name, description)) = found {
            let hidden = private || name.starts_with('_');
            if !hidden && !tasks.iter().any(|t| t.name == name) {
                tasks.push(Task::new(name, description));
            }
        }
        doc = None;
        private = false;
    }
    tasks
}

/// The recipe name if `line` is a recipe header such as `build target="x": deps`.
fn recipe_name(line: &str) -> Option<&str> {
    let line = line.strip_prefix('@').unwrap_or(line);
    let len = line
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(line.len());
    if len == 0 {
        return None;
    }
    // A header has a `:` outside of quotes that is not part of `:=`
    // (assignments, settings and exports use `:=`).
    let mut quote: Option<char> = None;
    let mut chars = line[len..].chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' | '`' => quote = Some(c),
                ':' => return (chars.peek() != Some(&'=')).then_some(&line[..len]),
                _ => {}
            },
        }
    }
    None
}

/// Text of a `[doc('...')]` attribute.
fn doc_attribute(line: &str) -> Option<String> {
    let rest = &line[line.find("doc(")? + 4..];
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let rest = &rest[1..];
    Some(rest[..rest.find(quote)?].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipes_docs_and_privacy() {
        let content = r#"
set shell := ["bash", "-c"]
export RUST_BACKTRACE := "1"
version := "1.0"
import 'other.just'

# Build the project
build target="debug": deps
    cargo build

@test *args:
    cargo test {{args}}

_helper:
    echo hidden

[private]
secret:
    echo hidden

[doc('Deploy to production')]
[confirm]
deploy env="a:b": build
    ./deploy

alias b := build

# stale comment

lint:
    cargo clippy
"#;
        assert_eq!(
            parse(content),
            vec![
                Task::new("build", Some("Build the project".into())),
                Task::new("test", None),
                Task::new("deploy", Some("Deploy to production".into())),
                Task::new("b", Some("alias for build".into())),
                Task::new("lint", None),
            ]
        );
    }

    #[test]
    fn header_detection() {
        assert_eq!(recipe_name("build:"), Some("build"));
        assert_eq!(recipe_name("@build-all a b: dep"), Some("build-all"));
        assert_eq!(recipe_name("x := 'a'"), None);
        assert_eq!(recipe_name("set dotenv-load"), None);
        assert_eq!(recipe_name("mod docs 'a:b'"), None);
    }
}
