//! Integration tests for the bkt CLI.
//!
//! These tests run the compiled binary and verify its output.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use assert_fs::prelude::*;
use predicates::prelude::*;
use std::os::unix::fs::PermissionsExt;

/// Get bkt command for testing.
///
/// Sets `BKT_FORCE_HOST=1` to bypass host delegation, ensuring tests run
/// deterministically regardless of whether they execute in a container/toolbox.
///
/// Clears `XDG_STATE_HOME` so that tests setting `HOME` get proper isolation
/// for the ephemeral manifest (which checks `XDG_STATE_HOME` before `HOME`).
///
/// Tests that modify state (ephemeral manifest, shims, etc.) should create their
/// own `TempDir` and set `HOME` via `.env("HOME", temp.path())` for isolation.
fn bkt() -> Command {
    let mut cmd = cargo_bin_cmd!("bkt");
    cmd.env("BKT_FORCE_HOST", "1");
    cmd.env_remove("XDG_STATE_HOME");
    cmd
}

// ============================================================================
// Basic CLI tests
// ============================================================================

#[test]
fn cli_no_args_shows_help() {
    bkt()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn cli_help_flag_shows_help() {
    bkt()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Bucket - manage your bootc manifests",
        ));
}

#[test]
fn cli_version_flag_shows_version() {
    bkt()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("bkt"));
}

// ============================================================================
// Capture command tests
// ============================================================================

