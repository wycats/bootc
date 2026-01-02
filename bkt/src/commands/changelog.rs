//! Changelog command implementation.
//!
//! This module provides commands for managing the distribution changelog as specified in RFC-0005.

use crate::manifest::{
    ChangeCategory, ChangeType, ChangelogEntry, ChangelogManager, VersionMetadata, find_repo_root,
};
use crate::output::Output;
use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use std::env;

#[derive(Debug, Args)]
pub struct ChangelogArgs {
    #[command(subcommand)]
    pub action: ChangelogAction,
}

#[derive(Debug, Subcommand)]
pub enum ChangelogAction {
    /// Show changelog entries
    Show {
        /// Version to show (default: recent changes)
        version: Option<String>,

        /// Show all versions
        #[arg(long)]
        all: bool,

        /// Number of versions to show
        #[arg(short, long, default_value = "5")]
        count: usize,
    },

    /// Show pending changes (not yet released)
    Pending,

    /// Generate a changelog entry for the current changes
    Generate {
        /// Type of change
        #[arg(short, long, value_parser = parse_change_type)]
        r#type: ChangeType,

        /// Category of change
        #[arg(short, long, value_parser = parse_category)]
        category: ChangeCategory,

        /// Description of the change
        message: String,

        /// Mark as draft (cannot be merged until finalized)
        #[arg(long)]
        draft: bool,
    },

    /// Add a changelog entry
    Add {
        /// Type of change
        #[arg(short, long, value_parser = parse_change_type)]
        r#type: ChangeType,

        /// Category of change
        #[arg(short, long, value_parser = parse_category)]
        category: ChangeCategory,

        /// Description of the change
        message: String,

        /// Mark as draft
        #[arg(long)]
        draft: bool,
    },

    /// Validate changelog entries (no drafts, proper format)
    Validate,

    /// List available versions
    List {
        /// Number of versions to list
        #[arg(short, long, default_value = "10")]
        count: usize,
    },

    /// Create a new version from pending entries
    Release {
        /// Version string (default: auto-generated YYYY.MM.DD.N)
        version: Option<String>,

        /// Don't update CHANGELOG.md
        #[arg(long)]
        no_update: bool,
    },

    /// Clear all pending entries (use with caution)
    Clear {
        /// Confirm clearing pending entries
        #[arg(long)]
        confirm: bool,
    },
}

fn parse_change_type(s: &str) -> Result<ChangeType, String> {
    match s.to_lowercase().as_str() {
        "added" | "add" => Ok(ChangeType::Added),
        "changed" | "change" => Ok(ChangeType::Changed),
        "removed" | "remove" => Ok(ChangeType::Removed),
        "fixed" | "fix" => Ok(ChangeType::Fixed),
        "security" | "sec" => Ok(ChangeType::Security),
        "deprecated" | "deprecate" => Ok(ChangeType::Deprecated),
        _ => Err(format!(
            "Invalid change type '{}'. Valid: added, changed, removed, fixed, security, deprecated",
            s
        )),
    }
}

fn parse_category(s: &str) -> Result<ChangeCategory, String> {
    match s.to_lowercase().as_str() {
        "flatpak" => Ok(ChangeCategory::Flatpak),
        "flatpak-remote" | "flatpakremote" | "remote" => Ok(ChangeCategory::FlatpakRemote),
        "package" | "pkg" | "dnf" | "rpm" => Ok(ChangeCategory::Package),
        "toolbox" | "toolbox-package" | "dev" => Ok(ChangeCategory::ToolboxPackage),
        "extension" | "ext" => Ok(ChangeCategory::Extension),
        "gsetting" | "gsettings" | "setting" => Ok(ChangeCategory::Gsetting),
        "shim" => Ok(ChangeCategory::Shim),
        "upstream" => Ok(ChangeCategory::Upstream),
        "copr" => Ok(ChangeCategory::Copr),
        "system" => Ok(ChangeCategory::System),
        "other" => Ok(ChangeCategory::Other),
        _ => Err(format!(
            "Invalid category '{}'. Valid: flatpak, package, toolbox, extension, gsetting, shim, upstream, copr, system, other",
            s
        )),
    }
}

