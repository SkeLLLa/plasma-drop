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

The release archive (`x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`) is intended for
user-local installation and contains:

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

Native packages are built for `x86_64` (`amd64.deb`, `x86_64.rpm`) and `aarch64` (`arm64.deb`,
`aarch64.rpm`) and install:

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

- `https://skellla.github.io/plasma-drop/rpm/x86_64` and `.../rpm/aarch64`
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
- `release-prepare.yml`: on every push to `master`, commit the version bump and `CHANGELOG.md`
  update to `master`, then start `release.yml` for that commit and wait for it, failing if the
  release fails.
- `release.yml`: started on `master` by `release-prepare.yml` with the commit to release as
  input `sha`; tags the bump commit, drafts the release, builds, signs and verifies everything,
  then publishes.

The release flow:

1. `release-prepare.yml` acts only if its commit is still `master`'s HEAD; otherwise the run of
   the newer push does the work. It then checks that the previous release is complete. If the
   last bump was never tagged, this run undoes it locally and bumps again over every commit since
   the last release, so the changelog never misses a commit. If it is tagged but not published
   (on GitHub or crates.io), it fails and points to [Recovery](#recovery).
2. `release-plz update` edits the next version and `CHANGELOG.md` in the workflow checkout, and
   the workflow commits that bump directly to `master`.
3. It starts `release.yml` with a `workflow_dispatch` on `master`, passing the bump commit as
   input `sha`, and watches that run. If `master` is no longer that commit when the run is
   dispatched, or just before it would tag (a run that queued, or a re-run), the run tags nothing
   and ends green; the newest push's prepare run releases instead. Once the tag exists on that
   commit, re-runs always go through, so a tagged release can be finished.
4. `release-plz release` creates the tag on the commit this run was started for and a **draft**
   release; crates.io comes later. The job then checks that the tag points to exactly that commit.
5. The read-only `linux-assets` job checks out that commit and runs once per architecture
   (`x86_64` and `aarch64`), each on a native runner (`ubuntu-24.04`, `ubuntu-24.04-arm`). It
   builds the optimized binary.
6. It assembles the binary tarball with the install helper and runtime assets.
7. It builds `deb` via `cargo-deb`.
8. It builds `rpm` via `cargo-generate-rpm`.
9. It verifies package contents and architectures before upload. The tarball, `.deb` and `.rpm`
   of each architecture are separate artifacts (`release-linux-{tar,deb,rpm}-<arch>`).
10. `crate-package` runs `cargo package --locked`, which also builds the packaged source, and
    keeps `plasma-drop-<version>.crate` with the Rust version it used. If an earlier attempt of
    the same run already published the version, it keeps crates.io's file instead, after
    checking crates.io records this run.
11. `sign-linux` signs both `rpm` files with the Linux signing subkey, and `verify-linux` checks
    them against `packaging/signing/repo.asc` and installs the packages in Fedora 44, Debian
    trixie and Ubuntu 26.04 (`x86_64`), and Fedora 44 and Debian trixie (`aarch64`).
12. `checksums` generates a combined `SHA256SUMS` file, one `.sha256` sidecar per artifact, and
    build provenance attestations for the 6 Linux files and the crate, kept as
    `plasma-drop-<version>.intoto.jsonl`.
13. `jdx/packslip` signs a `packslip.sigstore.json` manifest covering both local tarballs, one
    `linux/<arch>` entry each (used by `packslip install` and the `mise` packslip backend),
    recording the tagged commit. The signing job has read-only access.
14. `verify-signatures` checks the checksums, the packslip manifest against the signer
    fingerprint in `README.md` and the exact signer `release.yml@refs/heads/master`, and the
    provenance bundle, all before anything is uploaded.
15. `upload-draft` uploads all 17 files to the draft release, downloads them again and compares
    them byte for byte with what was built.
16. `repo` builds the RPM/APT repositories and signs their metadata, without deploying them.
17. `publish-crate` re-packages the crate with the same Rust version and checks it is
    byte-identical to the attested file, then runs `cargo publish --no-verify` (no build runs
    while the crates.io token exists) and checks that crates.io's checksum and
    trusted-publishing record match the attested file, this commit and this run.
18. `publish` turns the draft into a published release, marking it `latest` only if no higher
    version is published. With release immutability on, its assets and tag are fixed from then on.
19. `verify-published` checks the public release and crates.io the way users do (every file's
    provenance, packslip, the crate, and for immutable releases `gh release verify`), then
    writes the provenance badge. `pages` adds it to the site `repo` built and deploys them,
    unless a newer release is already published.

Nothing is publicly visible (crate, published release, `latest`, Pages) until `verify-linux`,
`verify-signatures`, `upload-draft`'s byte comparison and `repo`'s site checks have all passed.
The order is `publish-crate`, `publish`, `verify-published`, then `pages`. `publish` is the only
job with `contents: write` after `upload-draft`, and compares the draft with the verified files
once more right before publishing it.

The tag `vX.Y.Z` appears when the Release run starts. A tag without a published release is not a
release; install only from published releases.

Every action is pinned to a commit SHA and every container image to a digest listed in
`.ci/images.env`; `.ci/check-action-pins.sh` enforces it in CI and in the `pin-gate`
job of both release workflows (`.ci/pin-gate.sh`), and `zizmor` checks the workflows. Jobs get
no permissions unless they ask for them. Only `master` saves the Rust cache in `ci.yml`, and the
release workflows use none, so nothing a pull request builds reaches a release. Dependabot updates
the action SHAs after a 7-day cooldown; the image digests in `images.env` are updated by hand.

Both workflows use only the default `GITHUB_TOKEN`. `release-prepare.yml` starts `release.yml`
with a `workflow_dispatch` on `master`, the one kind of event a `GITHUB_TOKEN` can start, so no PAT
or GitHub App is needed. Because the release run is started for the version-bump commit itself,
its build provenance names the exact commit the tag points to. The committed `release.yml`
refuses any ref but `master` (its `pin-gate` is skipped there, and so is everything else), and the
`linux-signing`, `crates-io` and `github-pages` environments allow only `master`. The crates.io
trusted publisher requires the `crates-io` environment, so only `release.yml` on `master` can
publish the crate. Verifiers check `--source-ref refs/heads/master`, which rejects a build from
any other branch.

### Recovery

"Re-run failed jobs" in the Actions UI re-runs the same Release run at the same commit and reuses
the outputs and artifacts of the jobs that passed (artifacts are kept 7 days, runs can be re-run
for 30 days). "Re-run all jobs" rebuilds everything at the same commit. Every job is safe to
re-run: the draft gets every file again, stray files are removed, and the bytes are compared;
`publish-crate` and `publish` do nothing if their part is already done.

| Fails at | Public state | Recovery |
| --- | --- | --- |
| `release-prepare.yml` before the bump is pushed | nothing | fix and push, or `gh workflow run release-prepare.yml --ref master` |
| bump pushed, release never started or failed before tagging (Release (prepare) says "before tagging") | bump commit on `master`, no tag | Push anything, or run `gh workflow run release-prepare.yml --ref master`. The next prepare run redoes the bump over every commit since the last release and starts the release |
| Release (prepare) says "after tagging" | tag, maybe a draft or a published release | Re-run the failed jobs of the Release run it links, not the prepare run |
| `release` after tagging (draft exists or tag only) | tag only | Re-run the failed jobs of that Release run (Actions → the run → Re-run failed jobs). Never start a new one: prepare runs fail until it is published |
| any job from `linux-assets` to `publish` | draft release and tag; the next prepare fails until this is fixed | Re-run failed jobs (within 7 days), else Re-run all jobs (within 30 days). If trusted publishing is misconfigured, fix it on crates.io first |
| `publish-crate`: could not ask crates.io (outage) | draft and tag | Re-run failed jobs once crates.io answers (within 7 days); later, Re-run all jobs (it re-packages and re-attests) |
| `publish-crate`: "Package the crate again" differs | draft and tag; nothing published | A reproducibility bug: investigate; Re-run all jobs re-packages and re-attests |
| `crate-package` or `publish-crate`: crates.io holds another checksum, or another publisher/run | **crate public**, draft and tag | Don't publish the draft. Yank the version on crates.io, delete the draft and the tag, release the next version, and treat a foreign publisher as an incident |
| `verify-published` | published, immutable; Pages not deployed (APT/DNF and the badge stay on the previous release) | A download or network failure: Re-run failed jobs (this also deploys Pages). A failed check: the assets can't change; investigate; if they are bad, delete the release and release a new version (the tag name can't be reused) |
| `pages` only | published, Pages stale | Re-run failed jobs; it deploys nothing if a newer release is published |
| run older than 30 days, still a draft, crate not published | draft and tag | Delete the draft (`gh release delete vX.Y.Z --yes`) and the tag (`git push origin :refs/tags/vX.Y.Z`, admin bypass of the tag ruleset), then run `gh workflow run release-prepare.yml --ref master` |
| same, crate already published | crate and draft | the assets were already compared byte for byte: `gh release edit v<version> --draft=false`, then verify by hand ([verifying.md](verifying.md)). Pages and the badge update with the next release. |
| the newest push's commit message contains `[skip ci]` | nothing | No prepare run starts; run `gh workflow run release-prepare.yml --ref master` |

