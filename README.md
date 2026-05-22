# plasma-drop

<p align="center">
  <img src="resources/icons/plasma-drop-icon.svg" alt="plasma-drop icon" width="128" height="128">
</p>

[![CI](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml)
[![Release](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/plasma-drop.svg)](https://crates.io/crates/plasma-drop)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](COPYING)

`plasma-drop` is a KDE Plasma 6 dropdown app launcher. It registers global shortcuts through
KWin, finds or starts the apps you configure, and moves their windows into dropdown-style screen
positions.

Think Yakuake-style dropdown behavior, but for Dolphin, Kate, Chromium, or any other app you add to
the config.

It is heavily inspired by [windows-terminal-quake](https://github.com/flyingpie/windows-terminal-quake).
If you need a GUI, a Windows version, or a broader configuration surface, use that app.

## Demo

<p align="center">
  <video src="https://github.com/user-attachments/assets/dbfbf9c8-ea90-4d26-ac43-279864756e5a" controls poster="https://github.com/user-attachments/assets/5eacbb37-b803-4998-ba2f-235818dec1f5" width="720">
    <img src="https://github.com/user-attachments/assets/5eacbb37-b803-4998-ba2f-235818dec1f5" alt="plasma-drop demo" width="720" height="450">
  </video>
</p>

## Requirements

- Linux desktop session
- KDE Plasma 6 on Wayland
- Session D-Bus
- KWin scripting support

`plasma-drop` does not target X11 or non-KWin compositors.

## Install

Choose one install path.

### From the GitHub Pages Package Repositories

Use this if you want updates through your system package manager.

For Fedora, openSUSE, and other RPM-based systems:

```bash
sudo tee /etc/yum.repos.d/plasma-drop.repo >/dev/null <<'EOF'
[plasma-drop]
name=plasma-drop
baseurl=https://skellla.github.io/plasma-drop/rpm/x86_64
enabled=1
gpgcheck=0
repo_gpgcheck=0
EOF
sudo dnf install plasma-drop
```

For Debian, Ubuntu, and other APT-based systems:

```bash
echo 'deb [trusted=yes] https://skellla.github.io/plasma-drop/deb stable main' | sudo tee /etc/apt/sources.list.d/plasma-drop.list
sudo apt update
sudo apt install plasma-drop
```

Then create your user config and enable the user service:

```bash
mkdir -p ~/.config/plasma-drop
cp /usr/share/plasma-drop/examples/config.toml ~/.config/plasma-drop/config.toml
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

### From Crates.io

Use this if you already have Rust and Cargo:

```bash
cargo install --locked plasma-drop
plasma-drop init --systemd
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

`plasma-drop init --systemd` creates:

- `~/.config/plasma-drop/config.toml`
- `~/.config/systemd/user/plasma-drop.service`

Use `plasma-drop init` instead if you do not want a `systemd --user` service file.

### From a GitHub Release Archive

Download and extract the `tar.gz` release asset, then run:

```bash
cd plasma-drop-<version>-x86_64-unknown-linux-gnu
./install-user.sh
systemctl --user enable --now plasma-drop.service
```

The installer places the binary in `~/.local/bin`, copies the starter config to
`~/.config/plasma-drop/config.toml`, and installs a user service for `systemd --user`.

### From `deb` or `rpm`

Install the package with your distro package manager, then create your user config:

```bash
mkdir -p ~/.config/plasma-drop
cp /usr/share/plasma-drop/examples/config.toml ~/.config/plasma-drop/config.toml
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

The native packages install:

- `/usr/bin/plasma-drop`
- `/usr/lib/systemd/user/plasma-drop.service`
- `/usr/share/plasma-drop/examples/config.toml`

### With `mise`

End users can install release builds through `mise`'s GitHub backend:

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

## First Run

Edit the starter config before relying on the service:

```bash
$EDITOR ~/.config/plasma-drop/config.toml
systemctl --user restart plasma-drop.service
```

Watch logs while testing:

```bash
journalctl --user -u plasma-drop.service -f
```

If your session does not use `systemd --user`, start `plasma-drop` from your session startup
instead:

```bash
plasma-drop --config ~/.config/plasma-drop/config.toml
```

For foreground debugging:

```bash
plasma-drop --config ~/.config/plasma-drop/config.toml -v
```

## Configure Apps

Configuration is TOML. Each `[[app]]` entry defines one managed app, its hotkey, how to find or
launch it, and where to place it.

Minimal example:

```toml
[[app]]
name = "dolphin"
hotkey = "super+f9"
filename = "/usr/bin/dolphin"
attach_mode = "find-or-start"
hide_decorations = true

[app.placement]
width = "50%"
height = "100%"
position = "left"
```

Common fields:

| Field | Purpose |
| --- | --- |
| `name` | Unique app identifier |
| `hotkey` | Global shortcut, for example `super+f9` |
| `filename` | App/window identity matcher |
| `command` | Explicit launch command array, useful for wrappers such as Flatpak |
| `attach_mode` | `find` or `find-or-start` |
| `hide_decorations` | Hide the KWin title bar and border while managed |
| `[app.placement]` | Width, height, position, and offsets |
| `[app.animation]` | Optional slide/fade behavior |

See [resources/example-config.toml](resources/example-config.toml) for native app and Flatpak
examples, and [docs/configuration.md](docs/configuration.md) for every supported option.

## Documentation

- [Getting started](docs/getting-started.md)
- [Configuration](docs/configuration.md)
- [Distribution](docs/distribution.md)
- [Development](docs/development.md)
- [GitHub Copilot review setup](docs/copilot-review.md)
- [Documentation index](docs/index.md)

`cargo doc --no-deps --document-private-items` uses the guide pages in `docs/` as the crate
documentation entry point.

## Development

This repo includes a pinned `.mise.toml` for contributor tooling. It is separate from the end-user
`mise use -g github:SkeLLLa/plasma-drop` install flow.

Typical setup:

```bash
mise install
mise run build
```

Run the full local quality suite with either task runner:

```bash
make check
mise run check
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

## Distribution and Releases

Release CI builds:

- `tar.gz` binary bundle with `install-user.sh`
- `deb`
- `rpm`
- GitHub Pages RPM/APT repository metadata

Each artifact ships the `plasma-drop` binary, a user systemd unit, and an example config. The
detailed install layout and CI plan live in [docs/distribution.md](docs/distribution.md).

Version bumps, changelog updates, tags, and GitHub releases are managed with `release-plz`.
Maintainers should use Conventional Commits so release-plz can infer the correct SemVer bump. CI
enforces this with `opensource-nepal/commitlint@v1` on pull requests, except for Dependabot's
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
crate publish, configure crates.io to trust `SkeLLLa/plasma-drop` and workflow `release.yml`; no
long-lived Cargo registry token is required for later releases.

## Support

If `plasma-drop` is useful to you and you want to say thanks, please consider supporting Ukrainian
defenders instead of sending money to the author.

[![Come Back Alive](resources/badges/donate-come-back-alive.svg)](https://savelife.in.ua/en/donate-en/)
[![Sternenko Fund](resources/badges/donate-sternenko-fund.svg)](https://www.sternenkofund.org/en/donate)
[![Prytula Foundation](resources/badges/donate-prytula-foundation.svg)](https://prytulafoundation.org/en/donation)
