//! Shell completion: the static scripts and the dynamic candidates they ask
//! for through the hidden `r __complete` subcommand.

use std::path::{Path, PathBuf};

use crate::cli::{BUILTINS, Shell};
use crate::project::Project;

pub fn script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => include_str!("../completions/r.bash"),
        Shell::Zsh => include_str!("../completions/_r"),
        Shell::Fish => include_str!("../completions/r.fish"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Subcommand,
    Task,
    Runner,
    Option,
    Shell,
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Group::Subcommand => "subcommand",
            Group::Task => "task",
            Group::Runner => "runner",
            Group::Option => "option",
            Group::Shell => "shell",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub group: Group,
    pub value: String,
    pub description: String,
}

impl Candidate {
    fn new(group: Group, value: &str, description: impl Into<String>) -> Self {
        Candidate {
            group,
            value: value.to_string(),
            description: description.into(),
        }
    }
}

/// Candidates for the last of `words` (the arguments typed after `r`, the
/// last one possibly empty or incomplete). No candidates means "let the shell
/// complete file names", e.g. for a task's own arguments.
pub fn candidates(words: &[String], cwd: &Path) -> Vec<Candidate> {
    let Some((current, before)) = words.split_last() else {
        return Vec::new();
    };

    // Global options in front of the subcommand or task.
    let mut dir: Option<&str> = None;
    let mut index = 0;
    while let Some(word) = before.get(index) {
        if word == "-C" || word == "--dir" {
            match before.get(index + 1) {
                Some(value) => dir = Some(value),
                None => return Vec::new(), // completing the directory itself
            }
            index += 2;
        } else if let Some(value) = word.strip_prefix("--dir=") {
            dir = Some(value);
            index += 1;
        } else if let Some(value) = word.strip_prefix("-C").filter(|v| !v.is_empty()) {
            dir = Some(value);
            index += 1;
        } else if word.starts_with('-') {
            index += 1;
        } else {
            break;
        }
    }
    let positionals = &before[index..];
    let wants_option = current.starts_with('-');

    let start = match dir {
        Some(dir) => cwd.join(expand_tilde(dir)),
        None => cwd.to_path_buf(),
    };
    let project = Project::discover(&start);
    let project = project.as_ref();

    let Some((first, rest)) = positionals.split_first() else {
        if wants_option {
            return vec![
                Candidate::new(Group::Option, "-C", "Look for runner files in DIR"),
                Candidate::new(Group::Option, "--dir", "Look for runner files in DIR"),
                Candidate::new(Group::Option, "--quiet", "Do not print the command"),
                Candidate::new(Group::Option, "--help", "Show help"),
                Candidate::new(Group::Option, "--version", "Show the version"),
            ];
        }
        // In this position a built-in wins over a task of the same name, so
        // only the built-in is offered.
        let mut all = tasks_and_runners(project);
        all.retain(|c| !crate::cli::is_builtin(&c.value));
        all.extend(
            BUILTINS
                .iter()
                .filter(|(name, _)| *name != "ls")
                .map(|(name, about)| Candidate::new(Group::Subcommand, name, *about)),
        );
        return all;
    };

    match first.as_str() {
        "list" | "ls" => {
            if wants_option {
                vec![Candidate::new(Group::Option, "--json", "Print JSON")]
            } else if rest.iter().all(|w| w.starts_with('-')) {
                runners(project)
            } else {
                Vec::new()
            }
        }
        "run" | "which" => task_position(project, rest),
        "completions" if rest.is_empty() => ["bash", "zsh", "fish"]
            .iter()
            .map(|shell| Candidate::new(Group::Shell, shell, ""))
            .collect(),
        "help" if rest.is_empty() => BUILTINS
            .iter()
            .filter(|(name, _)| !matches!(*name, "ls" | "help"))
            .map(|(name, about)| Candidate::new(Group::Subcommand, name, *about))
            .collect(),
        "completions" | "help" | "runners" => Vec::new(),
        _ => task_position(project, positionals),
    }
}

