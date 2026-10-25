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
- `resources/icons/plasma-drop-icon.svg`
- `resources/icons/plasma-drop-icon.png`
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
- `/usr/share/doc/plasma-drop/resources/icons/plasma-drop-icon.svg`
- `/usr/share/doc/plasma-drop/resources/icons/plasma-drop-icon.png`

The packaged service points at `/usr/bin/plasma-drop` and expects the active user config at
`~/.config/plasma-drop/config.toml`.

### GitHub Pages repositories

Release CI publishes signed package repositories ([Linux signing](#linux-signing)) at:

- `https://skellla.github.io/plasma-drop/rpm/x86_64`
- `https://skellla.github.io/plasma-drop/deb`

The RPM repository is generated with `createrepo_c --compatibility`. The APT repository uses a
`stable` suite and `main` component generated from the published `.deb` packages. GitHub Releases
remain the long-term artifact archive; Pages keeps the 10 most recent releases. Every directory of
the site has an `index.html` listing, since Pages serves none of its own.

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
10. `sign-linux` signs the `rpm` with the Linux signing subkey, and `verify-linux` checks it
    against `packaging/signing/repo.asc` and installs the packages in Fedora, Debian and Ubuntu.
11. It generates a combined `SHA256SUMS` file, one `.sha256` sidecar per artifact, and build
    provenance attestations.
12. It attaches the `tar.gz`, `deb`, `rpm`, `SHA256SUMS`, and per-artifact `.sha256` files to
    the GitHub release.
13. Once the tarball is attached, `jdx/packslip` signs a `packslip.sigstore.json` manifest
    covering it (used by `packslip install` and the `mise` packslip backend), recording the
    commit the tag points to. The signing job has read-only access; a separate job uploads the
    bundle, and a third verifies it against the published tarball and the signer fingerprint in
    `README.md`.
14. `repo` builds the RPM/APT repositories and signs their metadata; `pages` deploys them to
    GitHub Pages.

Every action is pinned to a commit SHA and every container image to a digest listed in
`packaging/ci/images.env`; `scripts/check-action-pins.sh` enforces it in CI and in the release's
`pin-gate` job, and `zizmor` checks the workflows. Jobs get no permissions unless they ask for
them. Only `master` saves the Rust cache in `ci.yml`, and `release.yml` uses none, so nothing a
pull request builds reaches a release. Dependabot updates the action SHAs after a 7-day cooldown;
the image digests in `images.env` are updated by hand.

The release workflow uses the default `GITHUB_TOKEN` for repository operations. It does not depend
on a PAT to trigger a second workflow.

## Linux signing

The .rpm files and the APT and DNF repository metadata are signed with an OpenPGP key: a
committed pin, a key that lives only in a `master`-only environment, and a release that fails
rather than ship unsigned or with another key once the pin exists. The key's primary part, which
users trust, stays offline with a passphrase; CI holds only a signing subkey, without the
primary's secret. The key is plasma-drop's own and signs no other project's packages.

| Signed                                | Checked by                                                |
| ------------------------------------- | --------------------------------------------------------- |
| each .rpm (release page and DNF repo) | dnf (`gpgcheck=1`), `rpm -K`                              |
| `rpm/x86_64/repodata/repomd.xml`      | dnf (`repo_gpgcheck=1`)                                   |
| APT `InRelease` and `Release.gpg`     | apt (`signed-by`); `InRelease` lists every .deb's SHA-256 |

The .deb files carry no signature of their own: APT never checks one (`dpkg-sig` and `debsigs`
are off by default), so the signed `InRelease` is what covers them. A .deb downloaded from the
release page is covered by the checksums and attestations instead.

**Pin.** `packaging/signing/repo.asc` holds the public key (the primary, its subkeys and their
revocations), armored, and only it. While it is absent, `sign-linux` passes the packages through
unsigned and `repo` builds unsigned repositories, with a notice. Once it is committed,
`packaging/ci/linux-signing.sh import` fails the release when the `REPO_SIGNING_KEY` secret is
missing, belongs to another primary key, or can't sign (passphrase, expiry, or a subkey the pin
revokes); setting the secret without committing the pin fails too. `import` also merges the pin
into CI's keyring, so the published `plasma-drop.asc` is the committed certificate.
`verify-linux` checks the .rpm against the pin before installing it, and `repo` checks every .rpm
it publishes.

**Where it signs.** `sign-linux` runs after `release`, in the Fedora 44 container with
`rpm-sign`, and replaces the `release-linux-rpm` artifact with the signed file, so
`verify-linux`, `checksums` and the release page see only that. No build runs in a job that holds
the key. `repo` (in the Debian trixie container: the runner's rpm 4.18 can't read a pin with a
revoked subkey) signs the metadata and also signs the .rpm files of the earlier releases it
republishes. The release's own is already signed and stays byte-identical, while those from
before the key, or signed by a since-revoked subkey, differ from their release page copies (and
checksums) by the signature. `pages` only deploys the site, so the key and the Pages deploy token
never share a job.

**Secret.** The subkey lives only in the `linux-signing` GitHub environment, used by
`sign-linux` and `repo`:

| Secret             | Value                                                                         |
| ------------------ | ----------------------------------------------------------------------------- |
| `REPO_SIGNING_KEY` | one signing subkey, armored, without a passphrase (`linux-signing.sh ci-key`) |

Settings → Environments → New environment `linux-signing`, deployment branches: only `master`
(optionally required reviewers). Don't keep it as a repository secret.

**Making the key.** On the maintainer's machine, not in CI, in a keyring kept for it (`gpg`
asks for the passphrase). RSA 4096 is what every supported rpm, dnf and apt (sqv) accepts. The
primary can only certify; the subkey can only sign:

