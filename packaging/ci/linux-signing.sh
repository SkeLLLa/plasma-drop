#!/bin/sh
# The OpenPGP key that signs the Linux packages and the APT and DNF repositories, held to the
# public key committed as packaging/signing/repo.asc (the pin). Once the pin is committed, every
# release must be signed with that key: import fails without the secret or with any other key.
# The pin is the primary key, which stays offline; CI holds one signing subkey of it.
#
# Usage:
#   packaging/ci/linux-signing.sh import
#       import REPO_SIGNING_KEY (an unprotected armored secret subkey, from ci-key) into
#       GNUPGHOME, which must be an empty directory, after checking it against the pin, and
#       merge the pin itself, so a subkey it revokes can't sign. With neither the pin nor the
#       key it imports nothing and the release stays unsigned; with only one of them it fails.
#       Writes LINUX_SIGNED=true|false to GITHUB_ENV when that is set.
#   packaging/ci/linux-signing.sh ci-key SUBKEY
#       on the maintainer's machine: print the signing subkey SUBKEY (a fingerprint) of the
#       offline key in GNUPGHOME (default ~/.gnupg) as REPO_SIGNING_KEY wants it, without the
#       primary's secret and without a passphrase. Reads the passphrase from the terminal.
#   packaging/ci/linux-signing.sh sign-rpm FILE...
#       sign each .rpm with the imported key, then check-rpm them
#   packaging/ci/linux-signing.sh check-rpm FILE...
#       fail unless each .rpm carries a valid signature by the pinned key (needs only rpmkeys)
set -eu

PIN="$(cd "$(dirname "$0")/../.." && pwd)/packaging/signing/repo.asc"

die() {
    echo "error: $*" >&2
    exit 1
}

gh_env() { if [ -n "${GITHUB_ENV:-}" ]; then echo "$1" >> "$GITHUB_ENV"; fi; }

# The primary key fingerprint of the pin; exactly one key.
pin_fpr() {
    keys="$(gpg --batch --with-colons --show-keys "$PIN" | grep -c '^pub:' || true)"
    [ "$keys" -eq 1 ] || die "$PIN must hold exactly one public key, not $keys"
    gpg --batch --with-colons --show-keys "$PIN" | awk -F: '$1 == "fpr" { print $10; exit }'
}

# The fingerprint of GNUPGHOME's only secret key, which must be the pinned one.
secret_fpr() {
    keys="$(gpg --batch --with-colons --list-secret-keys 2> /dev/null | grep -c '^sec:' || true)"
    [ "$keys" -eq 1 ] || die "gpg holds $keys secret keys; the signing key must be the only one"
    fpr="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')"
    [ "$fpr" = "$(pin_fpr)" ] || die "the signing key $fpr is not the one pinned in $PIN"
    echo "$fpr"
}

import() {
    if [ -z "${GNUPGHOME:-}" ] || [ ! -d "$GNUPGHOME" ]; then
        die "GNUPGHOME must name an existing directory"
    fi
    [ -z "$(ls -A "$GNUPGHOME")" ] || die "GNUPGHOME $GNUPGHOME is not empty"
    key="${REPO_SIGNING_KEY:-}"
    if [ ! -f "$PIN" ]; then
        [ -z "$key" ] || die "REPO_SIGNING_KEY is set but $PIN is missing; commit the public key"
        echo "$PIN is not committed: the Linux packages and repositories are unsigned" >&2
        gh_env LINUX_SIGNED=false
        return
    fi
    [ -n "$key" ] || die "$PIN is committed but the linux-signing environment has no REPO_SIGNING_KEY"
    printf '%s\n' "$key" | gpg --batch --quiet --import
    fpr="$(secret_fpr)"
    # A subkey added since the pin was exported is refused until the pin that lists it is committed.
    pinned="$(gpg --batch --with-colons --show-keys "$PIN" | awk -F: '$1 == "sub" { s = 1 } $1 == "fpr" && s { print $10; s = 0 }')"
    for sub in $(gpg --batch --with-colons --with-subkey-fingerprints --list-secret-keys |
        awk -F: '$1 == "ssb" { s = 1 } $1 == "fpr" && s { print $10; s = 0 }'); do
        echo "$pinned" | grep -qx "$sub" || die "the signing subkey $sub is not in $PIN; commit the exported public key"
    done
    # The committed certificate carries every subkey and revocation; it is what gets published.
    gpg --batch --quiet --import "$PIN"
    # Fails on a passphrase or an expired key, before anything is signed with it.
    echo probe | gpg --batch --quiet --pinentry-mode loopback --passphrase '' --local-user "$fpr" \
        --sign > /dev/null || die "the signing key can't sign (passphrase-protected or expired?)"
    echo "imported the signing key $fpr" >&2
    gh_env LINUX_SIGNED=true
}

sign_rpm() {
    command -v rpmsign > /dev/null || die "rpmsign is not installed"
    fpr="$(secret_fpr)"
    # __gpg: Debian's rpm looks for gpg2. An already identical signature is left as it is.
    rpmsign --define "__gpg $(command -v gpg)" --define "_gpg_name $fpr" \
        --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback" --addsign "$@"
    check_rpm "$@"
}

ci_key() {
    sub="$1"
    printf 'passphrase of %s: ' "$sub" >&2
    if [ -t 0 ]; then
        stty -echo
        read -r pw || true
        stty echo
        echo >&2
    else
        read -r pw || true
    fi
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    # The trailing ! exports this subkey alone; the primary's secret becomes a stub.
    printf '%s\n' "$pw" | gpg --batch --pinentry-mode loopback --passphrase-fd 0 --armor \
        --export-secret-subkeys "$sub!" | GNUPGHOME="$tmp" gpg --batch --quiet --import
    # Old passphrase, then an empty new one; gpg exits non-zero over the primary's stub.
    printf '%s\n\n\n' "$pw" | GNUPGHOME="$tmp" gpg --pinentry-mode loopback --command-fd 0 \
        --passwd "$sub" > /dev/null 2>&1 || true
    echo probe | GNUPGHOME="$tmp" gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
        --local-user "$sub!" --sign > /dev/null 2>&1 ||
        die "couldn't export $sub without a passphrase (wrong passphrase, or not a signing subkey?)"
    GNUPGHOME="$tmp" gpg --batch --armor --export-secret-subkeys "$sub!"
}

check_rpm() {
    command -v rpmkeys > /dev/null || die "rpmkeys is not installed"
    [ -f "$PIN" ] || die "$PIN is missing"
    db="$(mktemp -d)"
    trap 'rm -rf "$db"' EXIT
    rpmkeys --dbpath "$db" --import "$PIN"
    for f in "$@"; do
        # signature level: an unsigned package fails instead of passing on its digests.
        rpmkeys --dbpath "$db" --define '_pkgverify_level signature' --checksig "$f" ||
            die "$f is not signed by the key pinned in $PIN"
    done
}

[ "$#" -ge 1 ] || die "usage: $0 import | ci-key SUBKEY | sign-rpm FILE... | check-rpm FILE..."
cmd="$1"
shift
case "$cmd" in
    import) import ;;
    ci-key)
        [ "$#" -eq 1 ] || die "usage: $0 ci-key SUBKEY"
        ci_key "$1"
        ;;
    sign-rpm | check-rpm)
        [ "$#" -ge 1 ] || die "$cmd needs at least one .rpm"
        for f in "$@"; do [ -f "$f" ] || die "$f is not a file"; done
        if [ "$cmd" = sign-rpm ]; then sign_rpm "$@"; else check_rpm "$@"; fi
        ;;
    *) die "unknown command '$cmd'" ;;
esac
