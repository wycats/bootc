//! Bootstrap command — first-login system setup.
//!
//! Replaces the shell script `scripts/bootc-bootstrap`. Runs on first login
//! via a systemd user unit. Sets up Flatpak remotes, installs Flatpak apps,
//! installs and enables GNOME extensions, applies gsettings, configures
//! distrobox, syncs host shims, and clones the repo.
//!
//! Reads manifests from `/usr/share/bootc-bootstrap/` (baked into the image),
//! not from the repo (which may not exist yet on first boot).

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::output::Output;

/// System manifest directory (baked into the image by the Containerfile)
const BOOTSTRAP_DIR: &str = "/usr/share/bootc-bootstrap";

/// State directory for bootstrap markers
fn state_dir() -> PathBuf {
    let base = std::env::var("XDG_STATE_HOME")
        .unwrap_or_else(|_| format!("{}/.local/state", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("bootc-bootstrap")
}

/// Compute SHA-256 hash of all files in a directory.
fn manifest_hash(dir: &Path) -> Result<String> {
    use std::collections::BTreeMap;

    let mut files = BTreeMap::new();
    if !dir.exists() {
        return Ok(String::new());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let content = fs::read(&path)?;
            files.insert(
                path.file_name().unwrap().to_string_lossy().to_string(),
                content,
            );
        }
    }

    if files.is_empty() {
        return Ok(String::new());
    }

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for (name, content) in &files {
        hasher.update(name.as_bytes());
        hasher.update(content);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Check if bootstrap has already been applied for the current manifest hash.
fn already_applied(hash: &str) -> bool {
    let state_file = state_dir().join("last-applied.sha256");
    if let Ok(stored) = fs::read_to_string(&state_file) {
        stored.trim() == hash
    } else {
        false
    }
}

/// Record that bootstrap has been applied for the given manifest hash.
fn record_applied(hash: &str) -> Result<()> {
    let dir = state_dir();
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("last-applied.sha256"), format!("{}\n", hash))?;
    Ok(())
}

// ── Distrobox setup (outside hash gate) ─────────────────────────────────────

fn apply_distrobox() -> Result<()> {
    let ini = Path::new("/etc/distrobox/distrobox.ini");
    if !ini.exists() {
        Output::info("No distrobox.ini found; skipping");
        return Ok(());
    }

    if !command_exists("distrobox") {
        Output::info("distrobox not found; skipping");
        return Ok(());
    }

    // Check if all containers already exist
    let content = fs::read_to_string(ini)?;
    let mut needs_create = false;
    for line in content.lines() {
        if line.starts_with('[') && line.ends_with(']') {
            let name = &line[1..line.len() - 1];
            let status = Command::new("podman")
                .args(["container", "exists", name])
                .status();
            if !status.map(|s| s.success()).unwrap_or(false) {
                Output::info(format!(
                    "Distrobox container '{}' missing; will create",
                    name
                ));
                needs_create = true;
            }
        }
    }

    if !needs_create {
        Output::info("All distrobox containers already exist; skipping");
        return Ok(());
    }

    let arch = std::env::consts::ARCH;
    Output::info(format!(
        "Assembling distrobox containers from {} (arch: {})",
        ini.display(),
        arch
    ));

    let status = Command::new("distrobox")
        .args(["assemble", "create", "--file", &ini.to_string_lossy()])
        .status()
        .context("Failed to run distrobox assemble")?;

    if !status.success() {
        anyhow::bail!(
            "distrobox assemble failed (image may not be available for {})",
            arch
        );
    }

    Ok(())
}

// ── Composefs font cache workaround (outside hash gate) ─────────────────────

fn apply_font_workaround() -> Result<()> {
    // Check if root is a composefs overlay
    let fstype = Command::new("stat")
        .args(["-f", "-c", "%T", "/"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if fstype != "overlay" && fstype != "overlayfs" {
        return Ok(());
    }

    let marker_file = state_dir().join("fontcache-deployment");

    // Get current ostree deployment checksum
    let current_deployment = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output()
        .ok()
        .and_then(|o| {
            let json: serde_json::Value = serde_json::from_slice(&o.stdout).ok()?;
            json["deployments"][0]["checksum"]
                .as_str()
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    if !current_deployment.is_empty()
        && let Ok(stored) = fs::read_to_string(&marker_file)
            && stored.trim() == current_deployment {
                Output::info(format!(
                    "Fontconfig cache valid for deployment {}...",
                    &current_deployment[..12.min(current_deployment.len())]
                ));
                return Ok(());
            }

    Output::info("Clearing stale fontconfig cache (deployment changed)");

    let home = std::env::var("HOME").unwrap_or_default();
    let cache_dir = format!("{}/.cache/fontconfig", home);
    let _ = fs::remove_dir_all(&cache_dir);
    let _ = fs::create_dir_all(&cache_dir);
    let _ = Command::new("fc-cache").arg("-f").status();

    // Remove legacy font mirror if present
    let legacy_mirror = format!("{}/.local/share/fonts/composefs-mirror", home);
    if Path::new(&legacy_mirror).exists() {
        Output::info("Removing legacy font mirror");
        let _ = fs::remove_dir_all(&legacy_mirror);
        let _ = Command::new("fc-cache").status();
    }

    if !current_deployment.is_empty() {
        let dir = state_dir();
        fs::create_dir_all(&dir)?;
        fs::write(&marker_file, format!("{}\n", current_deployment))?;
    }

    Output::info("Fontconfig cache rebuilt for current deployment");
    Ok(())
}

// ── Flatpak remotes ─────────────────────────────────────────────────────────

fn apply_flatpak_remotes(bootstrap_dir: &Path) -> Result<()> {
    let file = bootstrap_dir.join("flatpak-remotes.json");
    if !file.exists() {
        return Ok(());
    }

    if !command_exists("flatpak") {
        Output::info("flatpak not found; skipping remotes");
        return Ok(());
    }

    let content = fs::read_to_string(&file)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let remotes = manifest["remotes"]
        .as_array()
        .context("Expected 'remotes' array")?;

    for remote in remotes {
        let name = remote["name"].as_str().unwrap_or_default();
        let url = remote["url"].as_str().unwrap_or_default();
        let scope = remote["scope"].as_str().unwrap_or("system");

        if name.is_empty() || url.is_empty() {
            continue;
        }

        Output::info(format!(
            "Ensuring flatpak remote ({}): {} {}",
            scope, name, url
        ));

        let mut args = vec!["remote-add", "--if-not-exists"];

        // .flatpakrepo URLs include GPG keys; bare repo URLs don't
        if !url.ends_with(".flatpakrepo") {
            args.push("--no-gpg-verify");
        }

        if scope == "user" {
            args.push("--user");
        }

        args.push(name);
        args.push(url);

        let status = run_flatpak(scope, &args)?;
        if !status.success() {
            anyhow::bail!("Failed to add flatpak remote: {}", name);
        }
    }

    Ok(())
}

// ── Flatpak apps ────────────────────────────────────────────────────────────

fn apply_flatpak_apps(bootstrap_dir: &Path) -> Result<()> {
    let file = bootstrap_dir.join("flatpak-apps.json");
    if !file.exists() {
        return Ok(());
    }

    if !command_exists("flatpak") {
        Output::info("flatpak not found; skipping apps");
        return Ok(());
    }

    let content = fs::read_to_string(&file)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let apps = manifest["apps"]
        .as_array()
        .context("Expected 'apps' array")?;

    for app in apps {
        let appid = app["id"].as_str().unwrap_or_default();
        let remote = app["remote"].as_str().unwrap_or("flathub");
        let scope = app["scope"].as_str().unwrap_or("system");

        if appid.is_empty() {
            continue;
        }

        Output::info(format!(
            "Installing/updating flatpak ({}): {} ({})",
            scope, appid, remote
        ));

        let args = vec![
            "install",
            "-y",
            "--noninteractive",
            "--or-update",
            remote,
            appid,
        ];

        let status = run_flatpak(scope, &args);
        if let Err(e) = status {
            Output::error(format!("Failed to install {}: {}", appid, e));
            return Err(e);
        }
        let status = status.unwrap();
        if !status.success() {
            Output::error(format!("Failed to install flatpak: {}", appid));
            return Err(anyhow::anyhow!("flatpak install failed for {}", appid));
        }
    }

    Ok(())
}

// ── GNOME extensions ────────────────────────────────────────────────────────

fn shell_major_version() -> Option<u32> {
    let output = Command::new("gnome-shell").arg("--version").output().ok()?;
    let version_str = String::from_utf8_lossy(&output.stdout);
    // "GNOME Shell 49.2" -> 49
    version_str
        .split_whitespace()
        .find_map(|word| word.parse::<u32>().ok())
}

fn apply_gnome_extensions(bootstrap_dir: &Path) -> Result<()> {
    let file = bootstrap_dir.join("gnome-extensions.json");
    if !file.exists() {
        return Ok(());
    }

    if !command_exists("gnome-extensions") {
        Output::info("gnome-extensions not found; skipping");
        return Ok(());
    }

    let shell_ver = match shell_major_version() {
        Some(v) => v,
        None => {
            Output::info("Could not detect GNOME Shell version; skipping extensions");
            return Ok(());
        }
    };

    let content = fs::read_to_string(&file)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let extensions = manifest["extensions"]
        .as_array()
        .context("Expected 'extensions' array")?;

    let tmpdir_path = std::env::temp_dir().join(format!("bkt-bootstrap-{}", std::process::id()));
    fs::create_dir_all(&tmpdir_path).context("Failed to create temp directory")?;
    // Clean up on scope exit via a guard
    struct TmpGuard(PathBuf);
    impl Drop for TmpGuard {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _tmpguard = TmpGuard(tmpdir_path.clone());

    for ext in extensions {
        let (uuid, enabled) = if ext.is_string() {
            // String format: just the UUID (implies enabled=true)
            (ext.as_str().unwrap_or_default().to_string(), true)
        } else if ext.is_object() {
            // Object format: {"id": "...", "enabled": true/false}
            let uuid = ext["id"].as_str().unwrap_or_default().to_string();
            // Explicit check: if "enabled" key exists, use its value; otherwise default true
            let enabled = if ext.get("enabled").is_some() {
                ext["enabled"].as_bool().unwrap_or(true)
            } else {
                true
            };
            (uuid, enabled)
        } else {
            continue;
        };

        if uuid.is_empty() {
            continue;
        }

        // Handle disabled extensions FIRST (before the already-installed check)
        if !enabled {
            Output::info(format!("Skipping disabled extension: {}", uuid));
            // If it's currently enabled, disable it to match the manifest
            if extension_is_installed(&uuid) {
                let _ = Command::new("gnome-extensions")
                    .args(["disable", &uuid])
                    .status();
            }
            continue;
        }

        // Already installed — just enable
        if extension_is_installed(&uuid) {
            Output::info(format!("Enabling GNOME extension: {}", uuid));
            let _ = Command::new("gnome-extensions")
                .args(["enable", &uuid])
                .status();
            continue;
        }

        // Not installed — download and install
        Output::info(format!(
            "Installing GNOME extension: {} (shell {})",
            uuid, shell_ver
        ));

        let info_url = format!(
            "https://extensions.gnome.org/extension-info/?uuid={}&shell_version={}",
            uuid, shell_ver
        );

        // Fetch extension info
        let info_output = Command::new("curl")
            .args(["-fsSL", &info_url])
            .output()
            .context("Failed to fetch extension info")?;

        if !info_output.status.success() {
            Output::error(format!("Failed to fetch extension info for {}", uuid));
            return Err(anyhow::anyhow!(
                "Failed to fetch extension info for {}",
                uuid
            ));
        }

        let info: serde_json::Value = serde_json::from_slice(&info_output.stdout)
            .context("Failed to parse extension info JSON")?;

        let dl_path = info["download_url"].as_str().unwrap_or_default();
        if dl_path.is_empty() {
            Output::info(format!("No download_url for {}; skipping", uuid));
            continue;
        }

        let zip_path = tmpdir_path.join(format!("{}.zip", uuid));
        let dl_url = format!("https://extensions.gnome.org{}", dl_path);

        // Download the extension zip
        let dl_status = Command::new("curl")
            .args(["-fsSL", &dl_url, "-o", &zip_path.to_string_lossy()])
            .status()
            .context("Failed to download extension")?;

        if !dl_status.success() {
            Output::error(format!("Failed to download {}", uuid));
            return Err(anyhow::anyhow!("Failed to download extension {}", uuid));
        }

        // Install the extension
        let install_status = Command::new("gnome-extensions")
            .args(["install", "--force", &zip_path.to_string_lossy()])
            .status()
            .context("Failed to install extension")?;

        if !install_status.success() {
            Output::error(format!("Failed to install {}", uuid));
            return Err(anyhow::anyhow!("Failed to install extension {}", uuid));
        }

        // Enable it
        let _ = Command::new("gnome-extensions")
            .args(["enable", &uuid])
            .status();
    }

    Ok(())
}

fn extension_is_installed(uuid: &str) -> bool {
    Command::new("gnome-extensions")
        .args(["info", uuid])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── GSettings ───────────────────────────────────────────────────────────────

fn apply_gsettings(bootstrap_dir: &Path) -> Result<()> {
    let file = bootstrap_dir.join("gsettings.json");
    if !file.exists() {
        return Ok(());
    }

    if !command_exists("gsettings") {
        Output::info("gsettings not found; skipping");
        return Ok(());
    }

    let content = fs::read_to_string(&file)?;
    let manifest: serde_json::Value = serde_json::from_str(&content)?;

    let settings = manifest["settings"]
        .as_array()
        .context("Expected 'settings' array")?;

    for setting in settings {
        let schema = setting["schema"].as_str().unwrap_or_default();
        let key = setting["key"].as_str().unwrap_or_default();
        let value = setting["value"].as_str().unwrap_or_default();

        if schema.is_empty() || key.is_empty() {
            continue;
        }

        Output::info(format!("Applying gsettings: {} {} {}", schema, key, value));

        let _ = Command::new("gsettings")
            .args(["set", schema, key, value])
            .status();
    }

    Ok(())
}

// ── Host shims ──────────────────────────────────────────────────────────────

fn apply_host_shims() {
    if command_exists("bkt") {
        Output::info("Syncing host shims");
        let status = Command::new("bkt").args(["shim", "sync"]).status();
        if let Err(e) = status {
            Output::info(format!("bkt shim sync failed (non-fatal): {}", e));
        } else if !status.unwrap().success() {
            Output::info("bkt shim sync failed (non-fatal)");
        }
    } else {
        Output::info("bkt command not found; skipping host shims");
    }
}

// ── Repo cloning ────────────────────────────────────────────────────────────

fn clone_repo() -> Result<()> {
    let repo_json = Path::new("/usr/share/bootc/repo.json");
    let cache_dir =
        PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            format!("{}/.local/state", std::env::var("HOME").unwrap_or_default())
        }))
        .join("bkt");
    let cache_file = cache_dir.join("repo-path");

    // Skip if repo path is already cached and valid
    if let Ok(cached) = fs::read_to_string(&cache_file) {
        let cached = cached.trim();
        if Path::new(cached).join("manifests").exists() {
            Output::info(format!("Repo already cloned at {}", cached));
            return Ok(());
        }
    }

    if !repo_json.exists() {
        Output::info("No repo.json; skipping clone");
        return Ok(());
    }

    let content = fs::read_to_string(repo_json)?;
    let config: serde_json::Value = serde_json::from_str(&content)?;
    let owner = config["owner"].as_str().unwrap_or_default();
    let name = config["name"].as_str().unwrap_or_default();

    if owner.is_empty() || name.is_empty() {
        Output::info("Invalid repo.json; skipping clone");
        return Ok(());
    }

    let home = std::env::var("HOME").unwrap_or_default();
    let clone_target = format!("{}/Code/Config/{}", home, name);

    if Path::new(&clone_target).join("manifests").exists() {
        Output::info(format!("Repo exists at {}", clone_target));
    } else if command_exists("gh") {
        Output::info(format!("Cloning {}/{} to {}...", owner, name, clone_target));
        let parent = Path::new(&clone_target).parent().unwrap();
        fs::create_dir_all(parent)?;
        let status = Command::new("gh")
            .args([
                "repo",
                "clone",
                &format!("{}/{}", owner, name),
                &clone_target,
                "--",
                "--depth=1",
            ])
            .status()
            .context("Failed to run gh repo clone")?;
        if !status.success() {
            anyhow::bail!("gh repo clone failed");
        }
    } else {
        Output::info("gh CLI not available; skipping clone");
        return Ok(());
    }

    // Cache the path
    if Path::new(&clone_target).join("manifests").exists() {
        fs::create_dir_all(&cache_dir)?;
        fs::write(&cache_file, &clone_target)?;
        Output::info(format!("Repo path cached at {}", cache_file.display()));
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_flatpak(scope: &str, args: &[&str]) -> Result<std::process::ExitStatus> {
    if scope.starts_with("system") {
        // System-scope operations need elevated privileges
        if command_exists("pkexec") {
            Command::new("pkexec")
                .arg("flatpak")
                .args(args)
                .status()
                .context("Failed to run pkexec flatpak")
        } else {
            Command::new("flatpak")
                .args(args)
                .status()
                .context("Failed to run flatpak")
        }
    } else {
        let mut full_args = vec!["--user"];
        full_args.extend_from_slice(args);
        Command::new("flatpak")
            .args(&full_args)
            .status()
            .context("Failed to run flatpak --user")
    }
}

// ── Main entry point ────────────────────────────────────────────────────────

pub fn run() -> Result<()> {
    let bootstrap_dir = Path::new(BOOTSTRAP_DIR);

    if !bootstrap_dir.exists() {
        Output::info("No bootstrap directory found; nothing to do");
        return Ok(());
    }

    Output::info("Starting bootstrap...");

    // ── Steps outside the hash gate ─────────────────────────────────────

    // Distrobox: container existence is a runtime concern, not a manifest concern
    if let Err(e) = apply_distrobox() {
        Output::info(format!(
            "distrobox setup failed (non-fatal; will retry next login): {}",
            e
        ));
    }

    // Font cache: keyed on ostree deployment, not manifest hash
    if let Err(e) = apply_font_workaround() {
        Output::info(format!("font workaround failed (non-fatal): {}", e));
    }

    // ── Hash gate ───────────────────────────────────────────────────────

    let hash = manifest_hash(bootstrap_dir)?;

    if !hash.is_empty() && already_applied(&hash) {
        Output::info(format!(
            "Already applied (manifest hash {}); exiting",
            &hash[..12.min(hash.len())]
        ));
        return Ok(());
    }

    // ── Steps inside the hash gate ──────────────────────────────────────

    let mut failed = false;

    if let Err(e) = apply_flatpak_remotes(bootstrap_dir) {
        Output::error(format!("Flatpak remotes failed: {}", e));
        failed = true;
    }

    if let Err(e) = apply_flatpak_apps(bootstrap_dir) {
        Output::error(format!("Flatpak apps failed: {}", e));
        failed = true;
    }

    if let Err(e) = apply_gnome_extensions(bootstrap_dir) {
        Output::error(format!("GNOME extensions failed: {}", e));
        failed = true;
    }

    if let Err(e) = apply_gsettings(bootstrap_dir) {
        Output::error(format!("GSettings failed: {}", e));
        failed = true;
    }

    // Non-fatal steps
    apply_host_shims();

    if let Err(e) = clone_repo() {
        Output::info(format!(
            "repo clone failed (non-fatal; bkt will clone on first PR): {}",
            e
        ));
    }

    // ── Record hash ─────────────────────────────────────────────────────

    if failed {
        Output::info("One or more steps failed; will retry next login");
        // Don't record hash — retry next login
        return Ok(());
    }

    if !hash.is_empty() {
        record_applied(&hash)?;
        Output::success(format!(
            "Applied successfully (manifest hash {})",
            &hash[..12.min(hash.len())]
        ));
    }

    Ok(())
}
