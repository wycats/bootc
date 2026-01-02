//! Output helpers for consistent CLI output.
//!
//! Provides standardized output formatting following cargo-like patterns:
//! - Status messages with colored prefixes
//! - Spinners for long-running operations
//! - Progress bars for multi-step operations
//!
//! # Example
//!
//! ```rust,ignore
//! use bkt::output::Output;
//!
//! Output::success("Installed 3 packages");
//! Output::error("Failed to connect");
//! Output::warning("Package already exists");
//! Output::info("Checking manifest...");
//!
//! let spinner = Output::spinner("Installing packages...");
//! // ... do work ...
//! spinner.finish_success("Installed 3 packages");
//! ```

use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use std::borrow::Cow;
use std::time::Duration;

/// Standard output helper for consistent CLI formatting.
pub struct Output;

impl Output {
    /// Print a success message with a green checkmark.
    ///
    /// Example: `✓ Installed 3 packages`
    pub fn success(msg: impl AsRef<str>) {
        println!("{} {}", "✓".green().bold(), msg.as_ref());
    }

    /// Print an error message with a red X to stderr.
    ///
    /// Example: `✗ Failed to install package`
    pub fn error(msg: impl AsRef<str>) {
        eprintln!("{} {}", "✗".red().bold(), msg.as_ref().red());
    }

    /// Print a warning message with a yellow warning symbol.
    ///
    /// Example: `⚠ Package already exists`
    pub fn warning(msg: impl AsRef<str>) {
        println!("{} {}", "⚠".yellow(), msg.as_ref());
    }

    /// Print an info/status message with a cyan arrow.
    ///
    /// Example: `→ Checking manifest...`
    pub fn info(msg: impl AsRef<str>) {
        println!("{} {}", "→".cyan(), msg.as_ref().dimmed());
    }

    /// Print a step message (for multi-step operations).
    ///
    /// Example: `• Processing flatpaks`
    pub fn step(msg: impl AsRef<str>) {
        println!("  {} {}", "•".cyan(), msg.as_ref());
    }

    /// Print a header/section title.
    ///
    /// Example: `=== Development Toolbox Status ===`
    pub fn header(msg: impl AsRef<str>) {
        println!("\n{}\n", msg.as_ref().bold().cyan());
    }

    /// Print a subheader for sections within output.
    ///
    /// Example: `PACKAGES:`
    pub fn subheader(msg: impl AsRef<str>) {
        println!("{}", msg.as_ref().bold());
    }

    /// Print an item in a list (indented).
    ///
    /// Example: `  gcc`
    pub fn list_item(msg: impl AsRef<str>) {
        println!("  {}", msg.as_ref());
    }

    /// Print a key-value pair with alignment.
    ///
    /// Example: `  Source:        user`
    pub fn kv(key: impl AsRef<str>, value: impl AsRef<str>) {
        println!("  {:<14} {}", format!("{}:", key.as_ref()).cyan(), value.as_ref());
    }

    /// Print a hint/suggestion message (indented with arrow).
    ///
    /// Example: `  → Run: gh auth login`
    pub fn hint(msg: impl AsRef<str>) {
        println!("  {} {}", "→".cyan(), msg.as_ref());
    }

    /// Print a dry-run message.
    ///
    /// Example: `[dry-run] Would install: gcc`
    pub fn dry_run(msg: impl AsRef<str>) {
        println!("{} {}", "[dry-run]".dimmed(), msg.as_ref().dimmed());
    }

    /// Print the running command (for transparency).
    ///
    /// Example: `Running: rpm-ostree install gcc`
    pub fn running(cmd: impl AsRef<str>) {
        println!("{} {}", "Running:".dimmed(), cmd.as_ref().dimmed());
    }

