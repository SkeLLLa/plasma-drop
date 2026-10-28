# plasma-drop

<p align="center">
  <img src="resources/icons/plasma-drop-icon.svg" alt="plasma-drop icon" width="128" height="128">
</p>

[![CI](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/ci.yml)
[![Release](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml/badge.svg)](https://github.com/SkeLLLa/plasma-drop/actions/workflows/release.yml)
[![Crates.io](https://img.shields.io/crates/v/plasma-drop.svg)](https://crates.io/crates/plasma-drop)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](COPYING)
[![macOS: quartz-drop](https://img.shields.io/badge/macOS-quartz--drop-black?logo=apple)](https://github.com/SkeLLLa/quartz-drop)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/SkeLLLa/plasma-drop/badge)](https://scorecard.dev/viewer/?uri=github.com/SkeLLLa/plasma-drop)
[![SLSA Build L2](https://img.shields.io/badge/SLSA-Build%20L2-green)](docs/verifying.md#build-provenance)
[![Provenance](https://img.shields.io/endpoint?url=https%3A%2F%2Fskellla.github.io%2Fplasma-drop%2Fprovenance-badge.json)](docs/verifying.md)

`plasma-drop` is a KDE Plasma 6 dropdown app launcher. It registers global shortcuts through
KWin, finds or starts the apps you configure, and moves their windows into dropdown-style screen
positions.

Think Yakuake-style dropdown behavior, but for Dolphin, Kate, Chromium, or any other app you add to
the config.

It is heavily inspired by [windows-terminal-quake](https://github.com/flyingpie/windows-terminal-quake).
If you need a GUI, a Windows version, or a broader configuration surface, use that app.

On macOS, use the sibling project [quartz-drop](https://github.com/SkeLLLa/quartz-drop). It is a
Swift port of `plasma-drop` and reads the same config file.

**Contents:** [Install](#install) · [First Run](#first-run) · [Configure Apps](#configure-apps) ·
[Verify Your Download](#verify-your-download) · [Documentation](#documentation) ·
[Development](#development) ·
[Distribution and Releases](#distribution-and-releases) · [Support Ukraine](#support-ukraine)

## Demo

<p align="center">
  <video src="https://github.com/user-attachments/assets/08b73bf8-e472-49b2-b3bf-3aa910bf5d47" controls poster="https://github.com/user-attachments/assets/5eacbb37-b803-4998-ba2f-235818dec1f5" width="360">
    <img src="https://github.com/user-attachments/assets/5eacbb37-b803-4998-ba2f-235818dec1f5" alt="plasma-drop demo" width="720" height="450">
  </video>
  <video src="https://github.com/user-attachments/assets/dbfbf9c8-ea90-4d26-ac43-279864756e5a" controls poster="https://github.com/user-attachments/assets/5eacbb37-b803-4998-ba2f-235818dec1f5" width="360"></video>
</p>

## Install

You need KDE Plasma 6 on Wayland, with a session D-Bus and KWin scripting. `plasma-drop` doesn't
support X11 or other compositors. Packages and tarballs exist for x86_64 and aarch64.

| Distribution | How to install |
| --- | --- |
| Fedora and other DNF-based systems | [DNF repository](#fedora-dnf-repository), [.rpm](#a-single-deb-or-rpm-file), [tarball](#any-distribution-tarball) |
| Debian, Ubuntu and other APT-based systems | [APT repository](#debian-and-ubuntu-apt-repository), [.deb](#a-single-deb-or-rpm-file), [tarball](#any-distribution-tarball) |
| Any distribution | [tarball](#any-distribution-tarball), [mise, packslip or cargo](#mise-packslip-or-cargo) |

All downloads are on the [latest release](https://github.com/SkeLLLa/plasma-drop/releases/latest)
page.

### Fedora: DNF repository

This is the recommended method: `dnf upgrade` then keeps `plasma-drop` up to date.

```sh
sudo curl -fsSLo /etc/yum.repos.d/plasma-drop.repo https://skellla.github.io/plasma-drop/rpm/plasma-drop.repo
sudo dnf install plasma-drop
```

### Debian and Ubuntu: APT repository

This is the recommended method: `apt upgrade` then keeps `plasma-drop` up to date.

```sh
sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/plasma-drop.asc https://skellla.github.io/plasma-drop/plasma-drop.asc
echo 'deb [signed-by=/etc/apt/keyrings/plasma-drop.asc] https://skellla.github.io/plasma-drop/deb stable main' | sudo tee /etc/apt/sources.list.d/plasma-drop.list
sudo apt update
sudo apt install plasma-drop
```

The packages and the repository metadata are signed with the plasma-drop packages key,
[plasma-drop.asc](https://skellla.github.io/plasma-drop/plasma-drop.asc) (the same as
[packaging/signing/repo.asc](packaging/signing/repo.asc)); APT and DNF refuse anything it didn't
sign. DNF asks you to confirm the key's fingerprint on the first install. The repositories keep
the 10 most recent releases, so you can also pin or downgrade a version
([details](docs/distribution.md#linux-signing)).

### A single .deb or .rpm file

You get no automatic updates with this method. Download the file from the
[latest release](https://github.com/SkeLLLa/plasma-drop/releases/latest), then run:

```sh
sudo apt install ./plasma-drop_<version>_amd64.deb      # Debian, Ubuntu (arm64.deb on aarch64)
sudo dnf install ./plasma-drop-<version>-1.x86_64.rpm   # Fedora (aarch64.rpm on aarch64)
```

Packages install `/usr/bin/plasma-drop`, the user service
`/usr/lib/systemd/user/plasma-drop.service` and the starter config
`/usr/share/plasma-drop/examples/config.toml`.

### Any distribution: tarball

This method installs for your user only, into `~/.local`, and doesn't need root. It also copies
the starter config and installs the user service:

```sh
tar -xzf plasma-drop-<version>-<arch>-unknown-linux-gnu.tar.gz   # <arch>: x86_64 or aarch64
cd plasma-drop-<version>-<arch>-unknown-linux-gnu
./install-user.sh
```

### mise, packslip or cargo

These put `plasma-drop` on your `PATH`. Pick one:

```sh
# mise. Append @<version> to pin a release.
mise use -g github:SkeLLLa/plasma-drop

# packslip. Checks the signed release manifest before it installs into ~/.local/bin.
packslip install github.com/SkeLLLa/plasma-drop --pin ps1_4yd5vxao3lfzaox72lynxuotke

# cargo. Builds from crates.io.
cargo install plasma-drop --locked
```

The packslip `--pin` fingerprint identifies this repository (not a workflow) and stays the same
across releases. Check it against this README rather than trusting it on first use. With mise,
`mise use -g packslip:github.com/SkeLLLa/plasma-drop` also checks the signed manifest.

### Verify your download

Every release file is built by this repository's release workflow on GitHub-hosted runners and
carries signed [build provenance](https://github.com/SkeLLLa/plasma-drop/attestations) naming the
commit and the run that built it. APT, DNF and packslip check signatures for you; for a file from
the releases page, run:

```sh
gh attestation verify plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --repo SkeLLLa/plasma-drop \
  --signer-workflow SkeLLLa/plasma-drop/.github/workflows/release.yml \
  --source-ref refs/heads/master
```

Crates published by this pipeline carry the same provenance (releases with a `plasma-drop-<version>.crate` asset); see [docs/verifying.md](docs/verifying.md#cratesio).

Newer releases also attach `plasma-drop-<version>.intoto.jsonl` (use it with `--bundle`) and can
be immutable (marked Immutable on the release page; check with `gh release verify v<version>`). [docs/verifying.md](docs/verifying.md) covers every
artifact and what each check proves. To report a vulnerability, see [SECURITY.md](SECURITY.md).

### After installing

Create your config, then start the user service:

```sh
# APT, DNF or a .deb/.rpm file: copy the starter config
mkdir -p ~/.config/plasma-drop
cp /usr/share/plasma-drop/examples/config.toml ~/.config/plasma-drop/config.toml

# mise, packslip or cargo: write the config and the user service
# (use `plasma-drop init` to skip the service)
plasma-drop init --systemd

# Every method
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

The tarball's `install-user.sh` already did the first step. Next, see [First Run](#first-run).

### Uninstall

First stop the service with `systemctl --user disable --now plasma-drop.service`, then:

| Installed with | Remove with |
| --- | --- |
| DNF | `sudo dnf remove plasma-drop`, then `sudo rm /etc/yum.repos.d/plasma-drop.repo` |
| APT | `sudo apt remove plasma-drop`, then `sudo rm /etc/apt/sources.list.d/plasma-drop.list /etc/apt/keyrings/plasma-drop.asc` |
| Tarball or packslip | `rm ~/.local/bin/plasma-drop` |
| mise | `mise unuse -g github:SkeLLLa/plasma-drop` |
| cargo | `cargo uninstall plasma-drop` |

Every method except the packages also wrote `~/.config/systemd/user/plasma-drop.service`; delete
it too. Your config in `~/.config/plasma-drop` stays in place until you delete it.

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

Animation example:

```toml
[app.animation]
style = "slide"
direction = "left"  # top | bottom | left | right, slide entry/exit edge for slide and slide-fade
duration_ms = 180
```

Omit `direction` to use the default (`top`). It is ignored by `fade` and `none`.

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
| `[app.animation]`    | Optional slide/fade behavior, including slide `direction`          |

Focus-loss hiding is event-driven through `KWin`

See [resources/example-config.toml](resources/example-config.toml)
examples, and [docs/configuration.md](docs/configuration.md) for every supported option.

## Documentation

- [Getting started](docs/getting-started.md)
- [Configuration](docs/configuration.md)
- [Distribution](docs/distribution.md)
- [Verifying a release](docs/verifying.md)
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
enforces this with `opensource-nepal/commitlint` on pull requests, except for Dependabot's
generated dependency update commits.

The repo release flow is:

1. Push commits to `master`
2. `release-prepare.yml` runs `release-plz update`, which updates the version and `CHANGELOG.md`,
   and commits that release bump back to `master`
3. It then starts `release.yml` for that exact commit and waits for it; if `master` has moved on
   by then, the newest push's run redoes the bump instead
4. `release-plz release` tags that commit and creates a **draft** GitHub release
5. The workflow builds the `tar.gz`, `deb`, and `rpm` assets and packages the crate
   (`plasma-drop-<version>.crate`), signs the `rpm`, and writes
   `SHA256SUMS`, one `.sha256` sidecar per artifact, and the build provenance bundle
   `plasma-drop-<version>.intoto.jsonl`
6. A separate job signs a `packslip.sigstore.json` manifest for the `tar.gz`; every signature is
   verified before anything is uploaded
7. All files are uploaded to the draft and compared byte for byte with what was built
8. The exact attested `.crate` is published to crates.io (the workflow checks crates.io's
   checksum against it), then the release is published (immutable from then on when release
   immutability is enabled) and verified again from its public URLs; only then is the
   provenance badge updated
9. The workflow publishes signed RPM/APT repositories and the badge to GitHub Pages
   ([Linux signing](docs/distribution.md#linux-signing))

Crates.io publishing uses trusted publishing through GitHub Actions OIDC. After the first manual
crate publish, configure crates.io to trust `SkeLLLa/plasma-drop` and workflow `release.yml`; no
long-lived Cargo registry token is required for later releases.

## Support Ukraine

If `plasma-drop` is useful to you and you want to say thanks, please consider supporting Ukrainian
defenders instead of sending money to the author.

[![Come Back Alive](resources/badges/donate-come-back-alive.svg)](https://savelife.in.ua/en/donate-en/)
[![Sternenko Fund](resources/badges/donate-sternenko-fund.svg)](https://www.sternenkofund.org/en/donate)
[![Prytula Foundation](resources/badges/donate-prytula-foundation.svg)](https://prytulafoundation.org/en/donation)

## License

GPL-3.0-or-later. See [COPYING](COPYING).