Don't start `release.yml` by hand: `master` may hold commits on top of a bump that was never
tagged, and only `release-prepare.yml` redoes that bump so the changelog lists them. Run
`gh workflow run release-prepare.yml --ref master` instead.

### Immutable releases

Releases are drafted first and published only once every file is uploaded and verified, so they
can be made immutable (Settings → General → Releases → release immutability). Immutability
depends on that repository setting: only releases published while it is on are immutable, and
`verify-published` runs `gh release verify` only for those. Enable it only after a release built
by this draft-first pipeline has been published successfully; enabled earlier, a release
published before its files were attached would be locked empty. Releases published before the
setting was enabled stay mutable.

Protect the release tags with a tag ruleset (Settings → Rules → Rulesets): target `refs/tags/v*`,
restrict updates and deletions, block force pushes, with the repository admin as the only bypass.
Don't restrict creations: `release.yml` creates the tag with `GITHUB_TOKEN`.

## Linux signing

The .rpm files and the APT and DNF repository metadata are signed with an OpenPGP key: a
committed pin, a key that lives only in a `master`-only environment, and a release that fails
rather than ship unsigned or with another key once the pin exists. The key's primary part, which
users trust, stays offline with a passphrase; CI holds only a signing subkey, without the
primary's secret. The key is plasma-drop's own and signs no other project's packages.

