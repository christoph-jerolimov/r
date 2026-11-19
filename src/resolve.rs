//! Turning `r <words...>` into the runner, task and arguments to execute.

use std::fmt;

use crate::cli::BUILTINS;
use crate::project::Project;
use crate::runner::{RUNNER_NAMES, Runner};

#[derive(Debug)]
pub enum Resolution<'a> {
    /// Run `task` with `tool`, passing `args` through.
    Run {
        runner: &'a Runner,
        tool: String,
        task: String,
        args: Vec<String>,
    },
    /// Only a runner was named (`r npm`): show what it offers.
    ListRunner(&'a Runner),
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResolveError {
    Ambiguous {
        task: String,
        /// (runner name, file name) of every runner defining the task.
        runners: Vec<(String, String)>,
    },
    UnknownTask {
        task: String,
        suggestions: Vec<String>,
    },
    UnknownTaskInRunner {
        runner: String,
        file: String,
        task: String,
        suggestions: Vec<String>,
    },
    RunnerNotDetected {
        runner: String,
    },
}

/// Resolution rules, in order:
/// 1. `<runner> <task> [args]` when the runner is present and has that task.
/// 2. `<task> [args]` when exactly one runner defines the task.
/// 3. Otherwise an error; more than one defining runner is ambiguous.
pub fn resolve<'a>(project: &'a Project, words: &[String]) -> Result<Resolution<'a>, ResolveError> {
    let first = words[0].as_str();
    let qualified = project.runner_named(first);

    if let (Some(runner), Some(task)) = (qualified, words.get(1))
        && runner.task(task).is_some()
    {
        return Ok(Resolution::Run {
            runner,
            tool: runner.tool_for(first),
            task: task.clone(),
            args: words[2..].to_vec(),
        });
    }

    let owners = project.owners(first);
    match owners.as_slice() {
        [runner] => Ok(Resolution::Run {
            runner,
            tool: runner.tool.clone(),
            task: first.to_string(),
            args: words[1..].to_vec(),
        }),
        [] => match (qualified, words.get(1)) {
            (Some(runner), None) => Ok(Resolution::ListRunner(runner)),
            (Some(runner), Some(task)) => Err(ResolveError::UnknownTaskInRunner {
                runner: first.to_string(),
                file: runner.file_name(),
                task: task.clone(),
                suggestions: suggest(task, runner.tasks.iter().map(|t| t.name.as_str())),
            }),
            (None, _) if RUNNER_NAMES.contains(&first) => Err(ResolveError::RunnerNotDetected {
                runner: first.to_string(),
            }),
            (None, _) => Err(ResolveError::UnknownTask {
                task: first.to_string(),
                suggestions: suggest(
                    first,
                    project
                        .task_names()
                        .into_iter()
                        .chain(project.runners.iter().map(|r| r.name()))
                        .chain(BUILTINS.iter().map(|(name, _)| *name)),
                ),
            }),
        },
        _ => Err(ResolveError::Ambiguous {
            task: first.to_string(),
            runners: owners
                .iter()
                .map(|r| (r.name().to_string(), r.file_name()))
                .collect(),
        }),
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::Ambiguous { task, runners } => {
                writeln!(
                    f,
                    "`{task}` is ambiguous, {} runners define it:",
                    runners.len()
                )?;
                let width = runners
                    .iter()
                    .map(|(name, _)| name.len())
                    .max()
                    .unwrap_or(0);
                for (name, file) in runners {
                    writeln!(f, "  {name:width$}  {file}")?;
                }
                let options: Vec<String> = runners
                    .iter()
                    .map(|(name, _)| format!("`r {name} {task}`"))
                    .collect();
                write!(f, "pick one with {}", options.join(" or "))
            }
            ResolveError::UnknownTask { task, suggestions } => {
                write!(f, "unknown task `{task}`")?;
                write_suggestions(f, suggestions)?;
                write!(f, "\nrun `r list` to see the available tasks")
            }
            ResolveError::UnknownTaskInRunner {
                runner,
                file,
                task,
                suggestions,
            } => {
                write!(f, "{file} has no task `{task}`")?;
                write_suggestions(f, suggestions)?;
                write!(f, "\nrun `r list {runner}` to see its tasks")
            }
            ResolveError::RunnerNotDetected { runner } => {
                write!(f, "no file for `{runner}` found here")?;
                write!(f, "\nrun `r runners` to see which runners are available")
            }
        }
    }
}

fn write_suggestions(f: &mut fmt::Formatter<'_>, suggestions: &[String]) -> fmt::Result {
    if suggestions.is_empty() {
        return Ok(());
    }
    let quoted: Vec<String> = suggestions.iter().map(|s| format!("`{s}`")).collect();
    write!(f, ", did you mean {}?", quoted.join(" or "))
}

