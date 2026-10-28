#!/usr/bin/env bats
# Tests for .ci/release.sh. gh, curl and the remote git calls are replaced by shims, so
# nothing goes to the network; bump-base runs real git in a throwaway repository. Run with
# `bats .ci/release.bats` (or `make lint-actions`).
# shellcheck disable=SC2030,SC2031 # Helpers check the status and output that run set in the same test.

bats_require_minimum_version 1.5.0

setup() {
    script="${BATS_TEST_DIRNAME}/release.sh"
    mkdir "${BATS_TEST_TMPDIR}/bin"
    PATH="${BATS_TEST_TMPDIR}/bin:${PATH}"
    export GITHUB_REPOSITORY=SkeLLLa/plasma-drop
    export TMPDIR="${BATS_TEST_TMPDIR}"
}

# shim NAME EXIT_CODE OUTPUT: a NAME on PATH that prints OUTPUT and exits with EXIT_CODE.
shim() {
    printf '%s\n' "$3" >"${BATS_TEST_TMPDIR}/$1.out"
    # shellcheck disable=SC2016 # The shim's own variables, expanded when it runs.
    printf '#!/bin/sh\necho "$*" >"%s/%s.args"\ncat "%s/%s.out"\nexit %s\n' \
        "${BATS_TEST_TMPDIR}" "$1" "${BATS_TEST_TMPDIR}" "$1" "$2" >"${BATS_TEST_TMPDIR}/bin/$1"
    chmod +x "${BATS_TEST_TMPDIR}/bin/$1"
}

latest() {
    run "${script}" make-latest "$1" <<<"$2"
    [ "${status}" -eq 0 ]
    [ "${output}" = "$3" ] || { echo "make-latest $1 gave ${output}, want $3"; return 1; }
}

newer() {
    run "${script}" newer-release "$1" <<<"$2"
    [ "${status}" -eq 0 ]
    [ "${output}" = "$3" ] || { echo "newer-release $1 gave '${output}', want '$3'"; return 1; }
}

@test "version reads Cargo.toml" {
    cd "${BATS_TEST_TMPDIR}"
    printf '[package]\nname = "plasma-drop"\nversion = "1.15.0"\n\n[dependencies]\nversion = "9"\n' >Cargo.toml
    run "${script}" version
    [ "${status}" -eq 0 ]
    [ "${output}" = "1.15.0" ]
}

@test "assets: the 7 built files" {
    run "${script}" assets 1.15.0
    [ "${status}" -eq 0 ]
    [ "${#lines[@]}" -eq 7 ]
    [ "${lines[0]}" = "plasma-drop-1.15.0-x86_64-unknown-linux-gnu.tar.gz" ]
    [ "${lines[3]}" = "plasma-drop_1.15.0_arm64.deb" ]
    [ "${lines[5]}" = "plasma-drop-1.15.0-1.aarch64.rpm" ]
    [ "${lines[6]}" = "plasma-drop-1.15.0.crate" ]
}

@test "release-files: 17 unique files with sidecars, sums, packslip and provenance" {
    run "${script}" release-files 1.15.0
    [ "${status}" -eq 0 ]
    [ "${#lines[@]}" -eq 17 ]
    [ "$(printf '%s\n' "${lines[@]}" | sort -u | wc -l)" -eq 17 ]
    [ "$(printf '%s\n' "${lines[@]}" | grep -c '\.sha256$')" -eq 7 ]
    printf '%s\n' "${lines[@]}" | grep -qxF SHA256SUMS
    printf '%s\n' "${lines[@]}" | grep -qxF packslip.sigstore.json
    printf '%s\n' "${lines[@]}" | grep -qxF plasma-drop-1.15.0.intoto.jsonl
    printf '%s\n' "${lines[@]}" | grep -qxF plasma-drop_1.15.0_amd64.deb.sha256
    printf '%s\n' "${lines[@]}" | grep -qxF plasma-drop-1.15.0.crate
    printf '%s\n' "${lines[@]}" | grep -qxF plasma-drop-1.15.0.crate.sha256
}

@test "missing argument or unknown command fails" {
    run "${script}" assets
    [ "${status}" -ne 0 ]
    run "${script}" nope
    [ "${status}" -eq 2 ]
    run "${script}"
    [ "${status}" -eq 2 ]
}

