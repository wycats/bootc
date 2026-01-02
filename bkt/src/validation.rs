//! Input validation for add/install commands.
//!
//! This module provides validation helpers that check whether items exist
//! before adding them to manifests. This prevents typos and invalid entries.

use anyhow::{Context, Result, bail};
use std::process::Command;

use crate::output::Output;

/// Validate that a GSettings schema exists.
pub fn validate_gsettings_schema(schema: &str) -> Result<()> {
    let output = Command::new("gsettings")
        .arg("list-schemas")
        .output()
        .context("Failed to list GSettings schemas")?;

    if !output.status.success() {
        // Can't validate, continue anyway
        Output::warning("Could not validate schema (gsettings unavailable)");
        return Ok(());
    }

    let schemas = String::from_utf8_lossy(&output.stdout);
    if schemas.lines().any(|s| s == schema) {
        return Ok(());
    }

    // Schema not found - find similar ones
    let similar: Vec<&str> = schemas
        .lines()
        .filter(|s| {
            // Check if any part of the schema matches
            let parts: Vec<&str> = schema.split('.').collect();
            parts.iter().any(|part| part.len() > 3 && s.contains(part))
        })
        .take(5)
        .collect();

    if similar.is_empty() {
        bail!(
            "GSettings schema '{}' not found.\n\n\
             To list available schemas:\n  \
             gsettings list-schemas | grep <term>",
            schema
        );
    } else {
        bail!(
            "GSettings schema '{}' not found.\n\n\
             Similar schemas:\n  {}\n\n\
             To list all schemas:\n  \
             gsettings list-schemas | grep <term>",
            schema,
            similar.join("\n  ")
        );
    }
}

/// Validate that a key exists in a GSettings schema.
pub fn validate_gsettings_key(schema: &str, key: &str) -> Result<()> {
    let output = Command::new("gsettings")
        .args(["list-keys", schema])
        .output()
        .context("Failed to list GSettings keys")?;

    if !output.status.success() {
        // Schema doesn't exist or gsettings unavailable
        // Schema validation should have already caught this
        return Ok(());
    }

    let keys = String::from_utf8_lossy(&output.stdout);
    if keys.lines().any(|k| k == key) {
        return Ok(());
    }

    // Key not found - show available keys
    let available: Vec<&str> = keys.lines().take(10).collect();
    let more = if keys.lines().count() > 10 {
        format!("\n  ... and {} more", keys.lines().count() - 10)
    } else {
        String::new()
    };

    bail!(
        "Key '{}' not found in schema '{}'.\n\n\
         Available keys:\n  {}{}\n\n\
         To list all keys:\n  \
         gsettings list-keys {}",
        key,
        schema,
        available.join("\n  "),
        more,
        schema
    );
}

/// Validate that a DNF package exists in repositories.
pub fn validate_dnf_package(package: &str) -> Result<()> {
    // Try dnf5 first, fall back to dnf
    let dnf_cmd = if Command::new("dnf5").arg("--version").output().is_ok() {
        "dnf5"
    } else {
        "dnf"
    };

    let output = Command::new(dnf_cmd)
        .args(["info", package])
        .output()
        .context("Failed to query package info")?;

    if output.status.success() {
        return Ok(());
    }

    // Package not found - try to search for similar
    let search_output = Command::new(dnf_cmd)
        .args(["search", package])
        .output()
        .ok();

    let suggestions: Vec<String> = search_output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.starts_with('=') && !l.is_empty() && l.contains(" : "))
                .take(5)
                .map(|l| l.split(" : ").next().unwrap_or(l).trim().to_string())
                .collect()
        })
        .unwrap_or_default();

    if suggestions.is_empty() {
        bail!(
            "Package '{}' not found in repositories.\n\n\
             To search for packages:\n  \
             {} search <term>",
            package,
            dnf_cmd
        );
    } else {
        bail!(
            "Package '{}' not found in repositories.\n\n\
             Similar packages:\n  {}\n\n\
             To search for packages:\n  \
             {} search <term>",
            package,
            suggestions.join("\n  "),
            dnf_cmd
        );
    }
}

