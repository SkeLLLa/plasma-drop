#!/usr/bin/env bash
# Release helpers for release-prepare.yml and release.yml, run from the repository root. Every
# command fails closed: a network or lookup error is a non-zero exit, never an empty answer.
#
# Usage:
#   .ci/release.sh version
#       print the package version from Cargo.toml
#   .ci/release.sh assets VERSION
#       print the 7 files the build produces, one per line: 2 tarballs, 2 .deb, 2 .rpm and
#       the crate plasma-drop-VERSION.crate
#   .ci/release.sh release-files VERSION
#       print the 17 files a release carries: the assets, their .sha256 sidecars, SHA256SUMS,
#       packslip.sigstore.json and the provenance bundle plasma-drop-VERSION.intoto.jsonl
#   .ci/release.sh tag-commit TAG
#       print the commit TAG points to on GitHub (peeled), or an empty line if there is no TAG
#   .ci/release.sh assert-tag TAG SHA
#       fail unless TAG points to commit SHA
#   .ci/release.sh make-latest VERSION
#       read the published, non-prerelease tag names on stdin; print true if VERSION should be
#       marked the latest release (it is not a prerelease and no published vX.Y.Z is higher)
#   .ci/release.sh newer-release VERSION
#       read the published, non-prerelease tag names on stdin; print the highest vX.Y.Z above
#       VERSION (a prerelease is below its release), or nothing
#   .ci/release.sh crate-published VERSION
#       exit 0 if crates.io has plasma-drop VERSION, 1 if it has not, 2 on any other answer
#   .ci/release.sh crate-check VERSION FILE SHA RUN_ID
#       fail (1) unless crates.io's VERSION has the sha256 of FILE and was trusted-published
#       from GitHub repository GITHUB_REPOSITORY at commit SHA by run RUN_ID; 2 on a lookup error
#   .ci/release.sh provenance-badge VERSION
#       print the shields.io endpoint JSON "provenance: verified vVERSION"
#   .ci/release.sh changelog-notes VERSION
#       read CHANGELOG.md on stdin; print the body of VERSION's section
#   .ci/release.sh release-state TAG
#       print none, draft or published: the GitHub release of TAG. Exit 2 on a lookup error, a
#       bad TAG or more than one release for TAG
#   .ci/release.sh check-release TAG VERSION DIR
#       download TAG's release and fail unless it holds exactly the release files of VERSION,
#       each byte for byte equal to DIR/<name>, and SHA256SUMS checks
#   .ci/release.sh drop-changelog-section VERSION
#       read CHANGELOG.md on stdin; print it without VERSION's section
#   .ci/release.sh bump-base VERSION
#       if VERSION was set by release-prepare.yml's bump commit, print the last released
#       version (the nearest vX.Y.Z tag before that commit); print an empty line if a person
#       set VERSION. Needs the history and tags (fetch-depth: 0)
#   .ci/release.sh set-version VERSION
#       set the package version in Cargo.toml and Cargo.lock
set -euo pipefail

CRATE=plasma-drop
SEMVER='^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'
BUMP_SUBJECT='chore(release): v%s [skip ci]'
UA="${CRATE}-release (github.com/SkeLLLa/plasma-drop)"

repo_url() { printf 'https://github.com/%s.git' "${GITHUB_REPOSITORY:?}"; }

assets() {
    local v="$1"
    printf '%s\n' \
        "${CRATE}-${v}-x86_64-unknown-linux-gnu.tar.gz" \
        "${CRATE}-${v}-aarch64-unknown-linux-gnu.tar.gz" \
        "${CRATE}_${v}_amd64.deb" \
        "${CRATE}_${v}_arm64.deb" \
        "${CRATE}-${v}-1.x86_64.rpm" \
        "${CRATE}-${v}-1.aarch64.rpm" \
        "${CRATE}-${v}.crate"
}

release_files() {
    local f
    assets "$1" | while IFS= read -r f; do printf '%s\n%s.sha256\n' "${f}" "${f}"; done
    printf '%s\n' SHA256SUMS packslip.sigstore.json "${CRATE}-$1.intoto.jsonl"
}

# Both patterns: ls-remote prints the peeled `^{}` line of an annotated tag only when asked.
tag_commit() {
    local out peeled plain
    out="$(git ls-remote --tags "$(repo_url)" "refs/tags/$1" "refs/tags/$1^{}")"
    peeled="$(awk -v r="refs/tags/$1^{}" '$2 == r { print $1 }' <<<"${out}")"
    plain="$(awk -v r="refs/tags/$1" '$2 == r { print $1 }' <<<"${out}")"
    printf '%s\n' "${peeled:-${plain}}"
}

assert_tag() {
    local got
    got="$(tag_commit "$1")"
    if [ "${got}" != "$2" ]; then
        echo "::error::tag $1 points at '${got:-nothing}', expected $2"
        exit 1
    fi
}

