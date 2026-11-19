//! Makefile targets.
//!
//! Targets are read straight from the file instead of asking `make` for its
//! database: that would be slow for shell completion and could run `$(shell)`
//! calls. The price is that `include`d files and generated targets are not
//! seen.

use std::path::Path;

use super::{Detected, Kind, Runner, Task, find_file, read};

// Same lookup order as GNU make.
const FILES: &[&str] = &["GNUmakefile", "makefile", "Makefile"];

const DIRECTIVES: &[&str] = &[
    "ifeq", "ifneq", "ifdef", "ifndef", "else", "endif", "include", "-include", "sinclude",
    "export", "unexport", "override", "private", "vpath", "undefine",
];

pub fn detect(dir: &Path) -> Detected {
    let file = find_file(dir, FILES)?;
    Some(read(&file).map(|content| Runner {
        kind: Kind::Make,
        tool: "make".into(),
        tasks: parse(&content),
        file,
    }))
}

pub fn parse(content: &str) -> Vec<Task> {
    let mut tasks: Vec<Task> = Vec::new();
    let mut in_define = false;
    let mut logical = String::new();

    for raw in content.lines() {
        // Join backslash-continued lines.
        if let Some(head) = raw.strip_suffix('\\') {
            logical.push_str(head);
            logical.push(' ');
            continue;
        }
        logical.push_str(raw);
        let line = std::mem::take(&mut logical);

        let trimmed = line.trim();
        if in_define {
            in_define = trimmed != "endef";
            continue;
        }
        if line.starts_with('\t') || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with("define ") || trimmed.starts_with("override define ") {
            in_define = true;
            continue;
        }
        parse_rule(&line, &mut tasks);
    }
    tasks
}

fn parse_rule(line: &str, tasks: &mut Vec<Task>) {
    // `target: deps ## description` is the common self-documenting convention.
    let (body, comment) = match line.split_once('#') {
        Some((body, comment)) => (body, Some(comment)),
        None => (line, None),
    };
    let description = comment
        .filter(|c| c.starts_with('#'))
        .map(|c| c.trim_start_matches('#').trim().to_string());

    let Some((targets, rest)) = body.split_once(':') else {
        return;
    };
    // `VAR = a:b`, `VAR := x`, `VAR ::= x`
    if targets.contains('=') || rest.starts_with('=') || rest.starts_with(":=") {
        return;
    }
    let rest = rest.strip_prefix(':').unwrap_or(rest); // double-colon rule
    let prerequisites = rest.split(';').next().unwrap_or("");
    // `target: VAR = value` sets a target-specific variable, it is not a rule.
    if prerequisites.contains('=') {
        return;
    }
    let first = targets.split_whitespace().next().unwrap_or("");
    if DIRECTIVES.contains(&first) {
        return;
    }

    for target in targets.split_whitespace() {
        // Special targets (.PHONY), pattern rules, computed names, `&:` groups.
        if target == "&" || target.starts_with('.') || target.contains(['%', '$', '(']) {
            continue;
        }
        match tasks.iter_mut().find(|t| t.name == target) {
            Some(existing) => {
                if existing.description.is_none() {
                    existing.description = description.clone();
                }
            }
            None => tasks.push(Task::new(target, description.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(content: &str) -> Vec<String> {
        parse(content).into_iter().map(|t| t.name).collect()
    }

    #[test]
    fn plain_targets_and_descriptions() {
        let tasks = parse(
            "CC := gcc\n\
             .PHONY: build test\n\
             build: main.o ## Build the binary\n\
             \t$(CC) -o app main.o\n\
             test: build\n\
             \t./app --test\n\
             # just a comment: nothing here\n\
             clean:: ; rm -f app\n",
        );
        assert_eq!(
            tasks,
            vec![
                Task::new("build", Some("Build the binary".into())),
                Task::new("test", None),
                Task::new("clean", None),
            ]
        );
    }

    #[test]
    fn skips_things_that_only_look_like_rules() {
        let content = "\
URL = http://example.com:8080
PATHS ?= a:b
X ::= y
%.o: %.c
\tcc -c $<
$(OUT)/app: main.o
build: CFLAGS = -O2
build: export MODE=release
ifeq ($(OS),Windows_NT)
endif
define HELP
usage: make things
endef
real: dep
";
        assert_eq!(names(content), vec!["real"]);
    }

    #[test]
    fn multiple_targets_continuations_and_duplicates() {
        let content = "\
lint fmt: \\
  deps
\techo hi
lint: ## Lint the code
install-dev: ; pip install -e .
";
        let tasks = parse(content);
        assert_eq!(
            tasks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            vec!["lint", "fmt", "install-dev"]
        );
        assert_eq!(tasks[0].description.as_deref(), Some("Lint the code"));
    }
}
