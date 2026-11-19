//! End-to-end tests: the real binary, run in throw-away projects, with fake
//! `make`, `npm`, ... on the PATH that report how they were called.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TOOLS: &[&str] = &[
    "make", "npm", "yarn", "pnpm", "bun", "just", "task", "deno", "cargo",
];

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// A fresh directory with a `project` (its own git repository, so the
    /// search for runner files stops there) and a `bin` full of fake tools.
    fn new(name: &str) -> Sandbox {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("project/.git")).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        let sandbox = Sandbox { root };
        for tool in TOOLS {
            sandbox.fake_tool(tool, 0);
        }
        sandbox
    }

    /// A tool that prints its name, arguments and working directory.
    fn fake_tool(&self, tool: &str, exit_code: u8) {
        let path = self.root.join("bin").join(tool);
        let script =
            format!("#!/bin/sh\necho \"{tool} $*\"\necho \"cwd=$(pwd -P)\"\nexit {exit_code}\n");
        fs::write(&path, script).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn project(&self) -> PathBuf {
        self.root.join("project")
    }

    fn file(&self, name: &str, content: &str) -> &Sandbox {
        let path = self.project().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        self
    }

    fn makefile(&self) -> &Sandbox {
        self.file(
            "Makefile",
            "build: ## Build with make\n\t@true\ndeploy:\n\t@true\nlist:\n\t@true\n",
        )
    }

    fn package_json(&self) -> &Sandbox {
        self.file(
            "package.json",
            r#"{"scripts": {"build": "tsc", "start": "node .", "build:prod": "tsc -p prod"}}"#,
        )
    }

    fn r(&self, args: &[&str]) -> Run {
        self.r_in("project", args)
    }

    fn r_in(&self, dir: &str, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_r"))
            .args(args)
            .current_dir(self.root.join(dir))
            .env("PATH", self.root.join("bin"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        Run::from(output)
    }
}

#[derive(Debug)]
struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl From<Output> for Run {
    fn from(output: Output) -> Run {
        Run {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

impl Run {
    /// The first line a fake tool printed: its name and arguments.
    fn invocation(&self) -> &str {
        assert_eq!(self.code, 0, "{self:?}");
        self.stdout.lines().next().unwrap_or("")
    }
}

#[test]
fn a_unique_task_runs_with_the_tool_that_defines_it() {
    let s = Sandbox::new("unique");
    s.makefile().package_json();
    assert_eq!(s.r(&["deploy"]).invocation(), "make deploy");
    assert_eq!(s.r(&["start"]).invocation(), "npm run start");
}

#[test]
fn the_command_is_announced_on_stderr_unless_quiet() {
    let s = Sandbox::new("announce");
    s.makefile();
    assert_eq!(s.r(&["deploy"]).stderr, "$ make deploy\n");
    assert_eq!(s.r(&["-q", "deploy"]).stderr, "");
}

#[test]
fn a_task_defined_twice_fails_and_names_the_ways_out() {
    let s = Sandbox::new("ambiguous");
    s.makefile().package_json();
    let run = s.r(&["build"]);
    assert_eq!(run.code, 2, "{run:?}");
    assert_eq!(run.stdout, "", "nothing may be executed");
    assert!(run.stderr.contains("`build` is ambiguous"), "{run:?}");
    assert!(
        run.stderr.contains("`r make build` or `r npm build`"),
        "{run:?}"
    );
}

#[test]
fn naming_the_runner_resolves_the_ambiguity() {
    let s = Sandbox::new("qualified");
    s.makefile().package_json();
    assert_eq!(s.r(&["make", "build"]).invocation(), "make build");
    assert_eq!(s.r(&["npm", "build"]).invocation(), "npm run build");
    // Any node package manager can be asked for explicitly.
    assert_eq!(s.r(&["yarn", "build"]).invocation(), "yarn run build");
}

#[test]
fn arguments_after_the_task_are_passed_through() {
    let s = Sandbox::new("passthrough");
    s.makefile().package_json();
    assert_eq!(
        s.r(&["start", "--port", "3000", "-v"]).invocation(),
        "npm run start -- --port 3000 -v"
    );
    assert_eq!(
        s.r(&["make", "build", "-j4", "V=1"]).invocation(),
        "make build -j4 V=1"
    );
    // A `--` belongs to the task as well, wherever it appears.
    assert_eq!(
        s.r(&["deploy", "--", "--flag"]).invocation(),
        "make deploy -- --flag"
    );
    assert_eq!(
        s.r(&["run", "deploy", "--", "--flag"]).invocation(),
        "make deploy -- --flag"
    );
    assert_eq!(
        s.r(&["which", "make", "deploy", "--", "--flag"]).stdout,
        "make deploy -- --flag\n"
    );
    // Options of r itself are only recognised in front of the task.
    assert_eq!(
        s.r(&["deploy", "-q", "--help"]).invocation(),
        "make deploy -q --help"
    );
}

#[test]
fn the_package_manager_follows_the_lock_file_or_package_json() {
    let s = Sandbox::new("lockfile");
    s.package_json().file("yarn.lock", "");
    assert_eq!(s.r(&["start"]).invocation(), "yarn run start");
    assert!(s.r(&["list"]).stdout.starts_with("yarn (package.json)"));

    let s = Sandbox::new("package-manager-field");
    s.file(
        "package.json",
        r#"{"packageManager": "pnpm@9.0.0", "scripts": {"start": "node ."}}"#,
    )
    .file("yarn.lock", "");
    assert_eq!(s.r(&["start"]).invocation(), "pnpm run start");
}

#[test]
fn every_supported_file_is_detected() {
    let s = Sandbox::new("all-runners");
    s.file("Makefile", "m-task:\n\t@true\n")
        .file("package.json", r#"{"scripts": {"n-task": "x"}}"#)
        .file("justfile", "# A recipe\nj-task arg='x':\n    echo hi\n")
        .file(
            "Taskfile.yml",
            "version: '3'\ntasks:\n  t-task:\n    desc: A task\n    cmds: [echo hi]\n",
        )
        .file(
            "deno.jsonc",
            "{\n  // comment\n  \"tasks\": { \"d-task\": \"deno run x.ts\", },\n}\n",
        )
        .file("Cargo.toml", "[package]\nname = \"x\"\n")
        .file(
            ".cargo/config.toml",
            "[alias]\nc-task = \"run --release\"\n",
        );

    let cases = [
        ("m-task", "make m-task a"),
        ("n-task", "npm run n-task -- a"),
        ("j-task", "just j-task a"),
        ("t-task", "task t-task -- a"),
        ("d-task", "deno task d-task a"),
        ("c-task", "cargo c-task a"),
        ("clippy", "cargo clippy a"),
    ];
    for (task, expected) in cases {
        assert_eq!(s.r(&[task, "a"]).invocation(), expected);
    }

    let runners = s.r(&["runners"]).stdout;
    for file in [
        "Makefile",
        "package.json",
        "justfile",
        "Taskfile.yml",
        "deno.jsonc",
        "Cargo.toml",
    ] {
        assert!(runners.contains(file), "{file} missing in:\n{runners}");
    }
}

#[test]
fn tasks_run_in_the_project_directory_even_from_a_subdirectory() {
    let s = Sandbox::new("subdir");
    s.makefile().file("src/deep/file.txt", "");
    let run = s.r_in("project/src/deep", &["deploy"]);
    let project = fs::canonicalize(s.project()).unwrap();
    assert_eq!(run.invocation(), "make deploy");
    assert!(
        run.stdout.contains(&format!("cwd={}", project.display())),
        "{run:?}"
    );
}

#[test]
fn dir_option_selects_the_project() {
    let s = Sandbox::new("dir-option");
    s.makefile();
    assert_eq!(
        s.r_in("bin", &["-C", "../project", "deploy"]).invocation(),
        "make deploy"
    );
    let run = s.r_in("bin", &["-C", "../nowhere", "list"]);
    assert_eq!(run.code, 1);
    assert!(run.stderr.contains("is not a directory"), "{run:?}");
}

#[test]
fn the_exit_code_of_the_tool_is_passed_on() {
    let s = Sandbox::new("exit-code");
    s.makefile();
    s.fake_tool("make", 7);
    assert_eq!(s.r(&["deploy"]).code, 7);
}

#[test]
fn a_missing_tool_is_reported() {
    let s = Sandbox::new("missing-tool");
    s.makefile();
    fs::remove_file(s.root.join("bin/make")).unwrap();
    let run = s.r(&["deploy"]);
    assert_eq!(run.code, 127, "{run:?}");
    assert!(run.stderr.contains("`make` is not installed"), "{run:?}");
}

#[test]
fn list_shows_tasks_per_runner_and_marks_special_ones() {
    let s = Sandbox::new("list");
    s.makefile().package_json();
    let expected = "\
make (Makefile)
  build*      Build with make
  deploy
  list+

npm (package.json)
  build*      tsc
  start       node .
  build:prod  tsc -p prod

* defined by several runners, name one: r make build
+ also an r subcommand, use: r run list (or r make list)
";
    assert_eq!(s.r(&["list"]).stdout, expected);
    assert_eq!(s.r(&[]).stdout, expected, "listing is the default");
    assert_eq!(s.r(&["ls"]).stdout, expected);

    // One runner only, either through `list` or by naming just the runner.
    let only_npm = s.r(&["list", "npm"]).stdout;
    assert!(only_npm.starts_with("npm (package.json)"), "{only_npm}");
    assert!(!only_npm.contains("Makefile"), "{only_npm}");
    assert_eq!(s.r(&["npm"]).stdout, only_npm);

    let run = s.r(&["list", "just"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("no file for `just`"), "{run:?}");
}

#[test]
fn list_as_json() {
    let s = Sandbox::new("list-json");
    s.makefile().package_json();
    let json: serde_json::Value = serde_json::from_str(&s.r(&["list", "--json"]).stdout).unwrap();
    let runners = json["runners"].as_array().unwrap();
    assert_eq!(runners.len(), 2);
    assert_eq!(runners[0]["name"], "make");
    assert_eq!(runners[0]["tasks"][0]["name"], "build");
    assert_eq!(runners[0]["tasks"][0]["description"], "Build with make");
    assert_eq!(runners[0]["tasks"][0]["ambiguous"], true);
    assert_eq!(runners[0]["tasks"][1]["ambiguous"], false);
    assert_eq!(runners[1]["tasks"][1]["name"], "start");
}

#[test]
fn a_task_named_like_a_builtin_is_reachable() {
    let s = Sandbox::new("shadowed");
    s.makefile();
    assert_eq!(s.r(&["run", "list"]).invocation(), "make list");
    assert_eq!(s.r(&["make", "list"]).invocation(), "make list");
    assert_eq!(
        s.r(&["run", "make", "list", "x"]).invocation(),
        "make list x"
    );
}

#[test]
fn which_prints_the_command_without_running_it() {
    let s = Sandbox::new("which");
    s.makefile().package_json();
    let run = s.r(&["which", "start", "--name", "two words"]);
    assert_eq!(run.stdout, "npm run start -- --name 'two words'\n");
    assert_eq!(run.stderr, "");
    assert_eq!(s.r(&["which", "pnpm", "build"]).stdout, "pnpm run build\n");
    assert_eq!(s.r(&["which", "build"]).code, 2);
    // No task given: usage error. `help` is a task name here, not a built-in.
    assert_eq!(s.r(&["which"]).code, 2);
    let run = s.r(&["which", "help"]);
    assert!(run.stderr.contains("unknown task `help`\n"), "{run:?}");
}

#[test]
fn unknown_tasks_get_suggestions() {
    let s = Sandbox::new("unknown");
    s.makefile().package_json();
    let run = s.r(&["delpoy"]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr
            .contains("unknown task `delpoy`, did you mean `deploy`?"),
        "{run:?}"
    );
    let run = s.r(&["npm", "strat"]);
    assert_eq!(run.code, 2);
    assert!(
        run.stderr
            .contains("package.json has no task `strat`, did you mean `start`?"),
        "{run:?}"
    );
}

#[test]
fn without_runner_files_there_is_a_clear_error() {
    let s = Sandbox::new("empty");
    for args in [&["list"][..], &["build"], &[]] {
        let run = s.r(args);
        assert_eq!(run.code, 1, "{run:?}");
        assert!(run.stderr.contains("no Makefile, package.json"), "{run:?}");
    }
}

#[test]
fn the_search_does_not_leave_the_git_repository() {
    let s = Sandbox::new("git-boundary");
    // A Makefile above the repository must not be picked up.
    fs::write(s.root.join("Makefile"), "outside:\n\t@true\n").unwrap();
    assert_eq!(s.r(&["outside"]).code, 1);
}

#[test]
fn a_broken_file_is_a_warning_not_a_failure() {
    let s = Sandbox::new("broken");
    s.makefile().file("package.json", "{ not json");
    let run = s.r(&["deploy"]);
    assert_eq!(run.invocation(), "make deploy");
    assert!(run.stderr.contains("warning:"), "{run:?}");
    assert!(run.stderr.contains("package.json"), "{run:?}");
}

#[test]
fn completion_candidates() {
    let s = Sandbox::new("complete");
    s.makefile().package_json();
    let complete = |args: &[&str]| s.r(args).stdout;

    // bash: the raw line, only matching words, colon prefix removed.
    assert_eq!(
        complete(&["__complete", "bash", "--line", "r bu"]),
        "build\nbuild:prod\n"
    );
    assert_eq!(
        complete(&["__complete", "bash", "--line", "r build:"]),
        "prod\n"
    );
    assert_eq!(
        complete(&["__complete", "bash", "--line", "r npm "]),
        "build\nstart\nbuild:prod\n"
    );
    assert_eq!(
        complete(&["__complete", "bash", "--line", "r make d"]),
        "deploy\n"
    );
    // After the task, r stays out of the way (the shell completes files).
    assert_eq!(complete(&["__complete", "bash", "--line", "r start "]), "");
    assert_eq!(complete(&["__complete", "bash", "--line", "r -C "]), "");

    // zsh: group, value and description for everything on offer.
    let zsh = complete(&["__complete", "zsh", "--", ""]);
    assert!(
        zsh.contains("task\tbuild\tambiguous, prefix with make or npm\n"),
        "{zsh}"
    );
    assert!(zsh.contains("task\tstart\tnpm: node .\n"), "{zsh}");
    assert!(zsh.contains("runner\tmake\ttasks from Makefile\n"), "{zsh}");
    assert!(zsh.contains("subcommand\tlist\t"), "{zsh}");
    assert!(
        !zsh.contains("task\tlist\t"),
        "the built-in `list` wins: {zsh}"
    );

    // fish: value and description.
    assert_eq!(
        complete(&["__complete", "fish", "--", "list", ""]),
        "make\ttasks from Makefile\nnpm\ttasks from package.json\n"
    );
    assert_eq!(
        complete(&["__complete", "fish", "--", "completions", ""]),
        "bash\t\nzsh\t\nfish\t\n"
    );
    // `run` offers tasks and runners, but no built-ins.
    let run = complete(&["__complete", "fish", "--", "run", ""]);
    assert!(
        run.contains("list\tmake\n") && !run.contains("completions"),
        "{run}"
    );
}

#[test]
fn completion_honours_the_dir_option_and_survives_no_project() {
    let s = Sandbox::new("complete-dir");
    s.makefile();
    let run = s.r_in(
        "bin",
        &["__complete", "bash", "--line", "r -C ../project de"],
    );
    assert_eq!(run.stdout, "deploy\n");

    let empty = Sandbox::new("complete-empty");
    let run = empty.r(&["__complete", "bash", "--line", "r "]);
    assert_eq!(run.code, 0);
    assert_eq!(run.stderr, "");
    assert!(run.stdout.contains("list\n"), "{run:?}");
}

#[test]
fn completion_scripts_are_printed_for_each_shell() {
    let s = Sandbox::new("scripts");
    for (shell, marker) in [
        (
            "bash",
            "complete -o bashdefault -o default -F _r_complete r",
        ),
        ("zsh", "#compdef r"),
        ("fish", "complete -c r"),
    ] {
        let run = s.r(&["completions", shell]);
        assert_eq!(run.code, 0);
        assert!(run.stdout.contains(marker), "{shell}: {}", run.stdout);
        assert!(run.stdout.contains("__complete"), "{shell}");
    }
    assert_eq!(s.r(&["completions", "tcsh"]).code, 2);
}
