# Verifying a Release

Every file on the [releases page](https://github.com/SkeLLLa/plasma-drop/releases) is built by
[`release.yml`](../.github/workflows/release.yml) on a GitHub-hosted runner, never on a
maintainer's machine. Each release carries several independent proofs of that; pick the one that
matches how you installed.

| You installed with | Check |
| --- | --- |
| A tarball, `.deb` or `.rpm` from the releases page | [Build provenance](#build-provenance) |
| `packslip` or `mise use packslip:…` | Done for you: [packslip manifest](#packslip-manifest) |
| The APT or DNF repository | Done for you: [package signatures](#package-signatures) |
| `cargo install` | [crates.io](#cratesio) |

All attestations are listed publicly at
<https://github.com/SkeLLLa/plasma-drop/attestations>.

## Build provenance

The `checksums` job signs a [SLSA](https://slsa.dev/spec/v1.0/provenance) build provenance
attestation for every tarball, `.deb` and `.rpm` with
[GitHub artifact attestations](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations).
The signature is a short-lived [Sigstore](https://www.sigstore.dev/) certificate issued to the
workflow run itself, so there is no long-lived key that could leak.

With the [GitHub CLI](https://cli.github.com/) (2.49 or later):

```sh
gh attestation verify plasma-drop-<version>-x86_64-unknown-linux-gnu.tar.gz \
  --repo SkeLLLa/plasma-drop \
  --signer-workflow SkeLLLa/plasma-drop/.github/workflows/release.yml
```

A passing check proves that the file is byte-for-byte what the release workflow of
`SkeLLLa/plasma-drop` built and that it was built on a GitHub-hosted runner. Add
`--format json` to see the details recorded in the signing certificate:

| Field | Meaning |
| --- | --- |
| `sourceRepositoryURI` | The repository the workflow ran in |
| `sourceRepositoryDigest` | The commit the run was started for |
| `buildSignerURI` | The workflow file and ref that signed it |
| `runInvocationURI` | The Actions run, with its full build log |

```sh
gh attestation verify plasma-drop_<version>_amd64.deb --repo SkeLLLa/plasma-drop --format json \
  --jq '.[0].verificationResult.signature.certificate
        | {sourceRepositoryURI, sourceRepositoryDigest, buildSignerURI, runInvocationURI}'
```

Open `runInvocationURI` to read the log of the exact run that produced your file.

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

The `--pin` fingerprint identifies this repository's release workflow and is the same for every
release. Compare it against the [README](../README.md#mise-packslip-or-cargo), not against a value
you were sent.

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

The crate is published from the same workflow with crates.io
[trusted publishing](https://crates.io/docs/trusted-publishing): there is no stored registry
token, only a short-lived one issued to the release workflow. `cargo install --locked` then builds
from that published source on your machine, with the committed `Cargo.lock`.

## How the pipeline is protected

- Every action is pinned to a commit SHA and every container image to a digest; CI and the release
  `pin-gate` job refuse anything unpinned. `zizmor` audits the workflows.
- Jobs start with no permissions and ask only for what they need. Build jobs are read-only; the
  jobs that sign never build, and the jobs that build never hold a signing key.
- Dependencies are audited with `cargo deny`, and Dependabot proposes updates only after a 7-day
  cooldown.
- [OpenSSF Scorecard](https://scorecard.dev/viewer/?uri=github.com/SkeLLLa/plasma-drop) checks
  these practices weekly.

Report security problems as described in [SECURITY.md](../SECURITY.md).
