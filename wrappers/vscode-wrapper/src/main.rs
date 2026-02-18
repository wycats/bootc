//! Auto-generated wrapper by bkt wrap
//! Target: /usr/share/code/bin/code
//! Slice: app-vscode.slice

use std::os::unix::process::CommandExt;

fn already_in_slice(slice: &str) -> bool {
    std::fs::read_to_string("/proc/self/cgroup")
        .map(|s| s.contains(slice))
        .unwrap_or(false)
}

fn main() {
    // Re-entry guard: if already running inside our target slice, exec directly.
    // Without this, VS Code's child processes (which re-invoke /usr/bin/code)
    // would each create a new systemd-run scope, causing an infinite loop.
    if already_in_slice("app-vscode.slice") {
        let err = std::process::Command::new("/usr/share/code/bin/code")
            .args(std::env::args().skip(1))
            .exec();
        eprintln!("Failed to exec target: {}", err);
        std::process::exit(1);
    }

    // VS Code remote-cli passthrough
    if std::env::var("VSCODE_IPC_HOOK_CLI").is_ok() {
        if let Some(remote_cli) = find_remote_cli() {
            let err = std::process::Command::new(&remote_cli)
                .args(std::env::args().skip(1))
                .exec();
            eprintln!("Failed to exec remote-cli: {}", err);
            std::process::exit(1);
        }
    }

    // Validate target exists
    let target = "/usr/share/code/bin/code";
    if !std::path::Path::new(target).exists() {
        eprintln!("Error: {} not found", target);
        std::process::exit(127);
    }

    // Generate unique unit name
    let unit_name = format!(
        "vscode-wrapper-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    );

    // Launch via systemd-run
    let err = std::process::Command::new("systemd-run")
        .args([
            "--user",
            "--slice=app-vscode.slice",
            "--scope",
            &format!("--unit={}", unit_name),
            "--description=VS Code (managed)",
            "--property=OOMPolicy=kill",
            "--",
            target,
        ])
        .args(std::env::args().skip(1))
        .exec();

    eprintln!("Failed to exec systemd-run: {}", err);
    std::process::exit(1);
}

fn find_remote_cli() -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = format!("{}/code", dir);
        if candidate.contains("/remote-cli/") && std::path::Path::new(&candidate).exists() {
            return Some(candidate);
        }
    }
    None
}