@test "make-latest: first release" {
    latest 1.15.0 "" true
}
@test "make-latest: newer than every published release" {
    latest 1.15.0 "$(printf 'v1.14.2\nv1.9.0\nv1.14.10\n')" true
}
@test "make-latest: older than a published release" {
    latest 1.15.0 "$(printf 'v1.16.0\nv1.14.2\n')" false
}
@test "make-latest: compares numerically, not as text" {
    latest 1.10.0 "$(printf 'v1.9.0\nv1.2.0\n')" true
    latest 1.9.1 "$(printf 'v1.10.0\n')" false
}
@test "make-latest: a prerelease is never latest" {
    latest 1.15.0-rc.1 "$(printf 'v1.14.2\n')" false
    latest 1.15.0-rc.1 "" false
}
@test "make-latest: ignores tags that are not vX.Y.Z" {
    latest 1.15.0 "$(printf 'v2.0\nv9.0.0-rc.1\nnightly\nv10.0.0x\nfoo-v99.0.0\nv1.14.2\n')" true
}

@test "newer-release: nothing published, or nothing newer" {
    newer 1.15.0 "" ""
    newer 1.15.0 "$(printf 'v1.14.2\n')" ""
    newer 1.15.0 "$(printf 'v1.15.0\n')" ""
}
@test "newer-release: a newer release" {
    newer 1.15.0 "$(printf 'v1.14.2\nv1.16.0\n')" v1.16.0
}
@test "newer-release: compares numerically, not as text" {
    newer 1.9.1 "$(printf 'v1.10.0\n')" v1.10.0
    newer 1.10.0 "$(printf 'v1.9.0\n')" ""
}
@test "newer-release: a prerelease is below its release" {
    newer 2.0.0-rc.1 "$(printf 'v2.0.0\n')" v2.0.0
    newer 2.0.0-rc.1 "$(printf 'v1.9.0\n')" ""
}
@test "newer-release: ignores tags that are not vX.Y.Z" {
    newer 1.15.0 "$(printf 'v9.0.0-rc.1\nnightly\nfoo-v99.0.0\n')" ""
}
@test "newer-release: a bad version fails" {
    run "${script}" newer-release v1.15.0 <<<""
    [ "${status}" -eq 2 ]
}

@test "provenance-badge: the shields.io endpoint JSON" {
    run "${script}" provenance-badge 1.15.0
    [ "${status}" -eq 0 ]
    [ "${output}" = '{"schemaVersion":1,"label":"provenance","message":"verified v1.15.0","color":"brightgreen"}' ]
    jq -e . <<<"${output}" >/dev/null
    run "${script}" provenance-badge 2.0.0-rc.1
    [ "$(jq -r .message <<<"${output}")" = "verified v2.0.0-rc.1" ]
}
@test "provenance-badge: a bad or missing version fails and prints nothing" {
    for v in '1.15.0"' v1.15.0; do
        run --separate-stderr "${script}" provenance-badge "${v}"
        [ "${status}" -eq 2 ] || { echo "accepted ${v}"; return 1; }
        [ -z "${output}" ]
    done
    # An empty argument stops at ${1:?}, like every command.
    run --separate-stderr "${script}" provenance-badge ''
    [ "${status}" -ne 0 ]
    [ -z "${output}" ]
}

@test "tag-commit: annotated tag gives the peeled commit" {
    shim git 0 "$(printf '%s\trefs/tags/v1.14.1\n%s\trefs/tags/v1.14.1^{}' \
        1d9df815d41b2786a34b5812c1f74a4ff03e853f 60fd8c0dfd92a78a0dcb4feea785b8495e159621)"
    run "${script}" tag-commit v1.14.1
    [ "${status}" -eq 0 ]
    [ "${output}" = "60fd8c0dfd92a78a0dcb4feea785b8495e159621" ]
    grep -qF "ls-remote --tags https://github.com/SkeLLLa/plasma-drop.git refs/tags/v1.14.1 refs/tags/v1.14.1^{}" \
        "${BATS_TEST_TMPDIR}/git.args"
}
@test "tag-commit: lightweight tag gives its commit" {
    shim git 0 "$(printf '%s\trefs/tags/v1.14.1' 60fd8c0dfd92a78a0dcb4feea785b8495e159621)"
    run "${script}" tag-commit v1.14.1
    [ "${status}" -eq 0 ]
    [ "${output}" = "60fd8c0dfd92a78a0dcb4feea785b8495e159621" ]
}
@test "tag-commit: ignores other refs that end the same way" {
    shim git 0 "$(printf '%s\trefs/tags/x/refs/tags/v1.14.1' 60fd8c0dfd92a78a0dcb4feea785b8495e159621)"
    run "${script}" tag-commit v1.14.1
    [ "${status}" -eq 0 ]
    [ -z "${output}" ]
}
@test "tag-commit: absent tag is empty" {
    shim git 0 ""
    run "${script}" tag-commit v9.9.9
    [ "${status}" -eq 0 ]
    [ -z "${output}" ]
}
@test "tag-commit: a git error fails" {
    shim git 128 "fatal: unable to access"
    run "${script}" tag-commit v1.14.1
    [ "${status}" -ne 0 ]
}