# The highest vX.Y.Z among the tag names on stdin, without the v.
stable_top() { sed -n 's/^v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p' | sort -V | tail -n 1; }

make_latest() {
    local top
    top="$(stable_top)"
    case "$1" in *-*)
        echo false
        return
        ;;
    esac
    if [ -z "${top}" ] || [ "$(printf '%s\n%s\n' "${top}" "$1" | sort -V | tail -n 1)" = "$1" ]; then
        echo true
    else
        echo false
    fi
}

crate_published() {
    local code
    code="$(curl -sS -o /dev/null -w '%{http_code}' --retry 3 \
        -A "${UA}" \
        "https://crates.io/api/v1/crates/${CRATE}/$1")" || exit 2
    case "${code}" in
    200) exit 0 ;;
    404) exit 1 ;;
    *)
        echo "::error::crates.io answered ${code} for ${CRATE} $1"
        exit 2
        ;;
    esac
}

# A published release above VERSION, as pages checks before it deploys: the core of a
# prerelease (2.0.0 for 2.0.0-rc.1) counts as above it.
newer_release() {
    local top
    check_semver "$1"
    top="$(stable_top)"
    if [ -n "${top}" ] && [ "${top}" != "$1" ] &&
        [ "$(printf '%s\n%s\n' "${top}" "${1%%-*}" | sort -V | tail -n 1)" = "${top}" ]; then
        printf 'v%s\n' "${top}"
    fi
}

# The checksum crates.io holds for VERSION (the one cargo checks downloads against) must be the
# attested file's, and trustpub_data must name this repository, commit and run.
crate_check() {
    local json want got pub provider repo sha run
    check_semver "$1"
    [ -s "$2" ] || { echo "::error::$2 is missing" >&2; exit 2; }
    json="$(curl -fsS --retry 3 -A "${UA}" "https://crates.io/api/v1/crates/${CRATE}/$1")" || {
        echo "::error::could not read ${CRATE} $1 from crates.io"
        exit 2
    }
    want="$(sha256sum -- "$2" | cut -d ' ' -f 1)"
    got="$(jq -r '.version.checksum // ""' <<<"${json}")" || exit 2
    if [ "${got}" != "${want}" ]; then
        echo "::error::crates.io has ${CRATE} $1 with sha256 '${got}', not the attested ${want}"
        exit 1
    fi
    pub="$(jq -r '.version.trustpub_data // {} | "\(.provider) \(.repository) \(.sha) \(.run_id)"' <<<"${json}")" || exit 2
    read -r provider repo sha run <<<"${pub}"
    if [ "${provider}" != github ] || [ "${repo,,}" != "${GITHUB_REPOSITORY,,}" ] || [ "${sha}" != "$3" ] || [ "${run}" != "$4" ]; then
        echo "::error::crates.io says ${CRATE} $1 was published by '${pub}', not by run $4 of ${GITHUB_REPOSITORY} at $3"
        exit 1
    fi
}

provenance_badge() {
    check_semver "$1"
    printf '{"schemaVersion":1,"label":"provenance","message":"verified v%s","color":"brightgreen"}\n' "$1"
}

changelog_notes() {
    awk -v h="## [$1]" '
        index($0, "## [") == 1 { if (found) exit; if (index($0, h) == 1) { found = 1; next } }
        found'
}

check_semver() {
    if ! [[ "$1" =~ ${SEMVER} ]]; then
        echo "::error::'$1' is not a version" >&2
        exit 2
    fi
}

# The package version: the first `version` line of a Cargo.toml on stdin.
cargo_version() { awk -F '"' '/^version/ { print $2; exit }'; }

# Lists instead of releases/tags/TAG, which answers 404 for a draft. TAG is checked before it
# goes into the jq filter.
release_state() {
    local out n
    check_semver "${1#v}"
    case "$1" in v*) ;; *)
        echo "::error::'$1' is not a vX.Y.Z tag" >&2
        exit 2
        ;;
    esac
    out="$(gh api --paginate "repos/${GITHUB_REPOSITORY:?}/releases?per_page=100" \
        --jq ".[] | select(.tag_name == \"$1\") | .draft")" || {
        echo "::error::could not list the releases of ${GITHUB_REPOSITORY}" >&2
        exit 2
    }
    n="$(printf '%s' "${out}" | grep -c . || true)"
    case "${n}:${out}" in
    0:) echo none ;;
    1:true) echo draft ;;
    1:false) echo published ;;
    *)
        echo "::error::expected at most one release for $1, found: ${out//$'\n'/ }" >&2
        exit 2
        ;;
    esac
}

