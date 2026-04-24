# Development

## Repo Layout

- `src/`: runtime code
- `resources/`: `KWin` script, service file, and sample config
- `docs/`: user and maintainer documentation

## Local Quality Workflow

The repo defines a single local verification entry point:

```bash
make check
```

That runs:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `cargo doc --no-deps --document-private-items`

Individual targets are also available through `make fmt`, `make clippy`, `make test`, and `make doc`.

## Editor Integration

Repo-local Zed settings live in:

- `.zed/settings.json`
- `.zed/tasks.json`

## GitHub Copilot Review

Copilot review configuration lives in `.github/copilot-instructions.md` and
`.github/instructions/*.instructions.md`.

See `docs/copilot-review.md` for the review request flow, automatic review settings, and
maintenance notes.

## Notes

- The current crate is a binary target, so crate docs are used as the main human-facing documentation surface for `cargo doc`.
