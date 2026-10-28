# Verifying a Release

Every file on the [releases page](https://github.com/SkeLLLa/plasma-drop/releases) is built by
[`release.yml`](../.github/workflows/release.yml) on a GitHub-hosted runner, never on a
maintainer's machine. `release.yml` is started by
[`release-prepare.yml`](../.github/workflows/release-prepare.yml) after each version bump, always
on `master`. Each release carries several independent proofs of that; pick the one that
matches how you installed.

| You installed with | Check |
| --- | --- |
| A tarball, `.deb` or `.rpm` from the releases page | [Build provenance](#build-provenance) |
| `packslip` or `mise use packslip:…` | Done for you: [packslip manifest](#packslip-manifest) |
| The APT or DNF repository | Done for you: [package signatures](#package-signatures) |
| `cargo install` | [crates.io](#cratesio) |
| Any file from an immutable release | [Release integrity](#release-integrity-immutable-releases) |

All attestations are listed publicly at
<https://github.com/SkeLLLa/plasma-drop/attestations>.

## Build provenance

The `checksums` job signs a [SLSA](https://slsa.dev/spec/v1.0/provenance) build provenance
attestation for every tarball, `.deb` and `.rpm` with
[GitHub artifact attestations](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations).
The signature is a short-lived [Sigstore](https://www.sigstore.dev/) certificate issued to the
workflow run itself, so there is no long-lived key that could leak.

With the [GitHub CLI](https://cli.github.com/) (2.68 or later):

```sh
gh attestation verify plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --repo SkeLLLa/plasma-drop \
  --signer-workflow SkeLLLa/plasma-drop/.github/workflows/release.yml \
  --source-ref refs/heads/master
```

A passing check proves that the file is byte-for-byte what the release workflow of
`SkeLLLa/plasma-drop` built, on `master`, on a GitHub-hosted runner. Keep `--source-ref`: without
it, a run of `release.yml` on any branch would pass. Add `--format json` to see the details
recorded in the signing certificate:

| Field | Meaning |
| --- | --- |
| `sourceRepositoryURI` | The repository the workflow ran in |
| `sourceRepositoryDigest` | The commit the run was started for (see below) |
| `buildTrigger` | The event that started the run |
| `buildSignerURI` | The workflow file and ref that signed it |
| `runInvocationURI` | The Actions run, with its full build log |

```sh
gh attestation verify plasma-drop_<version>_amd64.deb --repo SkeLLLa/plasma-drop --format json \
  --jq '.[0].verificationResult.signature.certificate
        | {sourceRepositoryURI, sourceRepositoryDigest, buildTrigger, buildSignerURI,
           runInvocationURI}'
```

Open `runInvocationURI` to read the log of the exact run that produced your file.

When `buildTrigger` is `workflow_dispatch`, `sourceRepositoryDigest` is the commit the
`v<version>` tag points to. Check that as part of the verification:

```sh
gh attestation verify plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --repo SkeLLLa/plasma-drop \
  --signer-workflow SkeLLLa/plasma-drop/.github/workflows/release.yml \
  --source-ref refs/heads/master \
  --source-digest "$(gh api repos/SkeLLLa/plasma-drop/commits/v<version> --jq .sha)"
```

When it is `push` (v1.14.2 and earlier), `sourceRepositoryDigest` is the commit that run was
started for, usually the parent of the tagged version-bump commit, but not always (v1.1.0's tag
is not on a bump commit). Don't derive it from the tag: read it from the attestation above and
check that the tag's commit contains it (`git merge-base --is-ancestor <digest> v<version>`).

### Using the bundle attached to the release

Releases built by the current pipeline also carry `plasma-drop-<version>.intoto.jsonl`, the
signed provenance of all six files. With it, `gh` does not need GitHub's attestation API:

```sh
gh attestation verify plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --bundle plasma-drop-<version>.intoto.jsonl \
  --repo SkeLLLa/plasma-drop \
  --signer-workflow SkeLLLa/plasma-drop/.github/workflows/release.yml \
  --source-ref refs/heads/master
```

This is not an offline check: `gh` still fetches Sigstore's trust root unless you pass
`--custom-trusted-root`. Earlier releases have no bundle; use the command above for them.

It does not prove the code is free of bugs or malice: it proves where the binary came from, so
that what you run matches the public source and build, and that nobody swapped it afterwards.

## Checksums

`SHA256SUMS` and the per-file `.sha256` sidecars catch a corrupted download:

```sh
sha256sum -c --ignore-missing SHA256SUMS
```

They sit next to the files they describe, so anyone able to replace a file could replace its
checksum too. Use build provenance to check where a file came from.

## packslip manifest

`packslip.sigstore.json` is a Sigstore-signed manifest of both tarballs, recording the tagged
commit. `packslip install` and the mise `packslip:` backend check it before installing. To check
downloaded tarballs by hand:

```sh
packslip verify packslip.sigstore.json \
  --identity-prefix https://github.com/SkeLLLa/plasma-drop/ \
  --issuer https://token.actions.githubusercontent.com \
  --pin ps1_4yd5vxao3lfzaox72lynxuotke \
  --artifact plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz
```

The `--pin` fingerprint identifies this repository (its GitHub repository ID and the GitHub
Actions OIDC issuer), not a workflow, and is the same for every release. packslip and mise also
remember which workflow signed the last release you installed (`.github/workflows/release.yml`)
and refuse a different one without your approval. Compare the pin against the
[README](../README.md#mise-packslip-or-cargo), not against a value you were sent.

## Release integrity (immutable releases)

Releases marked **Immutable** on their release page cannot have their assets or tag changed after
publishing. GitHub signs a release attestation for them, which the GitHub CLI checks.
`gh release verify` and `gh release verify-asset` need gh 2.75 or later, and work only on
releases marked Immutable:

```sh
gh release verify v<version> --repo SkeLLLa/plasma-drop
gh release verify-asset v<version> plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --repo SkeLLLa/plasma-drop
```

This only works for releases with the Immutable badge; earlier releases were published before the
setting was enabled. It proves the file is what was published in that release; build provenance
proves how it was built.

## Package signatures

The `.rpm` files and the APT and DNF repository metadata are signed with the plasma-drop
OpenPGP key ([`packaging/signing/repo.asc`](../packaging/signing/repo.asc)). APT and DNF check
these on every install and upgrade. For a `.rpm` from the releases page:

```sh
sudo rpm --import https://skellla.github.io/plasma-drop/plasma-drop.asc
rpm -K plasma-drop-<version>-1.x86_64.rpm
```

`.deb` files carry no signature of their own (APT checks the signed repository index instead), so
verify a downloaded `.deb` with [build provenance](#build-provenance). Key handling is described in
[distribution.md](distribution.md#linux-signing).

## crates.io

The crate is published from the same workflow, after all release checks pass, with crates.io
[trusted publishing](https://crates.io/docs/trusted-publishing): there is no stored registry
token, only a short-lived one issued to the release workflow. `cargo install --locked` then builds
from that published source on your machine, with the committed `Cargo.lock`.

## How the pipeline is protected

- Every action is pinned to a commit SHA and every container image to a digest; CI and the release
  `pin-gate` job refuse anything unpinned. `zizmor` audits the workflows.
- Jobs start with no permissions and ask only for what they need. Build jobs are read-only; the
  jobs that sign never build, and the jobs that build never hold a signing key.
- Nothing is published until every check passes: the release stays a draft until then, and
  can't be changed once published if it is marked Immutable on its release page.
- The committed `release.yml` refuses any ref but `master`. Check this with
  `--source-ref refs/heads/master`, which rejects a build from any other branch.
- Dependencies are audited with `cargo deny`, and Dependabot proposes updates only after a 7-day
  cooldown.
- [OpenSSF Scorecard](https://scorecard.dev/viewer/?uri=github.com/SkeLLLa/plasma-drop) checks
  these practices weekly.

Report security problems as described in [SECURITY.md](../SECURITY.md).
