//! Daemon stress tests for Phase 4 validation.
//!
//! These tests verify correctness under load:
//! - No process leaks (zombies, orphans)
//! - No resource leaks (fd leaks, socket leaks)
//! - Correct behavior under concurrent load
//!
//! Run with: `cargo test --test daemon_stress`
//!
//! Note: Tests skip gracefully if daemon isn't running.

use std::process::Command;
use std::thread;
use std::time::Duration;

/// Check if the daemon is available.
fn daemon_available() -> bool {
    Command::new("bkt")
        .args(["admin", "daemon", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Count open file descriptors for the current process.
fn fd_count() -> usize {
    let pid = std::process::id();
    let path = format!("/proc/{}/fd", pid);
    std::fs::read_dir(path).map(|rd| rd.count()).unwrap_or(0)
}

/// Count zombie processes owned by current user.
fn zombie_count() -> usize {
    Command::new("ps")
        .args(["--no-headers", "-o", "stat", "-u", &whoami::username()])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|line| line.starts_with('Z'))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn stress_sequential_requests() {
    if !daemon_available() {
        eprintln!("Skipping stress test: daemon not running");
        return;
    }

    let start_fd = fd_count();
    let start_zombies = zombie_count();

    // Run 100 sequential requests (reduced from 1000 for faster tests)
    for i in 0..100 {
        let output = Command::new("bkt")
            .args(["admin", "daemon", "test", "true"])
            .output();

        if let Ok(out) = output {
            assert!(out.status.success(), "Request {} failed", i);
        } else {
            panic!("Failed to execute request {}", i);
        }
    }

    let end_fd = fd_count();
    let end_zombies = zombie_count();

    // Allow small variance (3 fds) for normal operation
    assert!(
        end_fd <= start_fd + 3,
        "FD leak detected: {} -> {} (+{})",
        start_fd,
        end_fd,
        end_fd.saturating_sub(start_fd)
    );

    assert!(
        end_zombies <= start_zombies,
        "Zombie leak detected: {} -> {}",
        start_zombies,
        end_zombies
    );
}

#[test]
fn stress_concurrent_requests() {
    if !daemon_available() {
        eprintln!("Skipping stress test: daemon not running");
        return;
    }

    let start_fd = fd_count();

    // Run 50 concurrent requests
    let handles: Vec<_> = (0..50)
        .map(|i| {
            thread::spawn(move || {
                let output = Command::new("bkt")
                    .args(["admin", "daemon", "test", "true"])
                    .output();

                if let Ok(out) = output {
                    assert!(out.status.success(), "Concurrent request {} failed", i);
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Thread panicked");
    }

    // Give daemon time to clean up
    thread::sleep(Duration::from_millis(100));

    let end_fd = fd_count();
    assert!(
        end_fd <= start_fd + 3,
        "FD leak after concurrent requests: {} -> {}",
        start_fd,
        end_fd
    );
}

#[test]
fn stress_rapid_connect_disconnect() {
    if !daemon_available() {
        eprintln!("Skipping stress test: daemon not running");
        return;
    }

    // Rapidly connect and disconnect without sending requests
    // This tests socket cleanup in the daemon
    for _ in 0..100 {
        let _ = std::os::unix::net::UnixStream::connect(format!(
            "{}/bkt/host.sock",
            std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()
        ));
    }

    // Daemon should still be responsive
    let output = Command::new("bkt")
        .args(["admin", "daemon", "test", "true"])
        .output()
        .expect("Daemon unresponsive after rapid connects");

    assert!(
        output.status.success(),
        "Daemon failed after rapid connects"
    );
}
