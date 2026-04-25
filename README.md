# plasma-drop

[![CI](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml)
[![Release](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/plasma-drop.svg)](https://crates.io/crates/plasma-drop)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](COPYING)

`plasma-drop` is a KDE Plasma 6 dropdown app launcher built around KWin scripting and global shortcuts.

It is heavily inspired by [windows-terminal-quake](https://github.com/flyingpie/windows-terminal-quake).
If you need a GUI, a Windows version, or more configuration options, use that app.

## Documentation

- Documentation index: [docs/index.md](docs/index.md)
- Getting started: [docs/getting-started.md](docs/getting-started.md)
- Configuration: [docs/configuration.md](docs/configuration.md)
- Development: [docs/development.md](docs/development.md)
- GitHub Copilot review setup: [docs/copilot-review.md](docs/copilot-review.md)
- Distribution: [docs/distribution.md](docs/distribution.md)

`cargo doc --no-deps --document-private-items` uses the guide pages in `docs/` as the crate documentation entry point.

## Quality checks

Run the full local quality suite with either task runner:

```bash
make check
mise run check
```

Install pinned development tools first when using `mise`:

```bash
mise install
```

Common project scripts:

| Task | Make | mise |
| --- | --- | --- |
| Build | `make build` | `mise run build` |
| Format | `make fmt` | `mise run fmt` |
| Check format | `make fmt-check` | `mise run fmt-check` |
| Lint | `make lint` or `make clippy` | `mise run lint` or `mise run clippy` |
| Test | `make test` | `mise run test` |
| Docs | `make doc` | `mise run doc` |
| Full check | `make check` | `mise run check` |

Both runners delegate to the same Cargo commands.

## Distribution

Release CI now targets three Linux artifact types:

- `tar.gz` binary bundle with `install-user.sh`
- `deb`
- `rpm`
- GitHub Pages RPM/APT repositories

Each artifact ships the `plasma-drop` binary, a user systemd unit, and an example config for a
quicker first run. The detailed install layout and CI plan live in
[docs/distribution.md](docs/distribution.md).

## Releases

Version bumps, changelog updates, tags, and GitHub releases are managed with `release-plz`.
Maintainers should use Conventional Commits so release-plz can infer the correct SemVer bump.
CI enforces this with `opensource-nepal/commitlint@v1` on pull requests, except for Dependabot's
generated dependency update commits.

The repo release flow is:

1. Push commits to `master`
2. `release-plz update` updates the version and `CHANGELOG.md` directly in the workflow checkout
3. The workflow commits that release bump back to `master`
4. `release-plz release` publishes the crate, creates the tag, and creates the GitHub release
5. The same workflow attaches the `tar.gz`, `deb`, and `rpm` assets to that release
6. The workflow also attaches `SHA256SUMS` and one `.sha256` checksum sidecar per artifact
7. The workflow publishes unsigned RPM/APT repository metadata to GitHub Pages

Crates.io publishing uses trusted publishing through GitHub Actions OIDC. After the first manual
crate publish, configure crates.io to trust `SkeLLLa/plasma-drop` and workflow `release.yml`;
no long-lived Cargo registry token is required for later releases.

## Cargo Install

`plasma-drop` is also designed to work with `cargo install`:

```bash
cargo install --locked plasma-drop
plasma-drop init --systemd
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

Without `systemd --user`, skip the service file and start it from your session startup:

```bash
plasma-drop init
plasma-drop --config ~/.config/plasma-drop/config.toml
```

## Install With `mise`

End users can also install `plasma-drop` from GitHub releases via `mise`'s `github` backend:

```bash
mise use -g github:SkeLLLa/plasma-drop
plasma-drop init --systemd
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

To pin a specific release:

```bash
mise use -g github:SkeLLLa/plasma-drop@1.0.0
```

In a personal `mise.toml`, the same install can be declared as:

```toml
[tools]
"github:SkeLLLa/plasma-drop" = "latest"
```

Without `systemd --user`, initialize the config and launch it from your session startup:

```bash
plasma-drop init
plasma-drop --config ~/.config/plasma-drop/config.toml
```

## Development Setup With `mise`

This repo includes a pinned `.mise.toml` for local
tooling. That is intended for contributors and local automation. It is separate from the
end-user `mise use -g github:SkeLLLa/plasma-drop` install flow above.

Typical setup:

```bash
mise install
mise run build
```

What `mise` provides here:

- CLI tools pinned in `.mise.toml`
- project-local development commands such as `mise run check`
- MCP helper tools used in this workspace

For end users who just want to run `plasma-drop`, prefer one of:

- `mise use -g github:SkeLLLa/plasma-drop`
- `cargo install --locked plasma-drop`
- the GitHub release `tar.gz`
- the GitHub release `deb`
- the GitHub release `rpm`