| Signed                                | Checked by                                                |
| ------------------------------------- | --------------------------------------------------------- |
| each .rpm (release page and DNF repo) | dnf (`gpgcheck=1`), `rpm -K`                              |
| `rpm/<arch>/repodata/repomd.xml`      | dnf (`repo_gpgcheck=1`)                                   |
| APT `InRelease` and `Release.gpg`     | apt (`signed-by`); `InRelease` lists every .deb's SHA-256 |

The .deb files carry no signature of their own: APT never checks one (`dpkg-sig` and `debsigs`
are off by default), so the signed `InRelease` is what covers them. A .deb downloaded from the
release page is covered by the checksums and attestations instead.

**Pin.** `packaging/signing/repo.asc` holds the public key (the primary, its subkeys and their
revocations), armored, and only it. While it is absent, `sign-linux` passes the packages through
unsigned and `repo` builds unsigned repositories, with a notice. Once it is committed,
`.ci/linux-signing.sh import` fails the release when the `REPO_SIGNING_KEY` secret is
missing, belongs to another primary key, or can't sign (passphrase, expiry, or a subkey the pin
revokes); setting the secret without committing the pin fails too. `import` also merges the pin
into CI's keyring, so the published `plasma-drop.asc` is the committed certificate.
`verify-linux` checks each .rpm against the pin before installing it, and `repo` checks every .rpm
it publishes.

