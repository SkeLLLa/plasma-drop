---
applyTo: "**/*.rs,Cargo.toml,Cargo.lock,clippy.toml"
---

# Rust Review Instructions

Review Rust changes for this repository with these additional checks.

- Preserve the current Rust MSRV unless the PR intentionally changes `rust-version` and documents why.
- Prefer simple ownership and borrowing over new shared mutable state. Flag unnecessary `Arc`, `Mutex`, background tasks, or channels in hotkey/window lifecycle code.
- Treat D-Bus, KWin, file system, process spawning, and systemd calls as failure boundaries. Errors should include useful context and avoid panics in normal user flows.
- Check async code for cancellation safety, stuck awaits, missed timeouts, and dropped task handles.
- Check config parsing for sensible defaults, clear validation, and compatibility with existing config files.
- Keep `include_str!` paths valid for packaged crates. Any required compile-time file must be included by Cargo package rules.
- Prefer tests around pure behavior: config parsing, placement math, app matching, CLI argument handling, and serialization.
- Avoid suggesting dependencies unless they remove real complexity or match existing project patterns.
- Require `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `cargo doc --no-deps --document-private-items` for meaningful Rust changes.
