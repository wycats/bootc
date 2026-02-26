//! Auto-generated wrapper by bkt wrap
//! Target: /usr/lib/opt/microsoft/msedge/microsoft-edge
//! Slice: app-msedge.slice

use std::os::unix::process::CommandExt;

fn already_in_slice(slice: &str) -> bool {
    std::fs::read_to_string("/proc/self/cgroup")
        .map(|s| s.contains(slice))
        .unwrap_or(false)
}

fn main() {
    // Re-entry guard: if already running inside our target slice, exec directly.
    // Without this, child processes that re-invoke the wrapper binary
    // would each create a new systemd-run scope, causing an infinite loop.
    if already_in_slice("app-msedge.slice") {
        let err = std::process::Command::new("/usr/lib/opt/microsoft/msedge/microsoft-edge")
            .args(std::env::args().skip(1))
            .exec();
        eprintln!("Failed to exec target: {}", err);
        std::process::exit(1);
    }

    // Validate target exists
    let target = "/usr/lib/opt/microsoft/msedge/microsoft-edge";
    if !std::path::Path::new(target).exists() {
        eprintln!("Error: {} not found", target);
        std::process::exit(127);
    }

    // Generate unique unit name
    let unit_name = format!(
        "msedge-wrapper-{}-{}",
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
            "--quiet",
            "--slice=app-msedge.slice",
            "--scope",
            &format!("--unit={}", unit_name),
            "--description=Microsoft Edge (managed)",
            "--property=OOMPolicy=kill",
            "--",
            target,
        ])
        .args(std::env::args().skip(1))
        .exec();

    eprintln!("Failed to exec systemd-run: {}", err);
    std::process::exit(1);
}
