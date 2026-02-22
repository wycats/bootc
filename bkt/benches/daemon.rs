//! Daemon benchmarks for Phase 4 validation.
//!
//! These benchmarks measure:
//! - **End-to-end latency**: `daemon_echo` vs `flatpak_spawn_echo`
//! - **Component breakdown**: `daemon_components` with custom timing
//! - **Sequential throughput**: `daemon_sequential_10`
//! - **Concurrent throughput**: `daemon_concurrent_10`
//!
//! ## Running Benchmarks
//!
//! ```bash
//! # Start the daemon first
//! bkt admin daemon run &
//!
//! # Run all benchmarks
//! cargo bench --bench daemon
//!
//! # Run specific benchmark
//! cargo bench --bench daemon -- daemon_echo
//! ```
//!
//! ## Interpreting Results
//!
//! - `daemon_echo` should be ~4ms (vs ~120ms for `flatpak_spawn_echo`)
//! - `daemon_sequential_10` shows consistency across requests
//! - `daemon_concurrent_10` reveals contention under load
//!
//! Note: Benchmarks skip gracefully if daemon isn't running.

use criterion::{Criterion, criterion_group, criterion_main};
use std::process::Command;
use std::time::Instant;

/// Check if the daemon is available before running benchmarks.
fn daemon_available() -> bool {
    Command::new("bkt")
        .args(["admin", "daemon", "status"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Benchmark: Daemon executing `echo hello`
fn bench_daemon_echo(c: &mut Criterion) {
    if !daemon_available() {
        eprintln!("Skipping daemon benchmarks: daemon not running");
        return;
    }

    c.bench_function("daemon_echo", |b| {
        b.iter(|| {
            let output = Command::new("bkt")
                .args(["admin", "daemon", "test", "echo", "hello"])
                .output()
                .expect("Failed to execute daemon test");
            assert!(output.status.success(), "Daemon test failed");
        })
    });
}

/// Benchmark: flatpak-spawn baseline for comparison
fn bench_flatpak_spawn_echo(c: &mut Criterion) {
    // Check if flatpak-spawn is available
    let available = Command::new("which")
        .arg("flatpak-spawn")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !available {
        eprintln!("Skipping flatpak-spawn benchmark: not available");
        return;
    }

    c.bench_function("flatpak_spawn_echo", |b| {
        b.iter(|| {
            let output = Command::new("flatpak-spawn")
                .args(["--host", "echo", "hello"])
                .output()
                .expect("Failed to execute flatpak-spawn");
            assert!(output.status.success(), "flatpak-spawn failed");
        })
    });
}

/// Benchmark: Component breakdown with timing
fn bench_daemon_components(c: &mut Criterion) {
    if !daemon_available() {
        return;
    }

    c.bench_function("daemon_components", |b| {
        b.iter_custom(|iters| {
            let mut total = std::time::Duration::ZERO;
            for _ in 0..iters {
                let start = Instant::now();
                let _ = Command::new("bkt")
                    .args(["admin", "daemon", "test", "true"])
                    .output();
                total += start.elapsed();
            }
            total
        })
    });
}

/// Benchmark: Sequential requests to measure consistency
fn bench_sequential_requests(c: &mut Criterion) {
    if !daemon_available() {
        return;
    }

    c.bench_function("daemon_sequential_10", |b| {
        b.iter(|| {
            for _ in 0..10 {
                let _ = Command::new("bkt")
                    .args(["admin", "daemon", "test", "true"])
                    .output();
            }
        })
    });
}

/// Benchmark: Concurrent requests using threads
fn bench_concurrent_requests(c: &mut Criterion) {
    if !daemon_available() {
        return;
    }

    c.bench_function("daemon_concurrent_10", |b| {
        b.iter(|| {
            let handles: Vec<_> = (0..10)
                .map(|_| {
                    std::thread::spawn(|| {
                        Command::new("bkt")
                            .args(["admin", "daemon", "test", "true"])
                            .output()
                    })
                })
                .collect();

            for h in handles {
                let _ = h.join();
            }
        })
    });
}

criterion_group!(
    benches,
    bench_daemon_echo,
    bench_flatpak_spawn_echo,
    bench_daemon_components,
    bench_sequential_requests,
    bench_concurrent_requests,
);
criterion_main!(benches);
