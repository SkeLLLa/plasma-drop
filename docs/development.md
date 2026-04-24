# Development

## Repo Layout

- `src/`: runtime code
- `resources/`: `KWin` script, service file, and sample config
- `docs/`: user and maintainer documentation

## Local Task Runners

The repo exposes local project scripts through both `make` and `mise`. Use `make` when you already
have the Rust toolchain installed, or `mise` when you want the pinned toolchain and helper tools
from `.mise.toml`.

Install pinned tools before running `mise` tasks:

```bash
mise install
```

Common scripts:

| Task | Make | mise |
| --- | --- | --- |
| Build | `make build` | `mise run build` |
| Format | `make fmt` | `mise run fmt` |
| Check format | `make fmt-check` | `mise run fmt-check` |
| Lint | `make lint` or `make clippy` | `mise run lint` or `mise run clippy` |
| Test | `make test` | `mise run test` |
| Docs | `make doc` | `mise run doc` |
| Full check | `make check` | `mise run check` |

The full local verification entry point is:

```bash
make check
mise run check
```

Both commands run:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `cargo doc --no-deps --document-private-items`

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
