//! Homebrew manifest types.
//!
//! Manages Linuxbrew/Homebrew packages on the host system.

use anyhow::{Context, Result};
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
    /// Project manifest path (relative to workspace root).
    pub const PROJECT_PATH: &'static str = "manifests/homebrew.json";

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

    /// Load from the repository's manifests directory.
    pub fn load_repo() -> Result<Self> {
        let repo = crate::repo::find_repo_path()?;
        Self::load(&repo.join(Self::PROJECT_PATH))
    }

    /// Save to the repository's manifests directory.
    pub fn save_repo(&self) -> Result<()> {
        let repo = crate::repo::find_repo_path()?;
        self.save(&repo.join(Self::PROJECT_PATH))
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

}
