# plasma-drop

<p align="center">
  <img src="resources/icons/plasma-drop-icon.svg" alt="plasma-drop icon" width="128" height="128">
</p>

[![CI](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml)
[![Release](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/plasma-drop.svg)](https://crates.io/crates/plasma-drop)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](COPYING)
[![macOS: quartz-drop](https://img.shields.io/badge/macOS-quartz--drop-black?logo=apple)](https://github.com/SkeLLLa/quartz-drop)

`plasma-drop` is a KDE Plasma 6 dropdown app launcher. It registers global shortcuts through
KWin, finds or starts the apps you configure, and moves their windows into dropdown-style screen
positions.

Think Yakuake-style dropdown behavior, but for Dolphin, Kate, Chromium, or any other app you add to
the config.

It is heavily inspired by [windows-terminal-quake](https://github.com/flyingpie/windows-terminal-quake).
If you need a GUI, a Windows version, or a broader configuration surface, use that app.

On macOS, use the sibling project [quartz-drop](https://github.com/SkeLLLa/quartz-drop). It is a
Swift port of `plasma-drop` and reads the same config file.

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

Pick one install method. All of them target Linux `x86_64`.

| Method | Command | Service unit and example config |
| --- | --- | --- |
| Fedora / RPM | [DNF repository](#rpm-and-apt-repositories), then `sudo dnf install plasma-drop` | Installed by the package |
| Debian / Ubuntu | [APT repository](#rpm-and-apt-repositories), then `sudo apt install plasma-drop` | Installed by the package |
| Any (Rust) | `cargo install --locked plasma-drop` | `plasma-drop init --systemd` |
| mise | `mise use -g github:SkeLLLa/plasma-drop` | `plasma-drop init --systemd` |
| packslip | `packslip install github.com/SkeLLLa/plasma-drop --pin ps1_4yd5vxao3lfzaox72lynxuotke` | `plasma-drop init --systemd` |
| Release archive | Extract the [release](https://github.com/SkeLLLa/plasma-drop/releases/latest) `tar.gz`, run `./install-user.sh` | Installed by the script |

Then finish setup for your method:

```bash
# Package repositories or a downloaded deb/rpm:
mkdir -p ~/.config/plasma-drop
cp /usr/share/plasma-drop/examples/config.toml ~/.config/plasma-drop/config.toml

# cargo, mise, or packslip: writes ~/.config/plasma-drop/config.toml and
# ~/.config/systemd/user/plasma-drop.service (use `plasma-drop init` to skip the unit)
plasma-drop init --systemd

# Every method:
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

Notes:

- **mise:** append `@<version>` to pin a release. Releases from 1.9.0 on also publish a signed
  `packslip.sigstore.json` manifest; `mise use -g packslip:github.com/SkeLLLa/plasma-drop`
  installs through it.
- **packslip:** [packslip](https://packslip.dev) verifies the signed release manifest before
  installing into `~/.local/bin`; add `--version <version>` to pin a release. The `--pin`
  fingerprint identifies this repository's release workflow and stays the same across releases;
  check it against this README rather than trusting it on first use.
- **Release archive:** `install-user.sh` places the binary in `~/.local/bin`, copies the starter
  config, and installs the user service. The same release page has standalone `deb` and `rpm`
  files.
- Native packages install `/usr/bin/plasma-drop`, `/usr/lib/systemd/user/plasma-drop.service`,
  and `/usr/share/plasma-drop/examples/config.toml`.

### RPM and APT repositories

These repositories are unsigned; they track every release.

```bash
# Fedora, openSUSE, other RPM-based systems
sudo tee /etc/yum.repos.d/plasma-drop.repo >/dev/null <<'EOF'
[plasma-drop]
name=plasma-drop
baseurl=https://skellla.github.io/plasma-drop/rpm/x86_64
enabled=1
gpgcheck=0
repo_gpgcheck=0
EOF
sudo dnf install plasma-drop

# Debian, Ubuntu, other APT-based systems
echo 'deb [trusted=yes] https://skellla.github.io/plasma-drop/deb stable main' | sudo tee /etc/apt/sources.list.d/plasma-drop.list
sudo apt update
sudo apt install plasma-drop
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
hide_behavior = "minimize"
hide_on_focus_lost = true

[app.placement]
width = "50%"
height = "100%"
position = "left"
```

Common fields:

| Field                | Purpose                                                            |
| -------------------- | ------------------------------------------------------------------ |
| `name`               | Unique app identifier                                              |
| `hotkey`             | Global shortcut, for example `super+f9`                            |
| `filename`           | App/window identity matcher                                        |
| `command`            | Explicit launch command array, useful for wrappers such as Flatpak |
| `attach_mode`        | `find` or `find-or-start`                                          |
| `hide_decorations`   | Hide the KWin title bar and border while managed                   |
| `hide_behavior`      | `offscreen` (default) or KWin `minimize` when hidden               |
| `hide_on_focus_lost` | Hide after focus moves to another window                           |
| `[app.placement]`    | Width, height, position, offsets, and target screen                |
| `[app.animation]`    | Optional slide/fade behavior                                       |

Focus-loss hiding is event-driven through `KWin`

See [resources/example-config.toml](resources/example-config.toml)
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

For a release binary, use the release task:

```bash
mise run build-release
```

The release binary is written to `target/release/plasma-drop`.

Mise routes these Cargo commands through Mr Boxington, which reuses compiled Rust work across builds
on this machine. The first build fills the cache; later matching builds can reuse it. Use `mise run`
or `mise exec` when you want this cache. The Make targets and GitHub Actions continue to use regular
Cargo commands and do not require mise or Mr Boxington.

Run the full local quality suite with either task runner:

```bash
make check
mise run check
```

Common project scripts:

| Task         | Make                         | mise                                 |
| ------------ | ---------------------------- | ------------------------------------ |
| Build        | `make build`                 | `mise run build`                     |
| Release build | —                           | `mise run build-release`             |
| Format       | `make fmt`                   | `mise run fmt`                       |
| Check format | `make fmt-check`             | `mise run fmt-check`                 |
| Lint         | `make lint` or `make clippy` | `mise run lint` or `mise run clippy` |
| Test         | `make test`                  | `mise run test`                      |
| Docs         | `make doc`                   | `mise run doc`                       |
| Full check   | `make check`                 | `mise run check`                     |

Both runners use the same Cargo operations; mise also enables the Mr Boxington build cache.

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
7. Separate jobs sign a `packslip.sigstore.json` manifest for the `tar.gz`, upload it, and verify
   the published release against the signer fingerprint above
8. The workflow publishes unsigned RPM/APT repository metadata to GitHub Pages

Crates.io publishing uses trusted publishing through GitHub Actions OIDC. After the first manual
crate publish, configure crates.io to trust `SkeLLLa/plasma-drop` and workflow `release.yml`; no
long-lived Cargo registry token is required for later releases.

## Support

If `plasma-drop` is useful to you and you want to say thanks, please consider supporting Ukrainian
defenders instead of sending money to the author.

[![Come Back Alive](resources/badges/donate-come-back-alive.svg)](https://savelife.in.ua/en/donate-en/)
[![Sternenko Fund](resources/badges/donate-sternenko-fund.svg)](https://www.sternenkofund.org/en/donate)
[![Prytula Foundation](resources/badges/donate-prytula-foundation.svg)](https://prytulafoundation.org/en/donation)
