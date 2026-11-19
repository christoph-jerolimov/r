//! Command line definition.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Built-in subcommands and their summaries, for completion and suggestions.
/// A task with one of these names needs `r run <task>` or `r <runner> <task>`.
pub const BUILTINS: &[(&str, &str)] = &[
    ("list", "List the available tasks"),
    ("ls", "List the available tasks"),
    ("run", "Run a task, even if a built-in has the same name"),
    ("which", "Print the command a task would run"),
    ("runners", "Show the detected runners"),
    ("completions", "Print a shell completion script"),
    ("help", "Show help"),
];

pub fn is_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|(builtin, _)| *builtin == name)
}

const AFTER_HELP: &str = "\
Examples:
  r                    list all tasks
  r build              run `build` with whichever runner defines it
  r test --watch       everything after the task is passed on to it
  r npm start          choose the runner when several define `start`
  r npm                list only the tasks of one runner
  r run list           run a task named like a built-in subcommand

Runners: make (Makefile), npm/yarn/pnpm/bun (package.json), just (justfile),
task (Taskfile.yml), deno (deno.json), cargo (Cargo.toml)

Shell completion: eval \"$(r completions bash)\", see `r completions --help`.";

#[derive(Debug, Parser)]
#[command(
    name = "r",
    version,
    about = "Run tasks from Makefile, package.json, justfile and friends with one command",
    override_usage = "r [OPTIONS] [RUNNER] <TASK> [ARGS]...\n       r [OPTIONS] <COMMAND>",
    after_help = AFTER_HELP,
    allow_external_subcommands = true
)]
pub struct Cli {
    /// Look for runner files in DIR instead of the current directory
    #[arg(short = 'C', long = "dir", value_name = "DIR")]
    pub dir: Option<PathBuf>,

    /// Do not print the command before running it
    #[arg(short, long)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the available tasks (the default without arguments)
    #[command(visible_alias = "ls")]
    List {
        /// Only show the tasks of this runner
        runner: Option<String>,

        /// Print machine readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Run a task, even if a built-in subcommand has the same name
    #[command(
        override_usage = "r run [RUNNER] <TASK> [ARGS]...",
        disable_help_subcommand = true
    )]
    Run {
        #[command(subcommand)]
        target: Target,
    },

    /// Print the command a task would run, without running it
    #[command(
        override_usage = "r which [RUNNER] <TASK> [ARGS]...",
        disable_help_subcommand = true
    )]
    Which {
        #[command(subcommand)]
        target: Target,
    },

    /// Show the detected runners and the files they were found in
    Runners,

    /// Print a shell completion script
    #[command(after_help = COMPLETIONS_HELP)]
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Completion candidates for the shell scripts.
    #[command(name = "__complete", hide = true)]
    Complete {
        #[arg(value_enum)]
        shell: Shell,

        /// The command line up to the cursor (bash).
        #[arg(long, allow_hyphen_values = true)]
        line: Option<String>,

        /// The shell's word break characters (bash's COMP_WORDBREAKS).
        #[arg(long, allow_hyphen_values = true)]
        wordbreaks: Option<String>,

        /// The words up to and including the one being completed.
        #[arg(last = true)]
        words: Vec<String>,
    },

    #[command(external_subcommand)]
    Task(Vec<String>),
}

/// `[RUNNER] <TASK> [ARGS]...`, taken verbatim: nothing after the task is
/// interpreted by r, not even `--`, so `r run test -- --nocapture` hands the
/// separator on to the runner.
#[derive(Debug, Subcommand)]
pub enum Target {
    #[command(external_subcommand)]
    Words(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

const COMPLETIONS_HELP: &str = "\
Setup:
  bash   echo 'eval \"$(r completions bash)\"' >> ~/.bashrc

  zsh    `r` is a zsh builtin (it repeats the last command), so disable that
         first. Add to ~/.zshrc, after compinit:
           disable r
           source <(command r completions zsh)

  fish   r completions fish > ~/.config/fish/completions/r.fish";
