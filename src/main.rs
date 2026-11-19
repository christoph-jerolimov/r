//! `r` – one short command for every task runner.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use serde_json::json;

mod cli;
mod complete;
mod project;
mod resolve;
mod runner;

use cli::{Cli, Command, Shell, Target};
use project::Project;
use resolve::Resolution;
use runner::Runner;

/// Exit code for "the request does not make sense" (unknown or ambiguous task).
const USAGE: u8 = 2;
/// Exit code for "the runner's tool is not installed", as shells use it.
const NOT_FOUND: u8 = 127;

struct Failure {
    message: String,
    code: u8,
}

impl Failure {
    fn new(message: impl Into<String>) -> Self {
        Failure {
            message: message.into(),
            code: 1,
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Failure {
            message: message.into(),
            code: USAGE,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => code,
        Err(failure) => {
            let style = Style::for_stream(std::io::stderr().is_terminal());
            eprintln!("{}: {}", style.red("error"), failure.message);
            ExitCode::from(failure.code)
        }
    }
}

fn dispatch(cli: Cli) -> Result<ExitCode, Failure> {
    let dir = cli.dir.as_deref();
    match cli.command {
        None => list(&load_project(dir)?, None, false),
        Some(Command::List { runner, json }) => list(&load_project(dir)?, runner.as_deref(), json),
        Some(Command::Runners) => runners(&load_project(dir)?),
        Some(Command::Completions { shell }) => {
            print!("{}", complete::script(shell));
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Complete {
            shell,
            line,
            wordbreaks,
            words,
        }) => {
            completion_candidates(shell, line, wordbreaks, words);
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Run {
            target: Target::Words(words),
        }) => run(&load_project(dir)?, &words, Mode::Execute, cli.quiet),
        Some(Command::Which {
            target: Target::Words(words),
        }) => run(&load_project(dir)?, &words, Mode::Print, cli.quiet),
        Some(Command::Task(words)) => run(&load_project(dir)?, &words, Mode::Execute, cli.quiet),
    }
}

fn start_dir(dir: Option<&Path>) -> PathBuf {
    dir.map(Path::to_path_buf).unwrap_or_else(|| ".".into())
}

fn load_project(dir: Option<&Path>) -> Result<Project, Failure> {
    let start = start_dir(dir);
    if !start.is_dir() {
        return Err(Failure::new(format!(
            "{} is not a directory",
            start.display()
        )));
    }
    let Some(project) = Project::discover(&start) else {
        let place = match dir {
            Some(dir) => dir.display().to_string(),
            None => "the current directory".to_string(),
        };
        return Err(Failure::new(format!(
            "no {} found in {place} or its parents",
            runner::FILE_HINT
        )));
    };
    let style = Style::for_stream(std::io::stderr().is_terminal());
    for warning in &project.warnings {
        eprintln!("{}: {warning}", style.yellow("warning"));
    }
    Ok(project)
}

// ---------------------------------------------------------------------------
// Running tasks

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Execute,
    Print,
}

fn run(project: &Project, words: &[String], mode: Mode, quiet: bool) -> Result<ExitCode, Failure> {
    let resolution = resolve::resolve(project, words).map_err(|e| Failure::usage(e.to_string()))?;
    let (runner, tool, task, args) = match resolution {
        Resolution::ListRunner(runner) => return list(project, Some(runner.name()), false),
        Resolution::Run {
            runner,
            tool,
            task,
            args,
        } => (runner, tool, task, args),
    };
    let command = runner.command(&tool, &task, &args);
    let display = shell_join(&command);
    if mode == Mode::Print {
        println!("{display}");
        return Ok(ExitCode::SUCCESS);
    }
    if !quiet {
        let style = Style::for_stream(std::io::stderr().is_terminal());
        eprintln!("{}", style.dim(&format!("$ {display}")));
    }
    execute(&command, &project.dir, runner)
}

fn execute(command: &[String], dir: &Path, runner: &Runner) -> Result<ExitCode, Failure> {
    let tool = &command[0];
    let mut process = std::process::Command::new(program(tool));
    process.args(&command[1..]).current_dir(dir);

    let spawn_failure = |error: std::io::Error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Failure {
                message: format!(
                    "`{tool}` is not installed or not in PATH (needed for {})",
                    runner.file_name()
                ),
                code: NOT_FOUND,
            }
        } else {
            Failure::new(format!("could not run `{tool}`: {error}"))
        }
    };

    // On Unix the tool replaces this process, so signals, the terminal and
    // the exit status behave exactly as if it had been called directly.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(spawn_failure(process.exec()))
    }
    #[cfg(not(unix))]
    {
        let status = process.status().map_err(spawn_failure)?;
        Ok(ExitCode::from(status.code().unwrap_or(1) as u8))
    }
}

/// The executable for `tool`. On Windows the node package managers are
/// installed as `.cmd` shims, which are only found when named in full.
fn program(tool: &str) -> String {
    if cfg!(windows) && matches!(tool, "npm" | "yarn" | "pnpm") {
        format!("{tool}.cmd")
    } else {
        tool.to_string()
    }
}

/// The command as it could be pasted into a shell.
fn shell_join(command: &[String]) -> String {
    let quoted: Vec<String> = command.iter().map(|word| shell_quote(word)).collect();
    quoted.join(" ")
}

fn shell_quote(word: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if !word.is_empty() && word.chars().all(safe) {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

// ---------------------------------------------------------------------------
// Listing

fn select_runners<'a>(
    project: &'a Project,
    only: Option<&str>,
) -> Result<Vec<&'a Runner>, Failure> {
    let Some(name) = only else {
        return Ok(project.runners.iter().collect());
    };
    match project.runner_named(name) {
        Some(runner) => Ok(vec![runner]),
        None if runner::RUNNER_NAMES.contains(&name) => Err(Failure::usage(format!(
            "no file for `{name}` found in {}",
            project.dir.display()
        ))),
        None => Err(Failure::usage(format!(
            "unknown runner `{name}`, supported are {}",
            runner::RUNNER_NAMES.join(", ")
        ))),
    }
}