/// Candidates where `[RUNNER] <TASK>` is expected and `given` was already typed.
fn task_position(project: Option<&Project>, given: &[String]) -> Vec<Candidate> {
    match given {
        [] => tasks_and_runners(project),
        // After a runner its tasks follow. After a task come the task's own
        // arguments, which only the shell can complete.
        [runner] => project
            .and_then(|p| p.runner_named(runner))
            .map(|runner| {
                runner
                    .tasks
                    .iter()
                    .map(|task| {
                        let description = task.description.as_deref().unwrap_or("");
                        Candidate::new(Group::Task, &task.name, summarize(description))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn tasks_and_runners(project: Option<&Project>) -> Vec<Candidate> {
    let Some(project) = project else {
        return Vec::new();
    };
    let mut all: Vec<Candidate> = project
        .task_names()
        .into_iter()
        .map(|name| {
            let owners = project.owners(name);
            let description = match owners.as_slice() {
                [runner] => match runner.task(name).and_then(|t| t.description.as_deref()) {
                    Some(description) => format!("{}: {}", runner.name(), summarize(description)),
                    None => runner.name().to_string(),
                },
                _ => {
                    let names: Vec<&str> = owners.iter().map(|r| r.name()).collect();
                    format!("ambiguous, prefix with {}", names.join(" or "))
                }
            };
            Candidate::new(Group::Task, name, description)
        })
        .collect();
    all.extend(runners(Some(project)));
    all
}

fn runners(project: Option<&Project>) -> Vec<Candidate> {
    let Some(project) = project else {
        return Vec::new();
    };
    project
        .runners
        .iter()
        .map(|runner| {
            let description = format!("tasks from {}", runner.file_name());
            Candidate::new(Group::Runner, runner.name(), description)
        })
        .collect()
}

/// Formats the candidates the way the shell's completion script expects.
pub fn render(
    shell: Shell,
    candidates: &[Candidate],
    current: &str,
    wordbreaks: Option<&str>,
) -> String {
    let mut out = String::new();
    match shell {
        Shell::Bash => {
            // Bash wants only matching words. When it splits words at `:`
            // (the default), it replaces just the part after the last colon,
            // so that is all it must be given for names like `build:prod`.
            let breaks_at_colon = wordbreaks.is_none_or(|w| w.contains(':'));
            let skip = match current.rfind(':') {
                Some(colon) if breaks_at_colon => colon + 1,
                _ => 0,
            };
            for candidate in candidates {
                if candidate.value.starts_with(current) {
                    out.push_str(&candidate.value[skip..]);
                    out.push('\n');
                }
            }
        }
        Shell::Zsh => {
            for c in candidates {
                let line = format!("{}\t{}\t{}\n", c.group.label(), c.value, c.description);
                out.push_str(&line);
            }
        }
        Shell::Fish => {
            for c in candidates {
                out.push_str(&format!("{}\t{}\n", c.value, c.description));
            }
        }
    }
    out
}

/// Splits a command line the way a shell would, well enough for completion:
/// whitespace separates words, quotes and backslashes are honoured. The last
/// word is the one under the cursor and is empty after trailing whitespace.
pub fn split_line(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => word.extend(chars.next()),
            (Some(_), _) => word.push(c),
            (None, '"' | '\'') => quote = Some(c),
            (None, _) if c.is_whitespace() => {
                words.push(std::mem::take(&mut word));
                // Collapse runs of whitespace.
                while chars.clone().next().is_some_and(char::is_whitespace) {
                    chars.next();
                }
            }
            (None, _) => word.push(c),
        }
    }
    words.push(word);
    words
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(path)
}

/// A description that fits on one line of a listing.
pub fn summarize(description: &str) -> String {
    const MAX: usize = 72;
    let line = description.lines().next().unwrap_or("").trim();
    let line = line.replace('\t', " ");
    if line.chars().count() <= MAX {
        return line;
    }
    let mut short: String = line.chars().take(MAX - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(value: &str) -> Candidate {
        Candidate::new(Group::Task, value, "about")
    }

    #[test]
    fn splitting_lines() {
        assert_eq!(split_line("r bu"), vec!["r", "bu"]);
        assert_eq!(split_line("r  npm   "), vec!["r", "npm", ""]);
        assert_eq!(
            split_line("r -C 'my dir' st"),
            vec!["r", "-C", "my dir", "st"]
        );
        assert_eq!(split_line(r#"r "a b"\ c d\"#), vec!["r", "a b c", "d"]);
        assert_eq!(split_line("r build:pr"), vec!["r", "build:pr"]);
    }

    #[test]
    fn bash_filters_by_prefix_and_trims_colon_prefix() {
        let all = [
            candidate("build"),
            candidate("build:prod"),
            candidate("test"),
        ];
        assert_eq!(render(Shell::Bash, &all, "bu", None), "build\nbuild:prod\n");
        assert_eq!(render(Shell::Bash, &all, "build:p", None), "prod\n");
        assert_eq!(
            render(Shell::Bash, &all, "build:p", Some(" \t\n")),
            "build:prod\n"
        );
    }

    #[test]
    fn zsh_and_fish_get_descriptions() {
        let all = [candidate("build")];
        assert_eq!(render(Shell::Zsh, &all, "", None), "task\tbuild\tabout\n");
        assert_eq!(render(Shell::Fish, &all, "", None), "build\tabout\n");
    }

    #[test]
    fn summaries_are_single_short_lines() {
        assert_eq!(summarize("first\nsecond"), "first");
        let long = "x".repeat(100);
        let short = summarize(&long);
        assert_eq!(short.chars().count(), 72);
        assert!(short.ends_with('…'));
    }
}