check_release() {
    local dl f rc=0
    dl="$(mktemp -d)"
    gh release download "$1" --repo "${GITHUB_REPOSITORY:?}" --dir "${dl}" || rc=1
    if [ "${rc}" -eq 0 ] &&
        ! diff -u <(release_files "$2" | sort) <(cd "${dl}" && find . -type f | sed 's|^\./||' | sort); then
        echo "::error::$1 does not hold exactly the release files of $2"
        rc=1
    fi
    if [ "${rc}" -eq 0 ]; then
        while IFS= read -r f; do
            cmp -- "${dl}/${f}" "$3/${f}" || rc=1
        done < <(release_files "$2")
    fi
    if [ "${rc}" -eq 0 ]; then
        (cd "${dl}" && sha256sum -c --quiet SHA256SUMS) || rc=1
    fi
    rm -rf "${dl}"
    [ "${rc}" -eq 0 ] || echo "::error::$1 differs from the files this run verified"
    return "${rc}"
}

# A section runs from its `## [VERSION]` line to the next level 1 or 2 heading: release-plz
# writes `## [X.Y.Z](link) - date` sections, and older files repeat the `# Changelog` header
# after the last one.
drop_changelog_section() {
    awk -v h="## [$1]" '
        index($0, "# ") == 1 || index($0, "## ") == 1 { skip = (index($0, h) == 1) }
        !skip'
}

# The first-parent commit that set the current version decides: a bump commit means release-plz
# set it, so the base is the last release; anything else is a person's choice and is kept.
bump_base() {
    local c prev tag
    while IFS= read -r c; do
        # Empty when c is the first commit or the one that added Cargo.toml.
        prev="$(git show "${c}^:Cargo.toml" 2>/dev/null | cargo_version || true)"
        [ "${prev}" = "$1" ] && continue
        # shellcheck disable=SC2059 # BUMP_SUBJECT is the format
        if [ "$(git log -1 --format=%s "${c}")" != "$(printf "${BUMP_SUBJECT}" "$1")" ]; then
            echo
            return
        fi
        tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "${c}^" 2>/dev/null || true)"
        prev="${tag:+${tag#v}}"
        prev="${prev:-$(git show "${c}^:Cargo.toml" 2>/dev/null | cargo_version || true)}"
        if [ "${prev}" = "$1" ]; then echo; else printf '%s\n' "${prev}"; fi
        return
    done < <(git rev-list --first-parent HEAD -- Cargo.toml)
    echo
}

# rewrite FILE VERSION AWK_PROGRAM: write FILE.new, FILE rewritten by the program (VERSION is
# `v` in it). Fails unless exactly one line changed.
rewrite() {
    local n
    awk -v v="$2" "$3" "$1" >"$1.new" || return 1
    n="$(diff "$1" "$1.new" | grep -c '^>' || true)"
    if [ "${n}" -ne 1 ]; then
        echo "::error::expected to change one version line in $1, changed ${n}" >&2
        return 1
    fi
}

# Both files or neither.
set_version() {
    check_semver "$1"
    # shellcheck disable=SC2016 # awk programs
    if ! rewrite Cargo.toml "$1" '!done && /^version = "/ { print "version = \"" v "\""; done = 1; next } { print }' ||
        ! rewrite Cargo.lock "$1" 'pkg && /^version = "/ { print "version = \"" v "\""; pkg = 0; next }
            { pkg = ($0 == "name = \"plasma-drop\""); print }'; then
        rm -f Cargo.toml.new Cargo.lock.new
        exit 1
    fi
    mv Cargo.toml.new Cargo.toml
    mv Cargo.lock.new Cargo.lock
}

usage() {
    echo "usage: release.sh version|assets|release-files|tag-commit|assert-tag|make-latest|newer-release|crate-published|crate-check|provenance-badge|changelog-notes|release-state|check-release|drop-changelog-section|bump-base|set-version [ARGS]" >&2
    exit 2
}

cmd="${1:-}"
[ $# -gt 0 ] && shift
case "${cmd}" in
version) cargo_version <Cargo.toml ;;
assets) assets "${1:?VERSION}" ;;
release-files) release_files "${1:?VERSION}" ;;
tag-commit) tag_commit "${1:?TAG}" ;;
assert-tag) assert_tag "${1:?TAG}" "${2:?SHA}" ;;
make-latest) make_latest "${1:?VERSION}" ;;
newer-release) newer_release "${1:?VERSION}" ;;
crate-published) crate_published "${1:?VERSION}" ;;
crate-check) crate_check "${1:?VERSION}" "${2:?FILE}" "${3:?SHA}" "${4:?RUN_ID}" ;;
provenance-badge) provenance_badge "${1:?VERSION}" ;;
changelog-notes) changelog_notes "${1:?VERSION}" ;;
release-state) release_state "${1:?TAG}" ;;
check-release) check_release "${1:?TAG}" "${2:?VERSION}" "${3:?DIR}" ;;
drop-changelog-section)
    check_semver "${1:?VERSION}"
    drop_changelog_section "$1"
    ;;
bump-base)
    check_semver "${1:?VERSION}"
    bump_base "$1"
    ;;
set-version) set_version "${1:?VERSION}" ;;
*) usage ;;
esac
