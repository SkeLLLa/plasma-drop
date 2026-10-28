# Security Policy

## Supported versions

Only the latest release gets security fixes. Releases are cut from `master` automatically, so a
fix ships as a new patch release rather than a backport.

## Reporting a vulnerability

Report it privately through
[GitHub private vulnerability reporting](https://github.com/SkeLLLa/plasma-drop/security/advisories/new).
Please don't open a public issue for it.

Include the version (`plasma-drop --version`), how you installed it, and steps to reproduce. You
should get a first answer within 7 days. Once a fix is released, the advisory is published with
credit to you unless you'd rather stay anonymous.

This covers `plasma-drop` itself, its release artifacts, and the APT and DNF repositories on
GitHub Pages, including the release workflow and the package signing key.

## Verifying what you downloaded

Every release artifact has signed build provenance tying it to this repository, the workflow run
that built it, and the commit it was built from (the tagged commit; for v1.14.2 and earlier, the
commit that release's run was started for, usually the one just before the version bump).
The crate on crates.io is the attested `.crate` attached to the release, for releases that carry
one. Releases are published only after every check passes. A release marked Immutable on its release
page can't be changed after it is published. See [docs/verifying.md](docs/verifying.md).