@test "assert-tag: matching commit passes" {
    shim git 0 "$(printf '%s\trefs/tags/v1.0.0' aaaa)"
    run "${script}" assert-tag v1.0.0 aaaa
    [ "${status}" -eq 0 ]
}
@test "assert-tag: other commit, absent tag and git error fail" {
    shim git 0 "$(printf '%s\trefs/tags/v1.0.0' aaaa)"
    run "${script}" assert-tag v1.0.0 bbbb
    [ "${status}" -eq 1 ]
    [[ "${output}" == *"points at 'aaaa', expected bbbb"* ]]
    shim git 0 ""
    run "${script}" assert-tag v1.0.0 bbbb
    [ "${status}" -eq 1 ]
    [[ "${output}" == *"points at 'nothing'"* ]]
    shim git 128 ""
    run "${script}" assert-tag v1.0.0 aaaa
    [ "${status}" -ne 0 ]
}

@test "crate-published: 200 is 0, 404 is 1, anything else is 2" {
    shim curl 0 200
    run "${script}" crate-published 1.14.2
    [ "${status}" -eq 0 ]
    grep -qF "https://crates.io/api/v1/crates/plasma-drop/1.14.2" "${BATS_TEST_TMPDIR}/curl.args"
    shim curl 0 404
    run "${script}" crate-published 1.14.2
    [ "${status}" -eq 1 ]
    shim curl 0 503
    run "${script}" crate-published 1.14.2
    [ "${status}" -eq 2 ]
    shim curl 6 000
    run "${script}" crate-published 1.14.2
    [ "${status}" -eq 2 ]
}

# --- crate-check --------------------------------------------------------------------------

# file: a .crate and sum, its sha256. crate_json CHECKSUM TRUSTPUB_DATA: a crates.io answer.
crate_fixture() {
    file="${BATS_TEST_TMPDIR}/plasma-drop-1.15.0.crate"
    printf 'crate bytes\n' >"${file}"
    sum="$(sha256sum -- "${file}" | cut -d ' ' -f 1)"
}
crate_json() { printf '{"version":{"num":"1.15.0","checksum":"%s","trustpub_data":%s}}' "$1" "$2"; }
pub='{"provider":"github","repository":"SkeLLLa/plasma-drop","run_id":"42","sha":"abc"}'