/// Validate that a Flatpak app exists on a remote.
pub fn validate_flatpak_app(app_id: &str, remote: &str) -> Result<()> {
    // Check if the remote exists first
    let remotes_output = Command::new("flatpak")
        .args(["remotes", "--columns=name"])
        .output()
        .context("Failed to list Flatpak remotes")?;

    if !remotes_output.status.success() {
        Output::warning("Could not validate app (flatpak unavailable)");
        return Ok(());
    }

    let remotes = String::from_utf8_lossy(&remotes_output.stdout);
    if !remotes.lines().any(|r| r == remote) {
        bail!(
            "Flatpak remote '{}' not found.\n\n\
             Available remotes:\n  {}\n\n\
             To add flathub:\n  \
             flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo",
            remote,
            remotes.lines().collect::<Vec<_>>().join("\n  ")
        );
    }

    // Check if the app exists on the remote
    let output = Command::new("flatpak")
        .args(["remote-info", remote, app_id])
        .output()
        .context("Failed to query Flatpak remote")?;

    if output.status.success() {
        return Ok(());
    }

    // App not found - search for similar
    let search_output = Command::new("flatpak")
        .args(["search", app_id])
        .output()
        .ok();

    let suggestions: Vec<String> = search_output
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .skip(1) // Skip header
                .take(5)
                .filter_map(|l| l.split('\t').nth(1)) // Get Application ID column
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if suggestions.is_empty() {
        bail!(
            "Flatpak app '{}' not found on remote '{}'.\n\n\
             To search for apps:\n  \
             flatpak search <term>\n\n\
             Browse Flathub:\n  \
             https://flathub.org",
            app_id,
            remote
        );
    } else {
        bail!(
            "Flatpak app '{}' not found on remote '{}'.\n\n\
             Similar apps:\n  {}\n\n\
             To search for apps:\n  \
             flatpak search <term>",
            app_id,
            remote,
            suggestions.join("\n  ")
        );
    }
}

/// Validate that a GNOME Shell extension UUID exists.
///
/// This checks against extensions.gnome.org. If network is unavailable,
/// it will warn but not fail.
pub fn validate_gnome_extension(uuid: &str) -> Result<()> {
    // Use curl to query the API (avoiding extra dependencies)
    let url = format!(
        "https://extensions.gnome.org/extension-info/?uuid={}",
        urlencoding::encode(uuid)
    );

    let output = Command::new("curl")
        .args(["-s", "-f", "-o", "/dev/null", "-w", "%{http_code}", &url])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let code = String::from_utf8_lossy(&o.stdout);
            if code.trim() == "200" {
                return Ok(());
            }
            // 404 or other error - extension not found
        }
        Ok(_) | Err(_) => {
            // Network error - warn but continue
            Output::warning("Could not validate extension (network unavailable)");
            return Ok(());
        }
    }

    // Extension not found
    bail!(
        "GNOME extension '{}' not found on extensions.gnome.org.\n\n\
         To browse extensions:\n  \
         https://extensions.gnome.org\n\n\
         Verify the exact UUID from the extension's page URL.",
        uuid
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_validation_format() {
        // This test documents the error message format
        // Actual validation requires gsettings to be available
        let err = validate_gsettings_schema("nonexistent.schema.that.does.not.exist");
        // Error should contain helpful suggestions
        if let Err(e) = err {
            let msg = e.to_string();
            assert!(
                msg.contains("not found") || msg.contains("unavailable"),
                "Error message should be helpful: {}",
                msg
            );
        }
    }

    #[test]
    fn test_dnf_validation_format() {
        // This test documents the error message format
        // Note: May fail with "Failed to query" if dnf5/dnf is not installed
        let err = validate_dnf_package("nonexistent-package-xyz-12345");
        if let Err(e) = err {
            let msg = e.to_string();
            assert!(
                msg.contains("not found") || msg.contains("Failed to query"),
                "Error message should be helpful: {}",
                msg
            );
        }
    }
}