```sh
export GNUPGHOME=~/.local/share/package-signing/gnupg   # any private directory
install -d -m 700 "$GNUPGHOME"
gpg --quick-gen-key 'plasma-drop packages <you@example.com>' rsa4096 cert never
FPR=<the primary's fingerprint, from gpg -K>
gpg --quick-add-key "$FPR" rsa4096 sign 3y
SUB=<the new subkey's fingerprint, from gpg -K --with-subkey-fingerprints>
gpg --armor --export "$FPR" > packaging/signing/repo.asc
packaging/ci/linux-signing.sh ci-key "$SUB" | gh secret set REPO_SIGNING_KEY --env linux-signing
```

`ci-key` exports that one subkey with the primary's secret left out and the passphrase removed.
Keep offline, apart from each other: the keyring (the primary's secret is passphrase-protected)
and the revocation certificate gpg wrote to `$GNUPGHOME/openpgp-revocs.d/<fingerprint>.rev`,
which revokes the whole key even if the keyring is lost. Commit `repo.asc` in the same change
that updates the README's install commands to the signed ones (the index page shows them); the
next release is the first signed one.

**Rotating or revoking the subkey.** Users trust the primary, so a subkey change needs no action
from them beyond the `plasma-drop.asc` they fetch. Before the subkey expires, or if the secret
leaks: with the offline keyring, revoke the old subkey if it leaked (`gpg --edit-key "$FPR"`,
`key N`, `revkey`, `save`), add a new one with `--quick-add-key`, export `repo.asc` again and set
the secret from `ci-key` for the new one. Commit `repo.asc`: until then, `import` refuses a
subkey the pin doesn't know, and once a revocation is committed it refuses the revoked one. Users
then get the revocation with the next `plasma-drop.asc` they fetch; dnf refuses packages signed
by the revoked subkey, and `repo` re-signs the earlier releases' with the new one. If the primary
itself leaks, import the revocation certificate (remove the `:` before `-----BEGIN` first),
commit the revoked `repo.asc` (every release fails from then on), and start over with a new key;
users have to fetch it again.

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
sudo curl -fsSLo /etc/yum.repos.d/plasma-drop.repo https://skellla.github.io/plasma-drop/rpm/plasma-drop.repo
sudo dnf install plasma-drop
```

### APT repository

```bash
sudo install -d -m 0755 /etc/apt/keyrings
sudo curl -fsSLo /etc/apt/keyrings/plasma-drop.asc https://skellla.github.io/plasma-drop/plasma-drop.asc
echo 'deb [signed-by=/etc/apt/keyrings/plasma-drop.asc] https://skellla.github.io/plasma-drop/deb stable main' | sudo tee /etc/apt/sources.list.d/plasma-drop.list
sudo apt update
sudo apt install plasma-drop
```
