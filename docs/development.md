# Development

## Repo Layout

- `src/`: runtime code
- `resources/`: `KWin` script, service file, sample config, and visual assets
- `docs/`: user and maintainer documentation

Visual assets live under:

- `resources/icons/plasma-drop-icon.svg`: compact README/docs mark.
- `resources/icons/plasma-drop-icon.png`: PNG fallback shipped with native packages.
- `resources/icons/plasma-drop.svg` and `resources/icons/plasma-drop-512.png`: larger app icon variants.
- `resources/social/github-social-preview.svg` and `.png`: GitHub repository social preview assets.

## Local Task Runners

The repo exposes local project scripts through both `make` and `mise`. Use `make` when you already
have the Rust toolchain installed, or `mise` when you want the pinned toolchain and helper tools
from `.mise.toml`. The `mise` tasks that compile run Cargo through `mbx`
([Mr Boxington](https://mr-boxington.jdx.dev)), which caches compiled crates across
checkouts; `make` and CI use plain Cargo.

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
| Dependency audit | `make lint-deny` | `mise run lint-deny` |
| Complexity ceilings (needs `cccc`) | `make lint-complexity` | `mise run lint-complexity` |
| `KWin` script analysis (needs `fallow`) | `make lint-js` | `mise run lint-js` |
| Shell lint | `make lint-shell` | `mise run lint-shell` |
| Workflow lint and action pins (needs `jactionlint`, `bats`) | `make lint-actions` | `mise run lint-actions` |
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
- `cargo deny --locked check` (licenses, bans, sources and security advisories, configured in
  `deny.toml`; fetches the advisory database)

Lints are configured in `Cargo.toml` (`[lints]`) and `clippy.toml`. Beyond clippy's
`pedantic` and `nursery` groups they include restriction lints from
[BeautifulRust](https://github.com/morganlinton/BeautifulRust): no `unwrap()` outside tests
(use `expect("why")`), `#[expect(lint, reason = "...")]` instead of `#[allow]`, a
`// SAFETY:` comment on every `unsafe` block, `Arc::clone(&x)`, no lossy `as` casts, and no
`bool` function parameters (use a two-variant enum).

## Editor Integration

Repo-local Zed settings live in:

- `.zed/settings.json`
- `.zed/tasks.json`

## GitHub Copilot Review

Copilot review configuration lives in `.github/copilot-instructions.md` and
`.github/instructions/*.instructions.md`.

See `docs/copilot-review.md` for the review request flow, automatic review settings, and
maintenance notes.

## Sibling Project

[quartz-drop](https://github.com/SkeLLLa/quartz-drop) is the macOS (Swift) port of `plasma-drop`
and is usually checked out next to this repo as `../quartz-drop`. It parses the same config
format, so config changes here (new fields, defaults, validation rules) should be mirrored there,
and its `docs/configuration.md` "Differences from plasma-drop" section kept up to date.

## Notes

- The current crate is a binary target, so crate docs are used as the main human-facing documentation surface for `cargo doc`.