/// Up to three candidates that are close to `input`, closest first.
fn suggest<'a>(input: &str, candidates: impl Iterator<Item = &'a str>) -> Vec<String> {
    let limit = (input.chars().count() / 3).max(1);
    let mut scored: Vec<(usize, &str)> = Vec::new();
    for candidate in candidates {
        // `r run help` reaches this with the built-in `help` as a candidate.
        if candidate == input {
            continue;
        }
        let distance = edit_distance(input, candidate);
        let related = distance <= limit || (input.len() >= 3 && candidate.starts_with(input));
        if related && !scored.iter().any(|(_, c)| *c == candidate) {
            scored.push((distance, candidate));
        }
    }
    scored.sort_by_key(|(distance, _)| *distance);
    scored
        .into_iter()
        .take(3)
        .map(|(_, c)| c.to_string())
        .collect()
}

/// Edit distance in which swapping two neighbouring characters counts as a
/// single edit, since that is the most common typo (`biuld`).
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in table.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in table[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let substitution = table[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]);
            let mut best = substitution
                .min(table[i - 1][j] + 1)
                .min(table[i][j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(table[i - 2][j - 2] + 1);
            }
            table[i][j] = best;
        }
    }
    table[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::runner::{Kind, Task};

    fn runner(kind: Kind, tool: &str, file: &str, tasks: &[&str]) -> Runner {
        Runner {
            kind,
            tool: tool.into(),
            file: PathBuf::from(file),
            tasks: tasks.iter().map(|t| Task::new(*t, None)).collect(),
        }
    }

    fn project() -> Project {
        Project {
            dir: PathBuf::from("/p"),
            runners: vec![
                runner(Kind::Make, "make", "Makefile", &["build", "deploy", "npm"]),
                runner(Kind::Node, "yarn", "package.json", &["build", "start"]),
            ],
            warnings: vec![],
        }
    }

    fn words(w: &[&str]) -> Vec<String> {
        w.iter().map(|s| s.to_string()).collect()
    }

    /// (tool, task, args) of a successful run resolution.
    fn run(w: &[&str]) -> (String, String, Vec<String>) {
        match resolve(&project(), &words(w)) {
            Ok(Resolution::Run {
                tool, task, args, ..
            }) => (tool, task, args),
            other => panic!("expected a run, got {other:?}"),
        }
    }

    #[test]
    fn unique_task_runs_with_its_runner() {
        assert_eq!(run(&["start"]), ("yarn".into(), "start".into(), vec![]));
        assert_eq!(
            run(&["deploy", "--now", "x"]),
            ("make".into(), "deploy".into(), words(&["--now", "x"]))
        );
    }

    #[test]
    fn duplicate_task_is_ambiguous() {
        let err = resolve(&project(), &words(&["build"])).unwrap_err();
        assert_eq!(
            err,
            ResolveError::Ambiguous {
                task: "build".into(),
                runners: vec![
                    ("make".into(), "Makefile".into()),
                    ("yarn".into(), "package.json".into())
                ],
            }
        );
        let message = err.to_string();
        assert!(
            message.contains("`r make build` or `r yarn build`"),
            "{message}"
        );
    }

    #[test]
    fn qualifier_resolves_duplicates() {
        assert_eq!(run(&["make", "build"]).0, "make");
        assert_eq!(
            run(&["yarn", "build", "--watch"]),
            ("yarn".into(), "build".into(), words(&["--watch"]))
        );
    }

    #[test]
    fn any_node_tool_can_be_forced() {
        assert_eq!(run(&["npm", "start"]).0, "npm");
        assert_eq!(run(&["pnpm", "build"]).0, "pnpm");
    }

    #[test]
    fn task_named_like_a_runner_still_runs() {
        // `npm` is a make target here; `npm x` is not a script, so the target wins.
        assert_eq!(run(&["npm"]), ("make".into(), "npm".into(), vec![]));
        assert_eq!(
            run(&["npm", "x"]),
            ("make".into(), "npm".into(), words(&["x"]))
        );
    }

    #[test]
    fn runner_without_task_lists_it() {
        let project = project();
        match resolve(&project, &words(&["yarn"])) {
            Ok(Resolution::ListRunner(r)) => assert_eq!(r.name(), "yarn"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn errors() {
        let p = project();
        assert_eq!(
            resolve(&p, &words(&["just", "build"])).unwrap_err(),
            ResolveError::RunnerNotDetected {
                runner: "just".into()
            }
        );
        assert_eq!(
            resolve(&p, &words(&["yarn", "strat"])).unwrap_err(),
            ResolveError::UnknownTaskInRunner {
                runner: "yarn".into(),
                file: "package.json".into(),
                task: "strat".into(),
                suggestions: vec!["start".into()],
            }
        );
        assert_eq!(
            resolve(&p, &words(&["biuld"])).unwrap_err(),
            ResolveError::UnknownTask {
                task: "biuld".into(),
                suggestions: vec!["build".into()],
            }
        );
    }

    #[test]
    fn distances() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
        assert_eq!(edit_distance("biuld", "build"), 1);
    }
}