    /// Create a spinner for long-running operations.
    ///
    /// The spinner will animate until you call `finish_*` on it.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let spinner = Output::spinner("Installing packages...");
    /// install_packages()?;
    /// spinner.finish_success("Installed 3 packages");
    /// ```
    pub fn spinner(msg: impl Into<Cow<'static, str>>) -> Spinner {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .expect("valid template"),
        );
        pb.set_message(msg);
        pb.enable_steady_tick(Duration::from_millis(80));
        Spinner(pb)
    }

    /// Create a progress bar for multi-step operations.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let progress = Output::progress(packages.len() as u64, "Installing packages");
    /// for pkg in packages {
    ///     progress.set_message(format!("Installing {}", pkg));
    ///     install(pkg)?;
    ///     progress.inc(1);
    /// }
    /// progress.finish_success("Installed all packages");
    /// ```
    pub fn progress(total: u64, msg: impl Into<Cow<'static, str>>) -> Progress {
        let pb = ProgressBar::new(total);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg}\n  {bar:40.cyan/blue} {pos}/{len}")
                .expect("valid template")
                .progress_chars("█▓░"),
        );
        pb.set_message(msg);
        Progress(pb)
    }

    /// Print a separator line.
    pub fn separator() {
        println!("{}", "-".repeat(50).dimmed());
    }

    /// Print a blank line.
    pub fn blank() {
        println!();
    }
}

/// A spinner for long-running operations.
///
/// Created via `Output::spinner()`.
pub struct Spinner(ProgressBar);

impl Spinner {
    /// Update the spinner message.
    pub fn set_message(&self, msg: impl Into<Cow<'static, str>>) {
        self.0.set_message(msg);
    }

    /// Finish with a success message.
    pub fn finish_success(self, msg: impl AsRef<str>) {
        self.0
            .finish_with_message(format!("{} {}", "✓".green().bold(), msg.as_ref()));
    }

    /// Finish with an error message.
    pub fn finish_error(self, msg: impl AsRef<str>) {
        self.0
            .finish_with_message(format!("{} {}", "✗".red().bold(), msg.as_ref()));
    }

    /// Finish with a warning message.
    pub fn finish_warning(self, msg: impl AsRef<str>) {
        self.0
            .finish_with_message(format!("{} {}", "⚠".yellow(), msg.as_ref()));
    }

    /// Finish and clear the line (no final message).
    pub fn finish_clear(self) {
        self.0.finish_and_clear();
    }
}

/// A progress bar for multi-step operations.
///
/// Created via `Output::progress()`.
pub struct Progress(ProgressBar);

impl Progress {
    /// Update the progress message.
    pub fn set_message(&self, msg: impl Into<Cow<'static, str>>) {
        self.0.set_message(msg);
    }

    /// Increment progress by the given amount.
    pub fn inc(&self, delta: u64) {
        self.0.inc(delta);
    }

    /// Set the current position.
    pub fn set_position(&self, pos: u64) {
        self.0.set_position(pos);
    }

    /// Finish with a success message.
    pub fn finish_success(self, msg: impl AsRef<str>) {
        self.0
            .finish_with_message(format!("{} {}", "✓".green().bold(), msg.as_ref()));
    }

    /// Finish with an error message.
    pub fn finish_error(self, msg: impl AsRef<str>) {
        self.0
            .finish_with_message(format!("{} {}", "✗".red().bold(), msg.as_ref()));
    }

    /// Finish and clear the progress bar.
    pub fn finish_clear(self) {
        self.0.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_methods_dont_panic() {
        // Just verify these don't panic - actual output is tested manually
        Output::success("test");
        Output::error("test");
        Output::warning("test");
        Output::info("test");
        Output::step("test");
        Output::hint("test");
        Output::dry_run("test");
        Output::running("test");
        Output::kv("key", "value");
        Output::separator();
        Output::blank();
    }

    #[test]
    fn test_spinner_lifecycle() {
        let spinner = Output::spinner("Testing...");
        spinner.set_message("Still testing...");
        spinner.finish_success("Done");
    }

    #[test]
    fn test_progress_lifecycle() {
        let progress = Output::progress(10, "Processing...");
        progress.inc(5);
        progress.set_position(8);
        progress.finish_success("Complete");
    }
}