@test "crate-check: the attested file, published by this run, passes" {
    crate_fixture
    shim curl 0 "$(crate_json "${sum}" "${pub}")"
    run "${script}" crate-check 1.15.0 "${file}" abc 42
    [ "${status}" -eq 0 ]
    grep -qF "https://crates.io/api/v1/crates/plasma-drop/1.15.0" "${BATS_TEST_TMPDIR}/curl.args"
    grep -qF -- "-A plasma-drop-release" "${BATS_TEST_TMPDIR}/curl.args"
}
@test "crate-check: the repository name is compared case-insensitively" {
    crate_fixture
    shim curl 0 "$(crate_json "${sum}" "${pub/SkeLLLa/skellla}")"
    run "${script}" crate-check 1.15.0 "${file}" abc 42
    [ "${status}" -eq 0 ]
}
@test "crate-check: another checksum fails" {
    crate_fixture
    shim curl 0 "$(crate_json "$(printf '%064d' 0)" "${pub}")"
    run "${script}" crate-check 1.15.0 "${file}" abc 42
    [ "${status}" -eq 1 ]
    [[ "${output}" == *"not the attested"* ]]
}
@test "crate-check: another publisher, commit or run fails" {
    crate_fixture
    for p in null "${pub/github/gitlab}" "${pub/SkeLLLa/other}" "${pub/abc/def}" "${pub/42/43}"; do
        shim curl 0 "$(crate_json "${sum}" "${p}")"
        run "${script}" crate-check 1.15.0 "${file}" abc 42
        [ "${status}" -eq 1 ] || { echo "accepted ${p}"; return 1; }
        [[ "${output}" == *"was published by"* ]]
    done
}
@test "crate-check: a crates.io error is 2, never a pass" {
    crate_fixture
    shim curl 22 ""
    run "${script}" crate-check 1.15.0 "${file}" abc 42
    [ "${status}" -eq 2 ]
    shim curl 0 "<html>"
    run "${script}" crate-check 1.15.0 "${file}" abc 42
    [ "${status}" -ne 0 ]
}
@test "crate-check: a bad version or a missing or empty file fails before curl runs" {
    crate_fixture
    shim curl 0 "$(crate_json "${sum}" "${pub}")"
    rm "${BATS_TEST_TMPDIR}/curl.out"
    : >"${BATS_TEST_TMPDIR}/empty.crate"
    for a in "1.15 ${file}" "1.15.0 ${BATS_TEST_TMPDIR}/none.crate" "1.15.0 ${BATS_TEST_TMPDIR}/empty.crate"; do
        read -r v f <<<"${a}"
        run "${script}" crate-check "${v}" "${f}" abc 42
        [ "${status}" -eq 2 ] || { echo "accepted ${a}"; return 1; }
    done
    [ ! -e "${BATS_TEST_TMPDIR}/curl.args" ]
}

@test "changelog-notes: one version's section, not its neighbours" {
    changelog='# Changelog

## [Unreleased]

## [1.14.10](https://example.invalid/compare/v1.14.9...v1.14.10) - 2026-11-02

### Fixed

- ten

## [1.14.1](https://example.invalid/compare/v1.14.0...v1.14.1) - 2026-10-28

### Other

- *(readme)* add mp4 video demo

## [1.14.0](https://example.invalid/compare/v1.13.2...v1.14.0) - 2026-10-27

### Added

- older'
    run "${script}" changelog-notes 1.14.1 <<<"${changelog}"
    [ "${status}" -eq 0 ]
    [[ "${output}" == *"add mp4 video demo"* ]]
    [[ "${output}" != *"ten"* ]]
    [[ "${output}" != *"older"* ]]
    [[ "${output}" != *"## ["* ]]
    run "${script}" changelog-notes 1.14.10 <<<"${changelog}"
    [[ "${output}" == *"- ten"* ]]
    [[ "${output}" != *"mp4"* ]]
}

# --- release-state ------------------------------------------------------------------------

@test "release-state: none, draft and published" {
    shim gh 0 ""
    run "${script}" release-state v1.15.0
    [ "${status}" -eq 0 ]
    [ "${output}" = none ]
    grep -qF 'repos/SkeLLLa/plasma-drop/releases?per_page=100' "${BATS_TEST_TMPDIR}/gh.args"
    grep -qF -- '--paginate' "${BATS_TEST_TMPDIR}/gh.args"
    grep -qF 'select(.tag_name == "v1.15.0")' "${BATS_TEST_TMPDIR}/gh.args"
    shim gh 0 true
    run "${script}" release-state v1.15.0
    [ "${status}" -eq 0 ]
    [ "${output}" = draft ]
    shim gh 0 false
    run "${script}" release-state v1.15.0-rc.1
    [ "${status}" -eq 0 ]
    [ "${output}" = published ]
}
@test "release-state: two releases for one tag fail" {
    shim gh 0 "$(printf 'true\nfalse')"
    run "${script}" release-state v1.15.0
    [ "${status}" -eq 2 ]
    [[ "${output}" == *"at most one release"* ]]
}
@test "release-state: a gh failure fails, never reads as none" {
    shim gh 1 "HTTP 502"
    run "${script}" release-state v1.15.0
    [ "${status}" -eq 2 ]
    [[ "${output}" != *none* ]]
}
@test "release-state: an unexpected answer fails" {
    shim gh 0 null
    run "${script}" release-state v1.15.0
    [ "${status}" -eq 2 ]
}
@test "release-state: a bad tag fails before gh runs" {
    shim gh 0 ""
    for t in 1.15.0 'v1.15.0") | 1 | ("' v1.15 'v1.15.0 ' x; do
        run "${script}" release-state "${t}"
        [ "${status}" -eq 2 ] || { echo "accepted ${t}"; return 1; }
    done
    [ ! -e "${BATS_TEST_TMPDIR}/gh.args" ]
}

