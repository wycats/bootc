//! Homebrew manifest types.
//!
//! Manages Linuxbrew/Homebrew packages on the host system.

use anyhow::{Context, Result};
use directories::BaseDirs;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A Homebrew formula entry.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum BrewFormula {
    /// Simple format: just the formula name
    Name(String),
    /// Full format: formula with tap info
    Full(BrewFormulaConfig),
}

/// Detailed configuration for a formula.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
pub struct BrewFormulaConfig {
    /// Formula name (e.g., "lefthook" or "valkyrie00/bbrew/bbrew")
    pub name: String,
    /// Optional tap to install from (e.g., "valkyrie00/bbrew")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tap: Option<String>,
}

impl BrewFormula {
    /// Get the formula name.
    pub fn name(&self) -> &str {
        match self {
            BrewFormula::Name(name) => name,
            BrewFormula::Full(config) => &config.name,
        }
    }

    /// Get the tap if specified.
    pub fn tap(&self) -> Option<&str> {
        match self {
            BrewFormula::Name(name) => {
                // Check if name contains a tap (e.g., "user/repo/formula")
                let parts: Vec<&str> = name.split('/').collect();
                if parts.len() == 3 {
                    Some(&name[..name.rfind('/').unwrap()])
                } else {
                    None
                }
            }
            BrewFormula::Full(config) => config.tap.as_deref(),
        }
    }

    /// Get just the formula name without tap prefix.
    pub fn formula_name(&self) -> &str {
        match self {
            BrewFormula::Name(name) => {
                // If it's "user/repo/formula", return just "formula"
                name.rsplit('/').next().unwrap_or(name)
            }
            BrewFormula::Full(config) => config.name.rsplit('/').next().unwrap_or(&config.name),
        }
    }
}

impl From<String> for BrewFormula {
    fn from(s: String) -> Self {
        BrewFormula::Name(s)
    }
}

impl From<&str> for BrewFormula {
    fn from(s: &str) -> Self {
        BrewFormula::Name(s.to_string())
    }
}

/// The homebrew.json manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct HomebrewManifest {
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// List of formulae to install
    #[serde(default)]
    pub formulae: Vec<BrewFormula>,
    /// List of taps to add
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taps: Vec<String>,
}

impl HomebrewManifest {
    /// System manifest path (baked into image).
    pub const SYSTEM_PATH: &'static str = "/usr/share/bootc-bootstrap/homebrew.json";

    /// Load a manifest from a path.
    pub fn load(path: &PathBuf) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read homebrew manifest from {}", path.display()))?;
        let manifest: Self = serde_json::from_str(&content).with_context(|| {
            format!("Failed to parse homebrew manifest from {}", path.display())
        })?;
        Ok(manifest)
    }

    /// Save a manifest to a path.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize homebrew manifest")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write homebrew manifest to {}", path.display()))?;
        Ok(())
    }

    /// Get the user manifest path.
    pub fn user_path() -> PathBuf {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".config"))
            .or_else(|| BaseDirs::new().map(|d| d.config_dir().to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("bootc")
            .join("homebrew.json")
    }

    /// Load the system manifest.
    pub fn load_system() -> Result<Self> {
        Self::load(&PathBuf::from(Self::SYSTEM_PATH))
    }

    /// Load the user manifest.
    pub fn load_user() -> Result<Self> {
        Self::load(&Self::user_path())
    }

    /// Save the user manifest.
    pub fn save_user(&self) -> Result<()> {
        self.save(&Self::user_path())
    }

    /// Merge system and user manifests (user overrides system).
    pub fn merged(system: &Self, user: &Self) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut formulae = Vec::new();

        // User formulae take precedence
        for f in &user.formulae {
            seen.insert(f.name().to_string());
            formulae.push(f.clone());
        }

        // Add system formulae not in user
        for f in &system.formulae {
            if !seen.contains(f.name()) {
                formulae.push(f.clone());
            }
        }

        // Sort for consistent output
        formulae.sort();

        // Merge taps
        let mut taps: Vec<String> = system
            .taps
            .iter()
            .chain(user.taps.iter())
            .cloned()
            .collect();
        taps.sort();
        taps.dedup();

        Self {
            schema: user.schema.clone().or_else(|| system.schema.clone()),
            formulae,
            taps,
        }
    }

    /// Check if a formula exists.
    pub fn contains(&self, name: &str) -> bool {
        self.formulae.iter().any(|f| f.name() == name)
    }

    /// Add a formula if not present.
    pub fn add(&mut self, formula: impl Into<BrewFormula>) -> bool {
        let formula = formula.into();
        if self.contains(formula.name()) {
            return false;
        }
        self.formulae.push(formula);
        self.formulae.sort();
        true
    }

    /// Remove a formula.
    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.formulae.len();
        self.formulae.retain(|f| f.name() != name);
        self.formulae.len() < len_before
    }

    /// List formula names.
    #[allow(dead_code)]
    pub fn list(&self) -> Vec<String> {
        self.formulae.iter().map(|f| f.name().to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_default_is_empty() {
        let manifest = HomebrewManifest::default();
        assert!(manifest.formulae.is_empty());
        assert!(manifest.taps.is_empty());
    }

    #[test]
    fn manifest_add_formula() {
        let mut manifest = HomebrewManifest::default();
        assert!(manifest.add("lefthook"));
        assert!(manifest.contains("lefthook"));
        assert!(!manifest.add("lefthook")); // duplicate
    }

    #[test]
    fn manifest_remove_formula() {
        let mut manifest = HomebrewManifest::default();
        manifest.add("lefthook");
        assert!(manifest.remove("lefthook"));
        assert!(!manifest.contains("lefthook"));
    }

    #[test]
    fn formula_with_tap_parses_correctly() {
        let formula: BrewFormula = "valkyrie00/bbrew/bbrew".into();
        assert_eq!(formula.name(), "valkyrie00/bbrew/bbrew");
        assert_eq!(formula.tap(), Some("valkyrie00/bbrew"));
        assert_eq!(formula.formula_name(), "bbrew");
    }

    #[test]
    fn simple_formula_has_no_tap() {
        let formula: BrewFormula = "lefthook".into();
        assert_eq!(formula.name(), "lefthook");
        assert_eq!(formula.tap(), None);
        assert_eq!(formula.formula_name(), "lefthook");
    }

    #[test]
    fn manifest_merge_combines_formulae() {
        let mut system = HomebrewManifest::default();
        system.add("system-pkg");

        let mut user = HomebrewManifest::default();
        user.add("user-pkg");

        let merged = HomebrewManifest::merged(&system, &user);
        assert_eq!(merged.formulae.len(), 2);
        assert!(merged.contains("system-pkg"));
        assert!(merged.contains("user-pkg"));
    }

    #[test]
    fn manifest_merge_deduplicates() {
        let mut system = HomebrewManifest::default();
        system.add("shared-pkg");

        let mut user = HomebrewManifest::default();
        user.add("shared-pkg");

        let merged = HomebrewManifest::merged(&system, &user);
        assert_eq!(merged.formulae.len(), 1);
    }
}
