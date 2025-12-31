//! Effect system for dry-run support.
//!
//! Provides an `Executor` that can either perform operations or report what would happen.

use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

/// Represents a side effect the CLI can perform.
#[derive(Debug, Clone)]
pub enum Effect {
    WriteFile {
        path: PathBuf,
        description: String,
    },
    DeleteFile {
        path: PathBuf,
        description: String,
    },
    RunCommand {
        program: String,
        args: Vec<String>,
        description: String,
    },
    GitCreateBranch {
        branch_name: String,
    },
    GitCommit {
        message: String,
    },
    GitPush,
    CreatePullRequest {
        title: String,
    },
}

impl Effect {
    /// Human-readable description for dry-run output.
    pub fn describe(&self) -> String {
        match self {
            Effect::WriteFile { path, description } => {
                format!("Write {}: {}", path.display(), description)
            }
            Effect::DeleteFile { path, description } => {
                format!("Delete {}: {}", path.display(), description)
            }
            Effect::RunCommand {
                program,
                args,
                description,
            } => {
                format!("Run `{} {}`: {}", program, args.join(" "), description)
            }
            Effect::GitCreateBranch { branch_name } => {
                format!("Create git branch: {}", branch_name)
            }
            Effect::GitCommit { message } => {
                format!("Git commit: {}", message)
            }
            Effect::GitPush => "Push to remote".to_string(),
            Effect::CreatePullRequest { title } => {
                format!("Create PR: {}", title)
            }
        }
    }
}

/// Execution context that tracks and optionally performs effects.
pub struct Executor {
    dry_run: bool,
    effects: Vec<Effect>,
}

impl Executor {
    /// Create a new executor.
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            effects: Vec::new(),
        }
    }

    /// Returns true if in dry-run mode.
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// Write a file, or record the intent in dry-run mode.
    pub fn write_file(&mut self, path: &Path, content: &str, description: &str) -> Result<()> {
        let effect = Effect::WriteFile {
            path: path.to_path_buf(),
            description: description.to_string(),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
            Ok(())
        } else {
            debug!(path = %path.display(), "Writing file");
            std::fs::write(path, content)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            info!(path = %path.display(), "Wrote file");
            Ok(())
        }
    }

    /// Delete a file, or record the intent in dry-run mode.
    pub fn delete_file(&mut self, path: &Path, description: &str) -> Result<()> {
        let effect = Effect::DeleteFile {
            path: path.to_path_buf(),
            description: description.to_string(),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
            Ok(())
        } else {
            debug!(path = %path.display(), "Deleting file");
            std::fs::remove_file(path)
                .with_context(|| format!("Failed to delete {}", path.display()))?;
            info!(path = %path.display(), "Deleted file");
            Ok(())
        }
    }

    /// Run a command, or record the intent in dry-run mode.
    /// Returns true if the command succeeded (or would succeed in dry-run).
    pub fn run_command(&mut self, program: &str, args: &[&str], description: &str) -> Result<bool> {
        let effect = Effect::RunCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            description: description.to_string(),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
            Ok(true) // Assume success in dry-run
        } else {
            debug!(program, ?args, "Running command");
            let status = Command::new(program)
                .args(args)
                .status()
                .with_context(|| format!("Failed to run {}", program))?;
            info!(program, success = status.success(), "Command completed");
            Ok(status.success())
        }
    }

    /// Run a command in a specific directory.
    pub fn run_command_in_dir(
        &mut self,
        program: &str,
        args: &[&str],
        dir: &Path,
        description: &str,
    ) -> Result<bool> {
        let effect = Effect::RunCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            description: format!("{} (in {})", description, dir.display()),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
            Ok(true)
        } else {
            debug!(program, ?args, dir = %dir.display(), "Running command in directory");
            let status = Command::new(program)
                .args(args)
                .current_dir(dir)
                .status()
                .with_context(|| format!("Failed to run {} in {}", program, dir.display()))?;
            info!(program, success = status.success(), "Command completed");
            Ok(status.success())
        }
    }

    /// Record a git branch creation.
    pub fn git_create_branch(&mut self, branch_name: &str) -> Result<()> {
        let effect = Effect::GitCreateBranch {
            branch_name: branch_name.to_string(),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
        }
        Ok(())
    }

    /// Record a git commit.
    pub fn git_commit(&mut self, message: &str) -> Result<()> {
        let effect = Effect::GitCommit {
            message: message.to_string(),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
        }
        Ok(())
    }

    /// Record a git push.
    pub fn git_push(&mut self) -> Result<()> {
        let effect = Effect::GitPush;

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
        }
        Ok(())
    }

    /// Record a PR creation.
    pub fn create_pull_request(&mut self, title: &str) -> Result<()> {
        let effect = Effect::CreatePullRequest {
            title: title.to_string(),
        };

        if self.dry_run {
            println!("  {} {}", "Would:".cyan(), effect.describe());
            self.effects.push(effect);
        }
        Ok(())
    }

    /// Get the list of effects that would be performed.
    pub fn effects(&self) -> &[Effect] {
        &self.effects
    }

    /// Print a summary of effects (for dry-run mode).
    pub fn summarize(&self) {
        if self.dry_run && !self.effects.is_empty() {
            println!(
                "\n{} {} operations would be performed",
                "[DRY-RUN]".yellow().bold(),
                self.effects.len()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_dry_run_collects_effects() {
        let mut exec = Executor::new(true);

        exec.write_file(Path::new("/tmp/test"), "content", "test file")
            .unwrap();
        exec.run_command("echo", &["hello"], "test command")
            .unwrap();

        assert_eq!(exec.effects().len(), 2);
        assert!(exec.is_dry_run());
    }

    #[test]
    fn test_real_mode_executes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        let mut exec = Executor::new(false);
        exec.write_file(&path, "hello", "test").unwrap();

        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert!(exec.effects().is_empty()); // Effects not tracked in real mode
    }
}