**Where it signs.** `sign-linux` runs after `linux-assets`, in the Fedora 44 container with
`rpm-sign`, and replaces each `release-linux-rpm-<arch>` artifact with the signed file, so
`verify-linux`, `checksums` and the release page see only that. No build runs in a job that holds
the key. `repo` (in the Debian trixie container: the runner's rpm 4.18 can't read a pin with a
revoked subkey) signs the metadata and also signs the .rpm files of the earlier releases it
republishes. The release's own is already signed and stays byte-identical, while those from
before the key, or signed by a since-revoked subkey, differ from their release page copies (and
checksums) by the signature. `pages` only adds the provenance badge to the site and deploys it, so the key and the Pages deploy token
never share a job.

**Secret.** The subkey lives only in the `linux-signing` GitHub environment, used by
`sign-linux` and `repo`:

| Secret             | Value                                                                         |
| ------------------ | ----------------------------------------------------------------------------- |
| `REPO_SIGNING_KEY` | one signing subkey, armored, without a passphrase (`linux-signing.sh ci-key`) |

Settings → Environments → New environment `linux-signing`, deployment branches: only `master`
(optionally required reviewers). Don't keep it as a repository secret. Both release workflows run
on `master`; never add tag patterns.

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
.ci/linux-signing.sh ci-key "$SUB" | gh secret set REPO_SIGNING_KEY --env linux-signing
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

The `publish-crate` job in `release.yml` has `id-token: write` and uses
`rust-lang/crates-io-auth-action` to request a short-lived crates.io token through GitHub Actions
OIDC. That token is passed to `cargo publish` through `CARGO_REGISTRY_TOKEN`; no long-lived Cargo
registry secret is stored in the repository. The job runs only after every release check has
passed. release-plz no longer publishes (`publish = false` in `release-plz.toml`); the trusted
publisher configuration below is unchanged.

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
   - Environment name: `crates-io`

   The `crates-io` environment allows deployments only from `master`; create it with:

   ```bash
   gh api -X PUT repos/SkeLLLa/plasma-drop/environments/crates-io \
     -F 'deployment_branch_policy[protected_branches]=false' \
     -F 'deployment_branch_policy[custom_branch_policies]=true'
   gh api -X POST repos/SkeLLLa/plasma-drop/environments/crates-io/deployment-branch-policies \
     -f name=master -f type=branch
   ```

After that setup, future releases can publish without a stored `CARGO_REGISTRY_TOKEN` secret.

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
tar -xzf plasma-drop-<version>-<arch>-unknown-linux-gnu.tar.gz
cd plasma-drop-<version>-<arch>-unknown-linux-gnu
./install-user.sh
```

To verify a downloaded artifact before installing it, download the matching `.sha256` sidecar and
run:

```bash
sha256sum -c plasma-drop-<version>-<arch>-unknown-linux-gnu.tar.gz.sha256
```

The release also includes `SHA256SUMS` for checking all downloaded artifacts at once.
Checksums only catch a corrupted download; to check that a file was built by this repository's
release workflow, verify its build provenance as described in [verifying.md](verifying.md).

### Native package

1. Install the package with the distro package manager.
2. Copy `/usr/share/plasma-drop/examples/config.toml` to `~/.config/plasma-drop/config.toml`.
3. Either enable the user service with `systemctl --user enable --now plasma-drop.service` or
   start `plasma-drop --config ~/.config/plasma-drop/config.toml` from your session startup.

Examples:

```bash
sudo dpkg -i plasma-drop_<version>_amd64.deb   # arm64.deb on aarch64
```

```bash
sudo rpm -i plasma-drop-<version>-1.x86_64.rpm # aarch64.rpm on aarch64
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
