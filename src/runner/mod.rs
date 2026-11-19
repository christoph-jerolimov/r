//! Runner definitions: which files they are detected from, which tasks they
//! offer and how a task is turned into a command line.
//!
//! To add a runner: add a `Kind`, a module with a `detect` function, register
//! it in `detect_all`, and teach `Runner::command` how to invoke it.

use std::path::{Path, PathBuf};

mod cargo;
mod deno;
mod just;
mod make;
mod node;
mod taskfile;

/// Tools that all read their tasks from `package.json`.
pub const NODE_TOOLS: &[&str] = &["npm", "yarn", "pnpm", "bun"];

/// Every name that can be used as a runner qualifier (`r <runner> <task>`).
pub const RUNNER_NAMES: &[&str] = &[
    "make", "npm", "yarn", "pnpm", "bun", "just", "task", "deno", "cargo",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub description: Option<String>,
}

impl Task {
    pub fn new(name: impl Into<String>, description: Option<String>) -> Self {
        Task {
            name: name.into(),
            description: description.filter(|d| !d.trim().is_empty()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Make,
    Node,
    Just,
    Taskfile,
    Deno,
    Cargo,
}

#[derive(Debug, Clone)]
pub struct Runner {
    pub kind: Kind,
    /// The program that runs the tasks. Doubles as the runner's name.
    pub tool: String,
    /// The file the tasks were read from.
    pub file: PathBuf,
    pub tasks: Vec<Task>,
}

impl Runner {
    pub fn name(&self) -> &str {
        &self.tool
    }

    pub fn file_name(&self) -> String {
        self.file
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Whether `name` addresses this runner. A `package.json` answers to every
    /// node package manager, so `r npm start` works in a yarn project too.
    pub fn answers_to(&self, name: &str) -> bool {
        self.tool == name || (self.kind == Kind::Node && NODE_TOOLS.contains(&name))
    }

    pub fn task(&self, name: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.name == name)
    }

    /// The tool to use when the runner was addressed as `qualifier`.
    pub fn tool_for(&self, qualifier: &str) -> String {
        if self.kind == Kind::Node && NODE_TOOLS.contains(&qualifier) {
            qualifier.to_string()
        } else {
            self.tool.clone()
        }
    }

    /// The full command line (program first) that runs `task` with `args`.
    pub fn command(&self, tool: &str, task: &str, args: &[String]) -> Vec<String> {
        let mut cmd = vec![tool.to_string()];
        // Some tools need `--` so that extra arguments reach the task instead
        // of being interpreted by the tool itself.
        let mut separator = false;
        match self.kind {
            Kind::Make | Kind::Just | Kind::Cargo => {}
            Kind::Node => {
                cmd.push("run".into());
                separator = tool == "npm";
            }
            Kind::Taskfile => separator = true,
            Kind::Deno => cmd.push("task".into()),
        }
        cmd.push(task.to_string());
        if !args.is_empty() {
            if separator {
                cmd.push("--".into());
            }
            cmd.extend(args.iter().cloned());
        }
        cmd
    }
}

/// Result of looking for runner files in one directory.
#[derive(Debug, Default)]
pub struct Detection {
    pub runners: Vec<Runner>,
    /// Runner files that exist but could not be read or parsed.
    pub warnings: Vec<String>,
}

impl Detection {
    pub fn is_empty(&self) -> bool {
        self.runners.is_empty() && self.warnings.is_empty()
    }
}

/// What a single runner module reports for a directory: `None` when its file
/// is absent, otherwise the runner or the reason it could not be loaded.
type Detected = Option<Result<Runner, String>>;

pub fn detect_all(dir: &Path) -> Detection {
    let mut detection = Detection::default();
    let detectors: [fn(&Path) -> Detected; 6] = [
        make::detect,
        node::detect,
        just::detect,
        taskfile::detect,
        deno::detect,
        cargo::detect,
    ];
    for detect in detectors {
        match detect(dir) {
            None => {}
            Some(Ok(runner)) => detection.runners.push(runner),
            Some(Err(warning)) => detection.warnings.push(warning),
        }
    }
    detection
}

/// File names looked for, used in "nothing found" messages.
pub const FILE_HINT: &str =
    "Makefile, package.json, justfile, Taskfile.yml, deno.json or Cargo.toml";

/// The first of `names` that exists as a file in `dir`.
///
/// The directory is listed rather than probed so that the on-disk spelling is
/// returned on case-insensitive file systems.
fn find_file(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    let present: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names
        .iter()
        .find(|name| present.iter().any(|p| p == *name))
        .map(|name| dir.join(name))
}

fn read(file: &Path) -> Result<String, String> {
    std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner(kind: Kind, tool: &str) -> Runner {
        Runner {
            kind,
            tool: tool.into(),
            file: PathBuf::from("x"),
            tasks: vec![],
        }
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn command_lines() {
        let extra = args(&["--watch", "x"]);
        let cases = [
            (Kind::Make, "make", "make build --watch x"),
            (Kind::Node, "npm", "npm run build -- --watch x"),
            (Kind::Node, "yarn", "yarn run build --watch x"),
            (Kind::Node, "pnpm", "pnpm run build --watch x"),
            (Kind::Node, "bun", "bun run build --watch x"),
            (Kind::Just, "just", "just build --watch x"),
            (Kind::Taskfile, "task", "task build -- --watch x"),
            (Kind::Deno, "deno", "deno task build --watch x"),
            (Kind::Cargo, "cargo", "cargo build --watch x"),
        ];
        for (kind, tool, expected) in cases {
            let r = runner(kind, tool);
            assert_eq!(r.command(tool, "build", &extra).join(" "), expected);
        }
        let npm = runner(Kind::Node, "npm");
        assert_eq!(npm.command("npm", "build", &[]).join(" "), "npm run build");
    }

    #[test]
    fn package_json_answers_to_every_node_tool() {
        let r = runner(Kind::Node, "yarn");
        assert!(r.answers_to("yarn") && r.answers_to("npm") && r.answers_to("bun"));
        assert!(!r.answers_to("make"));
        assert_eq!(r.tool_for("npm"), "npm");
        assert_eq!(r.tool_for("yarn"), "yarn");
        let make = runner(Kind::Make, "make");
        assert!(make.answers_to("make") && !make.answers_to("npm"));
    }
}