# --- check-release ------------------------------------------------------------------------

# src: the 17 files a run verified. gh "downloads" the release from rel, a copy of them.
release_fixture() {
    src="${BATS_TEST_TMPDIR}/src"
    rel="${BATS_TEST_TMPDIR}/rel"
    mkdir -p "${src}" "${rel}"
    local f
    "${script}" release-files 1.15.0 | grep -vxF SHA256SUMS | while IFS= read -r f; do
        printf 'content of %s\n' "${f}" >"${src}/${f}"
    done
    (cd "${src}" && sha256sum -- * >SHA256SUMS)
    cp "${src}"/* "${rel}/"
    # shellcheck disable=SC2016 # The shim's own variables, expanded when it runs.
    printf '#!/bin/sh\necho "$*" >"%s/gh.args"\n[ "$1 $2" = "release download" ] || exit 9\nwhile [ "$1" != --dir ]; do shift; done\ncp "%s"/* "$2"/\n' \
        "${BATS_TEST_TMPDIR}" "${rel}" >"${BATS_TEST_TMPDIR}/bin/gh"
    chmod +x "${BATS_TEST_TMPDIR}/bin/gh"
}
no_download_dir_left() {
    [ -z "$(find "${BATS_TEST_TMPDIR}" -maxdepth 1 -name 'tmp.*')" ]
}

@test "check-release: identical files pass" {
    release_fixture
    run "${script}" check-release v1.15.0 1.15.0 "${src}"
    [ "${status}" -eq 0 ]
    grep -qF "release download v1.15.0 --repo SkeLLLa/plasma-drop --dir" "${BATS_TEST_TMPDIR}/gh.args"
    no_download_dir_left
}
@test "check-release: a missing file fails" {
    release_fixture
    rm "${rel}/plasma-drop_1.15.0_arm64.deb.sha256"
    run "${script}" check-release v1.15.0 1.15.0 "${src}"
    [ "${status}" -ne 0 ]
    [[ "${output}" == *"does not hold exactly"* ]]
    [[ "${output}" == *"-plasma-drop_1.15.0_arm64.deb.sha256"* ]]
    no_download_dir_left
}
@test "check-release: an extra file fails" {
    release_fixture
    echo x >"${rel}/extra.txt"
    run "${script}" check-release v1.15.0 1.15.0 "${src}"
    [ "${status}" -ne 0 ]
}
@test "check-release: one changed byte fails, even with a matching SHA256SUMS" {
    release_fixture
    printf 'content of plasma-drop-1.15.0-1.x86_64.rpm\r' >"${rel}/plasma-drop-1.15.0-1.x86_64.rpm"
    (cd "${rel}" && rm SHA256SUMS && sha256sum -- * >SHA256SUMS)
    run "${script}" check-release v1.15.0 1.15.0 "${src}"
    [ "${status}" -ne 0 ]
    [[ "${output}" == *"differs from the files this run verified"* ]]
    no_download_dir_left
}
@test "check-release: a SHA256SUMS that does not check fails" {
    release_fixture
    printf '%064d  plasma-drop_1.15.0_amd64.deb\n' 0 >"${src}/SHA256SUMS"
    cp "${src}/SHA256SUMS" "${rel}/SHA256SUMS"
    run "${script}" check-release v1.15.0 1.15.0 "${src}"
    [ "${status}" -ne 0 ]
}
@test "check-release: a failed download fails" {
    release_fixture
    shim gh 1 "HTTP 404"
    run "${script}" check-release v1.15.0 1.15.0 "${src}"
    [ "${status}" -ne 0 ]
    no_download_dir_left
}

# --- drop-changelog-section ---------------------------------------------------------------

# As release-plz writes it, including the second header the real file carries at the end.
changelog() {
    cat <<'EOF'
# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [1.14.10](https://github.com/SkeLLLa/plasma-drop/compare/v1.14.9...v1.14.10) - 2026-11-02

### Fixed

- ten

## [1.14.1](https://github.com/SkeLLLa/plasma-drop/compare/v1.14.0...v1.14.1) - 2026-10-28

### Other

- *(readme)* add mp4 video demo

## [1.0.0] - 2026-01-01

### Added

- first
# Changelog

The format follows Keep a Changelog.
EOF
}
drop() {
    run "${script}" drop-changelog-section "$1" < <(changelog)
    [ "${status}" -eq 0 ] || { echo "exit ${status}: ${output}"; return 1; }
    [ "${output}" = "$(changelog | sed "$2")" ] || { echo "${output}"; return 1; }
}

@test "drop-changelog-section: the newest section, as a bump leaves it" {
    drop 1.14.10 '7,12d'
}
@test "drop-changelog-section: a middle section, not the one it prefixes" {
    drop 1.14.1 '13,18d'
    [[ "${output}" == *"- ten"* ]]
}
@test "drop-changelog-section: the last section keeps the trailing header" {
    drop 1.0.0 '19,23d'
    [[ "${output}" == *"The format follows Keep a Changelog."* ]]
}
@test "drop-changelog-section: an absent version changes nothing" {
    drop 1.14.2 '$!b'
    run "${script}" drop-changelog-section 1.14 < <(changelog)
    [ "${status}" -eq 2 ]
}
@test "drop-changelog-section: Unreleased entries and text that mentions a version stay" {
    run "${script}" drop-changelog-section 1.14.10 <<<'## [Unreleased]

- pending, see ## [1.14.10]

## [1.14.10] - 2026-11-02

### Fixed

- fix
'
    [ "${status}" -eq 0 ]
    [ "${output}" = '## [Unreleased]

- pending, see ## [1.14.10]' ]
}

# --- bump-base ----------------------------------------------------------------------------

# A repository whose last release is v1.0.0 (a tagged bot bump), followed by one more commit.
bump_repo() {
    cd "${BATS_TEST_TMPDIR}" || return
    git init -q repo
    cd repo || return
    git config user.name t
    git config user.email t@example.invalid
    git config commit.gpgsign false
    git config tag.gpgsign false
    set_cargo 0.9.0 "feat: start"
    set_cargo 1.0.0 "chore(release): v1.0.0 [skip ci]"
    git tag v1.0.0
    commit "feat: after the release"
}
# set_cargo VERSION SUBJECT: commit a Cargo.toml with VERSION and a changed dependency version.
set_cargo() {
    printf '[package]\nname = "plasma-drop"\nversion = "%s"\n\n[dependencies.x]\nversion = "%s"\n' \
        "$1" "${RANDOM}" >Cargo.toml
    git add Cargo.toml
    git commit -qm "$2"
}
commit() {
    echo "$1" >>other.txt
    git add other.txt
    git commit -qm "$1"
}
base() {
    run "${script}" bump-base "$1"
    [ "${status}" -eq 0 ] || { echo "exit ${status}: ${output}"; return 1; }
    [ "${output}" = "$2" ] || { echo "bump-base $1 gave '${output}', want '$2'"; return 1; }
}

@test "bump-base: a bot bump gives the last release" {
    bump_repo
    set_cargo 1.1.0 "chore(release): v1.1.0 [skip ci]"
    base 1.1.0 1.0.0
}
@test "bump-base: a dependency change after the bump does not hide it" {
    bump_repo
    set_cargo 1.1.0 "chore(release): v1.1.0 [skip ci]"
    set_cargo 1.1.0 "fix: bump dependency x"
    base 1.1.0 1.0.0
}
@test "bump-base: a redone bump on top of a stale one gives the last release" {
    bump_repo
    set_cargo 1.0.1 "chore(release): v1.0.1 [skip ci]"
    commit "fix: pushed before the release"
    # The redo kept 1.0.1 and only rewrote the changelog.
    commit "chore(release): v1.0.1 [skip ci]"
    commit "fix: pushed again"
    base 1.0.1 1.0.0
}
@test "bump-base: a redo that raised the version gives the last release, not the stale bump" {
    bump_repo
    set_cargo 1.0.1 "chore(release): v1.0.1 [skip ci]"
    commit "feat: pushed before the release"
    set_cargo 1.1.0 "chore(release): v1.1.0 [skip ci]"
    base 1.1.0 1.0.0
}
@test "bump-base: a version a person set is kept" {
    bump_repo
    set_cargo 2.0.0 "feat!: set by hand"
    base 2.0.0 ""
    # Also after the bot rewrote its changelog on top.
    commit "chore(release): v2.0.0 [skip ci]"
    base 2.0.0 ""
}
@test "bump-base: the bot subject on a commit that kept the version is not a bump" {
    bump_repo
    set_cargo 1.1.0 "feat: version set by hand"
    set_cargo 1.1.0 "chore(release): v1.1.0 [skip ci]"
    base 1.1.0 ""
}
@test "bump-base: only the exact bump subject counts" {
    bump_repo
    set_cargo 1.1.0 "chore(release): v1.1.0"
    base 1.1.0 ""
}
@test "bump-base: without a release tag, the version before the bump" {
    bump_repo
    git tag -d v1.0.0 >/dev/null
    set_cargo 1.1.0 "chore(release): v1.1.0 [skip ci]"
    base 1.1.0 1.0.0
}
@test "bump-base: the version of the first commit is kept" {
    cd "${BATS_TEST_TMPDIR}" || return
    git init -q repo
    cd repo || return
    git config user.name t
    git config user.email t@example.invalid
    git config commit.gpgsign false
    set_cargo 0.1.0 "chore(release): v0.1.0 [skip ci]"
    base 0.1.0 ""
}
@test "bump-base: a bad version fails" {
    bump_repo
    run "${script}" bump-base v1.0.0
    [ "${status}" -eq 2 ]
}

# --- set-version --------------------------------------------------------------------------

lockfile() {
    printf '[[package]]\nname = "plasma-cousin"\nversion = "%s"\n\n[[package]]\nname = "plasma-drop"\nversion = "%s"\ndependencies = [\n "anyhow",\n]\n' \
        "$1" "$2"
}

@test "set-version: the package version in Cargo.toml and Cargo.lock, nothing else" {
    cd "${BATS_TEST_TMPDIR}"
    printf '[package]\nname = "plasma-drop"\nversion = "1.15.0"\n\n[dependencies.x]\nversion = "1.15.0"\n' >Cargo.toml
    lockfile 1.15.0 1.15.0 >Cargo.lock
    chmod 644 Cargo.toml Cargo.lock
    run "${script}" set-version 1.14.2
    [ "${status}" -eq 0 ]
    [ "$(cat Cargo.toml)" = "$(printf '[package]\nname = "plasma-drop"\nversion = "1.14.2"\n\n[dependencies.x]\nversion = "1.15.0"')" ]
    [ "$(cat Cargo.lock)" = "$(lockfile 1.15.0 1.14.2)" ]
    [ -z "$(find . -name 'Cargo.*' ! -perm -0644)" ]
    [ -z "$(find . -name 'Cargo.*.*')" ]
}
@test "set-version: a bad version fails" {
    cd "${BATS_TEST_TMPDIR}"
    printf '[package]\nname = "plasma-drop"\nversion = "1.15.0"\n' >Cargo.toml
    lockfile 1.15.0 1.15.0 >Cargo.lock
    for v in v1.0.0 1.0 '1.0.0"' ''; do
        run "${script}" set-version "${v}"
        [ "${status}" -ne 0 ] || { echo "accepted ${v}"; return 1; }
    done
}
@test "set-version: changes neither file unless both change one line" {
    cd "${BATS_TEST_TMPDIR}"
    printf '[package]\nname = "plasma-drop"\nversion = "1.15.0"\n' >Cargo.toml
    lockfile 1.15.0 1.15.0 | sed 's/plasma-drop/other/' >Cargo.lock
    run "${script}" set-version 1.14.2
    [ "${status}" -ne 0 ]
    grep -qxF 'version = "1.15.0"' Cargo.toml
    # Already at that version: the caller expected a change.
    lockfile 1.15.0 1.15.0 >Cargo.lock
    run "${script}" set-version 1.15.0
    [ "${status}" -ne 0 ]
    [ -z "$(find . -name 'Cargo.*.*')" ]
}
@test "bump-base: the version of the commit that added Cargo.toml is kept" {
    cd "${BATS_TEST_TMPDIR}" || return
    git init -q repo
    cd repo || return
    git config user.name t
    git config user.email t@example.invalid
    git config commit.gpgsign false
    commit "docs: before the crate"
    set_cargo 0.1.0 "chore(release): v0.1.0 [skip ci]"
    base 0.1.0 ""
}