#[test]
fn capture_help_shows_options() {
    bkt()
        .args(["capture", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--only"))
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--apply"));
}

#[test]
fn capture_dry_run_succeeds() {
    // Exclude app-image since GearLever may not be installed in CI
    bkt()
        .args(["capture", "--dry-run", "--exclude", "app-image"])
        .assert()
        .success();
}

// ============================================================================
// Shim command tests
// ============================================================================

#[test]
fn shim_help_shows_subcommands() {
    bkt()
        .args(["shim", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("sync"));
}

#[test]
fn shim_list_succeeds() {
    bkt().args(["shim", "list"]).assert().success();
}

#[test]
fn shim_add_requires_name() {
    bkt()
        .args(["shim", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NAME"));
}

#[test]
fn shim_remove_requires_name() {
    bkt()
        .args(["shim", "remove"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("NAME"));
}

// ============================================================================
// Flatpak command tests
// ============================================================================

#[test]
fn flatpak_help_shows_subcommands() {
    bkt()
        .args(["flatpak", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("capture"));
}

#[test]
fn flatpak_list_succeeds() {
    bkt().args(["flatpak", "list"]).assert().success();
}

#[test]
fn flatpak_capture_dry_run_succeeds() {
    bkt()
        .args(["flatpak", "capture", "--dry-run"])
        .assert()
        .success();
}

#[test]
fn flatpak_add_requires_app_id() {
    bkt()
        .args(["flatpak", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("APP_ID"));
}

#[test]
fn flatpak_remove_requires_app_id() {
    bkt()
        .args(["flatpak", "remove"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("APP_ID"));
}

// ============================================================================
// Extension command tests
// ============================================================================

#[test]
fn extension_help_shows_subcommands() {
    bkt()
        .args(["extension", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("disable"))
        .stdout(predicate::str::contains("enable"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("capture"));
}

#[test]
fn extension_list_succeeds() {
    bkt().args(["extension", "list"]).assert().success();
}

#[test]
fn extension_add_requires_uuid() {
    bkt()
        .args(["extension", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("UUID"));
}

#[test]
fn extension_remove_requires_uuid() {
    bkt()
        .args(["extension", "remove"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("UUID"));
}

#[test]
fn extension_capture_dry_run_succeeds() {
    bkt()
        .args(["extension", "capture", "--dry-run"])
        .assert()
        .success();
}

// ============================================================================
// GSettings command tests
// ============================================================================

#[test]
fn gsetting_help_shows_subcommands() {
    bkt()
        .args(["gsetting", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("unset"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("apply"))
        .stdout(predicate::str::contains("capture"));
}

#[test]
fn gsetting_list_succeeds() {
    bkt().args(["gsetting", "list"]).assert().success();
}

#[test]
fn gsetting_capture_requires_schema() {
    bkt()
        .args(["gsetting", "capture"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SCHEMA"));
}

#[test]
fn gsetting_capture_validates_schema() {
    // Test that capture fails gracefully with a non-existent schema
    // (may fail with "not found" or "command failed" depending on environment)
    bkt()
        .args(["gsetting", "capture", "org.nonexistent.schema", "--dry-run"])
        .assert()
        .failure()
        // Either "not found" (schema missing) or "command failed" (no schemas at all)
        .stderr(
            predicate::str::contains("not found").or(predicate::str::contains("command failed")),
        );
}

#[test]
fn gsetting_set_requires_all_args() {
    bkt()
        .args(["gsetting", "set"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SCHEMA"));
}

#[test]
fn gsetting_unset_requires_args() {
    bkt()
        .args(["gsetting", "unset"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("SCHEMA"));
}

// ============================================================================
// Profile command tests
// ============================================================================

#[test]
fn profile_help_shows_subcommands() {
    bkt()
        .args(["profile", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("capture"))
        .stdout(predicate::str::contains("diff"))
        .stdout(predicate::str::contains("unowned"));
}

#[test]
fn profile_capture_help() {
    bkt()
        .args(["profile", "capture", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Capture"));
}

// ============================================================================
// Skel command tests
// ============================================================================

#[test]
fn skel_help_shows_subcommands() {
    bkt()
        .args(["skel", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("diff"))
        .stdout(predicate::str::contains("sync"));
}

#[test]
fn skel_list_succeeds() {
    bkt().args(["skel", "list"]).assert().success();
}

#[test]
fn skel_add_requires_file() {
    bkt()
        .args(["skel", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("FILE"));
}

// ============================================================================
// Repo command tests
// ============================================================================

#[test]
fn repo_help_shows_subcommands() {
    bkt()
        .args(["repo", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("path"));
}

#[test]
fn repo_path_succeeds() {
    // Should succeed even without a repo
    bkt().args(["repo", "path"]).assert().success();
}

#[test]
fn repo_info_succeeds() {
    // Should succeed even without a repo (outputs message about config not found)
    bkt().args(["repo", "info"]).assert().success();
}

// ============================================================================
// File-based integration tests using tempdir
// ============================================================================

#[test]
fn shim_add_and_list_integration() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Set HOME to temp dir so user config goes there
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["shim", "add", "test-shim", "--local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added shim: test-shim"));

    bkt()
        .env("HOME", home)
        .args(["shim", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-shim"));

    temp.close().unwrap();
}

#[test]
fn shim_add_and_remove_integration() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["shim", "add", "to-remove", "--local"])
        .assert()
        .success();

    bkt()
        .env("HOME", home)
        .args(["shim", "remove", "to-remove", "--local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed shim: to-remove"));

    temp.close().unwrap();
}

#[test]
fn shim_add_with_host_option() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["shim", "add", "docker", "--host", "podman", "--local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added shim: docker"));

    temp.close().unwrap();
}

#[test]
fn extension_add_and_list_integration() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["extension", "add", "test@example.com", "--local"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Added to user manifest: test@example.com",
        ));

    bkt()
        .env("HOME", home)
        .args(["extension", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test@example.com"));

    temp.close().unwrap();
}

#[test]
fn extension_remove_nonexistent_shows_message() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["extension", "remove", "nonexistent@example.com", "--local"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not found"));

    temp.close().unwrap();
}

#[test]
fn gsetting_set_adds_to_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    // Setting will be added to manifest even if apply fails
    // Use --force to skip validation in test environment
    bkt()
        .env("HOME", home)
        .args([
            "gsetting",
            "set",
            "org.test.schema",
            "key",
            "value",
            "--force",
            "--local",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added to user manifest"));

    temp.close().unwrap();
}

#[test]
fn gsetting_list_shows_empty_when_no_settings() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["gsetting", "list"])
        .assert()
        .success();

    temp.close().unwrap();
}

// ============================================================================
// Dev (toolbox) command tests
// ============================================================================

#[test]
fn dev_help_shows_subcommands() {
    bkt()
        .args(["dev", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("enter"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("diff"))
        .stdout(predicate::str::contains("copr"));
}

#[test]
fn dev_help_shows_install_subcommand() {
    bkt()
        .args(["dev", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("list"));
}

#[test]
fn dev_install_updates_toolbox_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    // Stub `dnf` so the command can run in CI without installing packages.
    let bin_dir = temp.child("bin");
    bin_dir.create_dir_all().unwrap();
    let dnf = bin_dir.child("dnf");
    std::fs::write(dnf.path(), "#!/usr/bin/env sh\nexit 0\n").unwrap();
    let mut perms = std::fs::metadata(dnf.path()).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(dnf.path(), perms).unwrap();

    let path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.path().display(), path);

    bkt()
        .env("HOME", home)
        .env("PATH", new_path)
        .args(["--no-delegate", "dev", "install", "--force", "gcc"])
        .assert()
        .success();

    let toolbox_manifest = temp
        .path()
        .join(".config")
        .join("bootc")
        .join("toolbox-packages.json");
    let system_manifest = temp
        .path()
        .join(".config")
        .join("bootc")
        .join("system-packages.json");

    let content = std::fs::read_to_string(&toolbox_manifest).unwrap();
    assert!(content.contains("\"gcc\""));
    assert!(!system_manifest.exists());

    temp.close().unwrap();
}

#[test]
fn dev_status_shows_empty_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["--no-delegate", "dev", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Development Toolbox Status"))
        .stdout(predicate::str::contains("No packages in toolbox manifest"));

    temp.close().unwrap();
}

#[test]
fn dev_diff_shows_empty_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["--no-delegate", "dev", "diff"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Toolbox manifest is empty"));

    temp.close().unwrap();
}

#[test]
fn dev_sync_shows_empty_manifest() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["--no-delegate", "dev", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "All manifest packages are already installed",
        ));

    temp.close().unwrap();
}

// ============================================================================
// Changelog command tests
// ============================================================================

#[test]
fn changelog_help_shows_subcommands() {
    bkt()
        .args(["changelog", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("pending"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("release"));
}

#[test]
fn changelog_pending_outside_repo_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .current_dir(temp.path())
        .args(["changelog", "pending"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not in a git repository"));

    temp.close().unwrap();
}

#[test]
fn changelog_validate_outside_repo_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .current_dir(temp.path())
        .args(["changelog", "validate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not in a git repository"));

    temp.close().unwrap();
}

#[test]
fn changelog_list_outside_repo_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .current_dir(temp.path())
        .args(["changelog", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not in a git repository"));

    temp.close().unwrap();
}

#[test]
fn changelog_add_requires_args() {
    bkt()
        .args(["changelog", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--type"))
        .stderr(predicate::str::contains("--category"))
        .stderr(predicate::str::contains("<MESSAGE>"));
}

#[test]
fn changelog_generate_requires_args() {
    bkt()
        .args(["changelog", "generate"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--type"))
        .stderr(predicate::str::contains("--category"))
        .stderr(predicate::str::contains("<MESSAGE>"));
}

// ============================================================================
// Drift command tests
// ============================================================================

#[test]
fn drift_help_shows_subcommands() {
    bkt()
        .args(["drift", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("check"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("explain"));
}

#[test]
fn drift_explain_succeeds() {
    bkt()
        .args(["drift", "explain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Drift Detection"))
        .stdout(predicate::str::contains("Types of Drift"));
}

#[test]
fn drift_check_shows_not_implemented() {
    // drift check currently shows a stub message since the Rust implementation
    // is not yet complete (see RFC 0007). It succeeds with a warning.
    bkt()
        .args(["drift", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Drift detection is not yet implemented",
        ));
}

// ============================================================================
// Base command tests
// ============================================================================

#[test]
fn base_help_shows_subcommands() {
    bkt()
        .args(["base", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("verify"))
        .stdout(predicate::str::contains("assume"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("info"));
}

#[test]
fn base_list_outside_repo_fails() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .current_dir(temp.path())
        .args(["base", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Not in a git repository"));

    temp.close().unwrap();
}

#[test]
fn base_assume_requires_package() {
    bkt()
        .args(["base", "assume"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("<PACKAGE>"));
}

// ============================================================================
// Validation tests
// ============================================================================

#[test]
fn gsetting_set_rejects_invalid_schema() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args(["gsetting", "set", "nonexistent.schema.xyz", "key", "value"])
        .assert()
        .failure()
        // Either "not found" (schema missing) or "command failed" (no schemas at all)
        .stderr(
            predicate::str::contains("not found").or(predicate::str::contains("command failed")),
        );

    temp.close().unwrap();
}

#[test]
fn gsetting_set_force_bypasses_validation() {
    let temp = assert_fs::TempDir::new().unwrap();
    let home = temp.path().to_str().unwrap();

    bkt()
        .env("HOME", home)
        .args([
            "gsetting",
            "set",
            "nonexistent.schema.xyz",
            "key",
            "value",
            "--force",
            "--local",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added to user manifest"));

    temp.close().unwrap();
}

#[test]
fn flatpak_add_force_bypasses_validation() {
    // Note: We just test the help to verify --force flag exists
    // Actual flatpak commands require host context (not in toolbox)
    bkt()
        .args(["flatpak", "add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--force"));
}

// ============================================================================
// Local command tests
// ============================================================================

#[test]
fn local_help_shows_subcommands() {
    bkt()
        .args(["local", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("commit"))
        .stdout(predicate::str::contains("clear"))
        .stdout(predicate::str::contains("path"));
}

#[test]
fn local_list_empty_shows_message() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["local", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No local changes tracked"));
}

#[test]
fn local_clear_empty_shows_message() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["local", "clear", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No local changes to clear"));
}

#[test]
fn local_commit_empty_shows_message() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["local", "commit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No local changes to commit"));
}

#[test]
fn local_path_shows_path() {
    let temp = assert_fs::TempDir::new().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["local", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains(".local/state/bkt/ephemeral.json"));
}

#[test]
fn local_list_with_changes() {
    let temp = assert_fs::TempDir::new().unwrap();

    // First make a local change using shim add
    let shims_dir = temp.child(".local/toolbox/shims");
    shims_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["shim", "add", "my-shim", "--local"])
        .assert()
        .success();

    // Then verify it shows in local list
    bkt()
        .env("HOME", temp.path())
        .args(["local", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("my-shim"))
        .stdout(predicate::str::contains("shim"));
}

#[test]
fn local_list_json_format() {
    let temp = assert_fs::TempDir::new().unwrap();

    // First make a local change
    let shims_dir = temp.child(".local/toolbox/shims");
    shims_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["shim", "add", "json-test", "--local"])
        .assert()
        .success();

    // Then verify JSON format works
    bkt()
        .env("HOME", temp.path())
        .args(["local", "list", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"identifier\": \"json-test\""))
        .stdout(predicate::str::contains("\"domain\": \"shim\""));
}

#[test]
fn local_clear_removes_changes() {
    let temp = assert_fs::TempDir::new().unwrap();

    // First make a local change
    let shims_dir = temp.child(".local/toolbox/shims");
    shims_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["shim", "add", "to-clear", "--local"])
        .assert()
        .success();

    // Verify it's tracked
    bkt()
        .env("HOME", temp.path())
        .args(["local", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("to-clear"));

    // Clear it
    bkt()
        .env("HOME", temp.path())
        .args(["local", "clear", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared"));

    // Verify it's gone
    bkt()
        .env("HOME", temp.path())
        .args(["local", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No local changes tracked"));
}

#[test]
fn local_commit_dry_run_shows_changes() {
    let temp = assert_fs::TempDir::new().unwrap();

    // First make a local change
    let shims_dir = temp.child(".local/toolbox/shims");
    shims_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["shim", "add", "commit-test", "--local"])
        .assert()
        .success();

    // Dry-run commit should show what would happen
    bkt()
        .env("HOME", temp.path())
        .args(["local", "commit", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would commit"))
        .stdout(predicate::str::contains("commit-test"));
}

#[test]
fn local_commit_with_message_dry_run() {
    let temp = assert_fs::TempDir::new().unwrap();

    // First make a local change
    let shims_dir = temp.child(".local/toolbox/shims");
    shims_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["shim", "add", "msg-test", "--local"])
        .assert()
        .success();

    // Dry-run commit with custom message
    bkt()
        .env("HOME", temp.path())
        .args(["local", "commit", "--dry-run", "-m", "My custom message"])
        .assert()
        .success()
        .stdout(predicate::str::contains("My custom message"));
}

#[test]
fn local_list_domain_filter() {
    let temp = assert_fs::TempDir::new().unwrap();

    // Make changes in different domains
    let shims_dir = temp.child(".local/toolbox/shims");
    shims_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["shim", "add", "shim-change", "--local"])
        .assert()
        .success();

    // Create extension config directory
    let config_dir = temp.child(".config/bootc");
    config_dir.create_dir_all().unwrap();

    bkt()
        .env("HOME", temp.path())
        .args(["extension", "add", "filter-ext@test.com", "--local"])
        .assert()
        .success();

    // Filter by shim domain should only show shim
    bkt()
        .env("HOME", temp.path())
        .args(["local", "list", "--domain", "shim"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shim-change"))
        .stdout(predicate::str::contains("filter-ext").not());

    // Filter by extension domain should only show extension
    bkt()
        .env("HOME", temp.path())
        .args(["local", "list", "--domain", "extension"])
        .assert()
        .success()
        .stdout(predicate::str::contains("filter-ext"))
        .stdout(predicate::str::contains("shim-change").not());
}
