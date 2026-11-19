//! Finding the project directory and the runners available in it.

use std::path::{Path, PathBuf};

use crate::runner::{self, Runner};

#[derive(Debug)]
pub struct Project {
    /// The directory the runner files live in; tasks are run from here.
    pub dir: PathBuf,
    pub runners: Vec<Runner>,
    /// Runner files that exist but could not be loaded.
    pub warnings: Vec<String>,
}

impl Project {
    /// The nearest directory, starting at `start` and walking up, that holds
    /// at least one runner file. The search does not leave the git repository
    /// `start` is in.
    pub fn discover(start: &Path) -> Option<Project> {
        let start = std::path::absolute(start).ok()?;
        for dir in start.ancestors() {
            let detection = runner::detect_all(dir);
            if !detection.is_empty() {
                return Some(Project {
                    dir: dir.to_path_buf(),
                    runners: detection.runners,
                    warnings: detection.warnings,
                });
            }
            if dir.join(".git").exists() {
                break;
            }
        }
        None
    }

    /// The runner addressed by `name` (`r <name> <task>`).
    pub fn runner_named(&self, name: &str) -> Option<&Runner> {
        self.runners.iter().find(|r| r.answers_to(name))
    }

    /// All runners that define a task called `task`.
    pub fn owners(&self, task: &str) -> Vec<&Runner> {
        self.runners
            .iter()
            .filter(|r| r.task(task).is_some())
            .collect()
    }

    /// Task names in order of first appearance, without duplicates.
    pub fn task_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for task in self.runners.iter().flat_map(|r| &r.tasks) {
            if !names.contains(&task.name.as_str()) {
                names.push(&task.name);
            }
        }
        names
    }
}
