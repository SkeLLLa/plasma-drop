# Distribution

## Goals

- Produce a standalone binary archive for users without distro packaging.
- Produce native `deb` and `rpm` packages for system-wide installation.
- Publish static RPM and APT repositories through GitHub Pages.
- Support `cargo install` without depending on separately installed resource files.
- Ship the files needed for a first launch: binary, user systemd unit, and example config.

The runtime `KWin` script is embedded into the binary with `include_str!`, so it does not need
to be installed as a separate runtime asset. The example config and user systemd unit are also
embedded so `cargo install` users can print them from the binary.

## Artifact Layout

### Binary archive

The release archive is intended for user-local installation and contains:

- `bin/plasma-drop`
- `share/systemd/user/plasma-drop.service`
- `share/plasma-drop/examples/config.toml`
- `install-user.sh`
- `README.md`
- license files

`install-user.sh` installs the binary into `~/.local/bin`, copies the example config into
`~/.config/plasma-drop/`, rewrites the service unit to point at the user-local binary, and
reloads the user systemd daemon when available.

### `deb` / `rpm`

Native packages install:

- `/usr/bin/plasma-drop`
- `/usr/lib/systemd/user/plasma-drop.service`
- `/usr/share/plasma-drop/examples/config.toml`
- `/usr/share/doc/plasma-drop/README.md`

The packaged service points at `/usr/bin/plasma-drop` and expects the active user config at
`~/.config/plasma-drop/config.toml`.

### GitHub Pages repositories

Release CI publishes unsigned package repositories at:

- `https://skellla.github.io/plasma-drop/rpm/x86_64`
- `https://skellla.github.io/plasma-drop/deb`

The RPM repository is generated with `createrepo_c --compatibility`. The APT repository uses a
`stable` suite and `main` component generated from the published `.deb` packages. GitHub Releases
remain the long-term artifact archive; Pages keeps a bounded recent package set.

### `cargo install`

`cargo install` only places the binary in Cargo's bin directory, so the starter assets are
exposed through CLI flags:

- `plasma-drop init`
- `plasma-drop init --systemd`
- `plasma-drop print-example-config`
- `plasma-drop print-systemd-service`

## CI Plan

GitHub Actions keeps source verification separate from release automation:

- `ci.yml`: fast quality checks on pushes and pull requests.
- `release.yml`: update the version and `CHANGELOG.md` directly on `master`, then create the tag,
  publish the crate, create the GitHub release, build release artifacts, and attach them to that
  release.

The release flow should:

1. `release-plz` analyzes commit history on `master`.
2. `release-plz update` edits the next version and `CHANGELOG.md` changes in the workflow checkout.
3. The workflow commits that release bump directly back to `master`.
4. `release-plz release` creates the tag, publishes to crates.io, and creates the GitHub release.
5. The same workflow builds the optimized binary once.
6. It assembles the binary tarball with the install helper and runtime assets.
7. It builds `deb` via `cargo-deb`.
8. It builds `rpm` via `cargo-generate-rpm`.
9. It verifies package contents before upload.
10. It generates a combined `SHA256SUMS` file and one `.sha256` sidecar per artifact.
11. It attaches the `tar.gz`, `deb`, `rpm`, `SHA256SUMS`, and per-artifact `.sha256` files to
    the GitHub release.
12. It builds and deploys RPM/APT repository metadata to GitHub Pages.

The release workflow uses the default `GITHUB_TOKEN` for repository operations. It does not depend
on a PAT to trigger a second workflow.

## Crates.io Trusted Publishing

`release.yml` has `id-token: write` on the release job and uses
`rust-lang/crates-io-auth-action` to request a short-lived crates.io token through GitHub Actions
OIDC. That token is passed to release-plz through `CARGO_REGISTRY_TOKEN`; no long-lived Cargo
registry secret is stored in the repository.

Trusted publishing cannot create a brand-new crate. Publish the first version manually, then
configure trusted publishing on crates.io for subsequent releases:

1. Publish the initial crate version from a local machine with a normal crates.io token:

   ```bash
   cargo publish --locked
   ```

2. Open the `plasma-drop` crate settings on crates.io.
3. Add a trusted publisher with:
   - Publisher: `GitHub`
   - Repository owner: `SkeLLLa`
   - Repository name: `plasma-drop`
   - Workflow filename: `release.yml`
   - Environment name: empty, unless the workflow later adds a GitHub Actions environment

After that setup, future release-plz runs can publish without a stored `CARGO_REGISTRY_TOKEN`
secret.

## First-Run Flow

### `cargo install`

1. Install the binary with `cargo install --locked plasma-drop`.
2. Create `~/.config/plasma-drop/config.toml` from `plasma-drop --print-example-config`.
3. If you use `systemd --user`, create `~/.config/systemd/user/plasma-drop.service` from
   `plasma-drop --print-systemd-service`.
4. Either enable the user service or add `plasma-drop --config ~/.config/plasma-drop/config.toml`
   to your session startup.

Example:

```bash
cargo install --locked plasma-drop
plasma-drop init --systemd
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

### Binary archive

1. Extract the archive.
2. Run `./install-user.sh`.
3. Edit `~/.config/plasma-drop/config.toml`.
4. Either enable the user service with `systemctl --user enable --now plasma-drop.service` or
   start `plasma-drop --config ~/.config/plasma-drop/config.toml` from your session startup.

Example:

```bash
tar -xzf plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz
cd plasma-drop-<version>-x86_64-unknown-linux-gnu
./install-user.sh
```

To verify a downloaded artifact before installing it, download the matching `.sha256` sidecar and
run:

```bash
sha256sum -c plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz.sha256
```

The release also includes `SHA256SUMS` for checking all downloaded artifacts at once.

### Native package

1. Install the package with the distro package manager.
2. Copy `/usr/share/plasma-drop/examples/config.toml` to `~/.config/plasma-drop/config.toml`.
3. Either enable the user service with `systemctl --user enable --now plasma-drop.service` or
   start `plasma-drop --config ~/.config/plasma-drop/config.toml` from your session startup.

Examples:

```bash
sudo dpkg -i plasma-drop_<version>_amd64.deb
```

```bash
sudo rpm -i plasma-drop-<version>-1.x86_64.rpm
```

Then:

```bash
mkdir -p ~/.config/plasma-drop
cp /usr/share/plasma-drop/examples/config.toml ~/.config/plasma-drop/config.toml
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
```

Without `systemd --user`:

```bash
plasma-drop --config ~/.config/plasma-drop/config.toml
```

### RPM repository

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

### APT repository

```bash
echo 'deb [trusted=yes] https://skellla.github.io/plasma-drop/deb stable main' | sudo tee /etc/apt/sources.list.d/plasma-drop.list
sudo apt update
sudo apt install plasma-drop
```