fn list(project: &Project, only: Option<&str>, as_json: bool) -> Result<ExitCode, Failure> {
    let runners = select_runners(project, only)?;
    let ambiguous = |task: &str| project.owners(task).len() > 1;

    if as_json {
        let runners: Vec<_> = runners
            .iter()
            .map(|runner| {
                let tasks: Vec<_> = runner
                    .tasks
                    .iter()
                    .map(|task| {
                        json!({
                            "name": task.name,
                            "description": task.description,
                            "ambiguous": ambiguous(&task.name),
                        })
                    })
                    .collect();
                json!({ "name": runner.name(), "file": runner.file, "tasks": tasks })
            })
            .collect();
        let document = json!({ "dir": project.dir, "runners": runners });
        println!("{document:#}");
        return Ok(ExitCode::SUCCESS);
    }

    let style = Style::for_stream(std::io::stdout().is_terminal());
    // Markers need a column of their own only if any task is marked.
    let marker = |task: &str| match (ambiguous(task), cli::is_builtin(task)) {
        (true, true) => "*+",
        (true, false) => "*",
        (false, true) => "+",
        (false, false) => "",
    };
    let width = runners
        .iter()
        .flat_map(|r| &r.tasks)
        .map(|t| t.name.chars().count() + marker(&t.name).len())
        .max()
        .unwrap_or(0)
        .min(32);

    let mut any_ambiguous = None;
    let mut any_shadowed = None;
    for (index, runner) in runners.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let file = format!("({})", runner.file_name());
        println!("{} {}", style.bold(runner.name()), style.dim(&file));
        if runner.tasks.is_empty() {
            println!("  {}", style.dim("no tasks"));
        }
        for task in &runner.tasks {
            if ambiguous(&task.name) {
                any_ambiguous.get_or_insert((runner.name(), task.name.as_str()));
            }
            if cli::is_builtin(&task.name) {
                any_shadowed.get_or_insert((runner.name(), task.name.as_str()));
            }
            let name = format!("{}{}", task.name, style.yellow(marker(&task.name)));
            let used = task.name.chars().count() + marker(&task.name).len();
            match &task.description {
                Some(description) => {
                    let padding = " ".repeat(width.saturating_sub(used));
                    let description = complete::summarize(description);
                    println!("  {name}{padding}  {}", style.dim(&description));
                }
                None => println!("  {name}"),
            }
        }
    }

    if any_ambiguous.is_some() || any_shadowed.is_some() {
        println!();
    }
    if let Some((runner, task)) = any_ambiguous {
        let note = format!("defined by several runners, name one: r {runner} {task}");
        println!("{} {}", style.yellow("*"), style.dim(&note));
    }
    if let Some((runner, task)) = any_shadowed {
        let note = format!("also an r subcommand, use: r run {task} (or r {runner} {task})");
        println!("{} {}", style.yellow("+"), style.dim(&note));
    }
    Ok(ExitCode::SUCCESS)
}

fn runners(project: &Project) -> Result<ExitCode, Failure> {
    let style = Style::for_stream(std::io::stdout().is_terminal());
    println!("{}", style.dim(&project.dir.display().to_string()));
    let name_width = project
        .runners
        .iter()
        .map(|r| r.name().len())
        .max()
        .unwrap_or(0);
    let file_width = project
        .runners
        .iter()
        .map(|r| r.file_name().len())
        .max()
        .unwrap_or(0);
    for runner in &project.runners {
        let count = match runner.tasks.len() {
            1 => "1 task".to_string(),
            n => format!("{n} tasks"),
        };
        // Pad before styling: escape codes would otherwise count as width.
        let name = format!("{:name_width$}", runner.name());
        println!(
            "  {}  {:file_width$}  {}",
            style.bold(&name),
            runner.file_name(),
            style.dim(&count)
        );
    }
    Ok(ExitCode::SUCCESS)
}

// ---------------------------------------------------------------------------
// Completion

fn completion_candidates(
    shell: Shell,
    line: Option<String>,
    wordbreaks: Option<String>,
    words: Vec<String>,
) {
    // Bash hands over the raw line, the first word of which is `r` itself.
    let words = match line {
        Some(line) => complete::split_line(&line).split_off(1),
        None => words,
    };
    let Ok(cwd) = std::env::current_dir() else {
        return;
    };
    let candidates = complete::candidates(&words, &cwd);
    let current = words.last().map(String::as_str).unwrap_or("");
    print!(
        "{}",
        complete::render(shell, &candidates, current, wordbreaks.as_deref())
    );
}

// ---------------------------------------------------------------------------
// Colours

#[derive(Clone, Copy)]
struct Style {
    enabled: bool,
}

impl Style {
    fn for_stream(is_terminal: bool) -> Self {
        Style {
            enabled: is_terminal && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn paint(self, code: &str, text: &str) -> String {
        if self.enabled && !text.is_empty() {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn bold(self, text: &str) -> String {
        self.paint("1", text)
    }

    fn dim(self, text: &str) -> String {
        self.paint("2", text)
    }

    fn red(self, text: &str) -> String {
        self.paint("1;31", text)
    }

    fn yellow(self, text: &str) -> String {
        self.paint("33", text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_for_display() {
        let command: Vec<String> = ["npm", "run", "build", "--", "a b", "it's", "", "x=1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            shell_join(&command),
            r"npm run build -- 'a b' 'it'\''s' '' x=1"
        );
    }

    #[test]
    fn cli_definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