fn get_changelog_manager() -> Result<ChangelogManager> {
    let cwd = env::current_dir().context("Failed to get current directory")?;
    let root = find_repo_root(&cwd)
        .context("Not in a git repository. Run this command from within the bootc repository.")?;
    Ok(ChangelogManager::new(root))
}

pub fn run(args: ChangelogArgs) -> Result<()> {
    match args.action {
        ChangelogAction::Show {
            version,
            all,
            count,
        } => handle_show(version, all, count),
        ChangelogAction::Pending => handle_pending(),
        ChangelogAction::Generate {
            r#type,
            category,
            message,
            draft,
        } => handle_generate(r#type, category, message, draft),
        ChangelogAction::Add {
            r#type,
            category,
            message,
            draft,
        } => handle_add(r#type, category, message, draft),
        ChangelogAction::Validate => handle_validate(),
        ChangelogAction::List { count } => handle_list(count),
        ChangelogAction::Release { version, no_update } => handle_release(version, no_update),
        ChangelogAction::Clear { confirm } => handle_clear(confirm),
    }
}

fn handle_show(version: Option<String>, all: bool, count: usize) -> Result<()> {
    let manager = get_changelog_manager()?;

    if let Some(ver) = version {
        // Show specific version
        if let Some(metadata) = manager.load_version(&ver)? {
            println!("{}", metadata.format_for_changelog());
        } else {
            Output::warning(format!("Version {} not found", ver));
        }
        return Ok(());
    }

    let versions = manager.list_versions()?;
    if versions.is_empty() {
        Output::info("No versions released yet.");
        return Ok(());
    }

    let limit = if all { versions.len() } else { count };

    for ver in versions.iter().take(limit) {
        if let Some(metadata) = manager.load_version(ver)? {
            println!("{}", metadata.format_for_changelog());
        }
    }

    if !all && versions.len() > count {
        println!(
            "{}",
            format!(
                "... and {} more versions (use --all to see all)",
                versions.len() - count
            )
            .dimmed()
        );
    }

    Ok(())
}

fn handle_pending() -> Result<()> {
    let manager = get_changelog_manager()?;
    let entries = manager.load_pending()?;

    if entries.is_empty() {
        Output::info("No pending changelog entries.");
        return Ok(());
    }

    Output::header("Pending Changelog Entries");
    println!();

    for entry in &entries {
        let draft_marker = if entry.draft {
            " [DRAFT]".yellow().to_string()
        } else {
            String::new()
        };

        let type_color = match entry.change_type {
            ChangeType::Added => "Added".green().to_string(),
            ChangeType::Changed => "Changed".blue().to_string(),
            ChangeType::Removed => "Removed".red().to_string(),
            ChangeType::Fixed => "Fixed".cyan().to_string(),
            ChangeType::Security => "Security".magenta().to_string(),
            ChangeType::Deprecated => "Deprecated".yellow().to_string(),
        };

        println!(
            "  {} {} {}{}",
            type_color,
            format!("[{}]", entry.category).dimmed(),
            entry.message,
            draft_marker
        );

        if let Some(ref cmd) = entry.command {
            println!("    {}", format!("Command: {}", cmd).dimmed());
        }
    }

    println!();
    Output::info(format!("Total: {} pending entries", entries.len()));

    if manager.has_draft_entries()? {
        Output::warning("Some entries are marked as drafts and cannot be released.");
    }

    Ok(())
}

fn handle_generate(
    change_type: ChangeType,
    category: ChangeCategory,
    message: String,
    draft: bool,
) -> Result<()> {
    // Just prints what would be generated without saving
    let mut entry = ChangelogEntry::new(change_type, category, &message);
    if draft {
        entry = entry.into_draft();
    }

    Output::header("Generated Changelog Entry");
    println!();
    println!("{}", serde_yaml::to_string(&entry)?);
    println!();
    Output::info("Use 'bkt changelog add' to save this entry.");

    Ok(())
}

fn handle_add(
    change_type: ChangeType,
    category: ChangeCategory,
    message: String,
    draft: bool,
) -> Result<()> {
    let manager = get_changelog_manager()?;

    let mut entry = ChangelogEntry::new(change_type, category, &message);
    if draft {
        entry = entry.into_draft();
    }

    let path = manager.add_pending(&entry)?;
    Output::success(format!(
        "Added changelog entry: {} - {}",
        change_type, message
    ));
    Output::info(format!("Saved to: {}", path.display()));

    Ok(())
}

fn handle_validate() -> Result<()> {
    let manager = get_changelog_manager()?;
    let entries = manager.load_pending()?;

    if entries.is_empty() {
        Output::info("No pending entries to validate.");
        return Ok(());
    }

    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    for (i, entry) in entries.iter().enumerate() {
        if entry.draft {
            errors.push(format!(
                "Entry {}: Draft entry cannot be released: {}",
                i + 1,
                entry.message
            ));
        }

        if entry.message.is_empty() {
            errors.push(format!("Entry {}: Empty message", i + 1));
        }

        if entry.message.len() > 200 {
            warnings.push(format!(
                "Entry {}: Message is quite long ({} chars)",
                i + 1,
                entry.message.len()
            ));
        }
    }

    if errors.is_empty() && warnings.is_empty() {
        Output::success(format!(
            "All {} changelog entries are valid.",
            entries.len()
        ));
        return Ok(());
    }

    if !warnings.is_empty() {
        Output::header("Warnings");
        for warning in &warnings {
            Output::warning(warning);
        }
    }

    if !errors.is_empty() {
        Output::header("Errors");
        for error in &errors {
            Output::error(error);
        }
        bail!("Validation failed with {} errors", errors.len());
    }

    Ok(())
}

fn handle_list(count: usize) -> Result<()> {
    let manager = get_changelog_manager()?;
    let versions = manager.list_versions()?;

    if versions.is_empty() {
        Output::info("No versions released yet.");
        return Ok(());
    }

    Output::header("Released Versions");
    println!();

    for ver in versions.iter().take(count) {
        if let Some(metadata) = manager.load_version(ver)? {
            let change_count = metadata.changes.len();
            println!(
                "  {} {} ({} changes)",
                ver.cyan(),
                format!("({})", metadata.date.format("%Y-%m-%d")).dimmed(),
                change_count
            );
        } else {
            println!("  {}", ver.cyan());
        }
    }

    if versions.len() > count {
        println!(
            "\n{}",
            format!("... and {} more versions", versions.len() - count).dimmed()
        );
    }

    Ok(())
}

fn handle_release(version: Option<String>, no_update: bool) -> Result<()> {
    let manager = get_changelog_manager()?;

    // Check for pending entries
    let pending_count = manager.pending_count()?;
    if pending_count == 0 {
        Output::warning("No pending entries to release.");
        return Ok(());
    }

    // Check for drafts
    if manager.has_draft_entries()? {
        bail!(
            "Cannot release: there are draft entries. Finalize them first or remove the draft flag."
        );
    }

    // Determine version
    let version_str = version.unwrap_or_else(VersionMetadata::next_version_for_today);

    Output::info(format!(
        "Creating version {} with {} entries...",
        version_str, pending_count
    ));

    // Create the version
    let metadata = manager.create_version(&version_str)?;

    // Update CHANGELOG.md
    if !no_update {
        manager.update_changelog_file(&metadata)?;
        Output::success(format!("Updated CHANGELOG.md with version {}", version_str));
    }

    Output::success(format!(
        "Released version {} with {} changes",
        version_str,
        metadata.changes.len()
    ));

    Ok(())
}

fn handle_clear(confirm: bool) -> Result<()> {
    if !confirm {
        Output::warning("This will delete all pending changelog entries.");
        Output::info("Use --confirm to proceed.");
        return Ok(());
    }

    let manager = get_changelog_manager()?;
    let count = manager.pending_count()?;

    if count == 0 {
        Output::info("No pending entries to clear.");
        return Ok(());
    }

    manager.clear_pending()?;
    Output::success(format!("Cleared {} pending entries.", count));

    Ok(())
}
