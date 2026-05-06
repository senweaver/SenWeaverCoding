//! sen-cli — Workspace CLI entry point (secondary binary).
//!
//! The primary `sen` binary lives in the root crate at `src/main.rs`.
//! This binary is a workspace structural placeholder that delegates to
//! `senweavercoding` and is available as `sen-tool` for workspace tooling.

fn main() {
    eprintln!("Use the `sen` binary from the root crate (`cargo run --bin sen`). This is a workspace structural placeholder.");
    std::process::exit(1);
}
