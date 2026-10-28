#!/usr/bin/env bats
# Tests for .ci/check-action-pins.sh (POSIX sh). Pinned forms pass; every bypass and every
# unsupported YAML feature fails, including `uses:` text inside a block scalar.
# Run with `bats .ci/check-action-pins.bats` (or `make lint-actions`).
# Set AWK=<path> to run the checker under a specific awk (the checker calls `awk`; a shim is used).

sha="0123456789abcdef0123456789abcdef01234567"
digest="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

setup() {
    checker="${BATS_TEST_DIRNAME}/check-action-pins.sh"
    if [ -n "${AWK:-}" ]; then
        mkdir "${BATS_TEST_TMPDIR}/bin"
        printf '#!/bin/sh\nexec "%s" "$@"\n' "${AWK}" >"${BATS_TEST_TMPDIR}/bin/awk"
        chmod +x "${BATS_TEST_TMPDIR}/bin/awk"
        PATH="${BATS_TEST_TMPDIR}/bin:${PATH}"
    fi
}

# accepts|rejects <yaml>: run the checker on a one-file workflow.
check() {
    printf "%s\n" "$1" >"${BATS_TEST_TMPDIR}/case.yml"
    run "${checker}" "${BATS_TEST_TMPDIR}/case.yml"
}
accepts() {
    check "$1"
    [ "${status}" -eq 0 ] || { echo "${output}"; return 1; }
}
rejects() {
    check "$1"
    [ "${status}" -ne 0 ] || { echo "checker accepted:"; printf "%s\n" "$1"; return 1; }
}

step() { printf 'jobs:\n  a:\n    steps:\n%s\n' "$1"; }

@test "sha pin" {
    accepts "$(step "      - uses: actions/checkout@${sha} # v7")"
}
@test "sha pin, path and quotes" {
    accepts "$(step "      - uses: \"owner/repo/sub/dir@${sha}\"")"
}
@test "local path" {
    accepts "$(step "        uses: ./.github/actions/x")"
}
@test "docker digest" {
    accepts "$(step "        uses: docker://alpine@sha256:${digest}")"
}
@test "uses inside run block" {
    rejects "$(step "      - run: |
          echo uses: actions/checkout@v7
          echo '- { uses: x@v1 }'
      - uses: actions/checkout@${sha}")"
}
@test "uses inside folded block, then pinned" {
    rejects "$(step "      - run: >-
          uses: bad@v1

        uses: actions/checkout@${sha}")"
}
@test "globs and empty flow mapping" {
    accepts "permissions: {}
on:
  push:
    paths: ['**/*.rs', '*.md']"
}

@test "tag ref" {
    rejects "$(step "      - uses: actions/checkout@v7")"
}
@test "short sha" {
    rejects "$(step "      - uses: actions/checkout@0123456")"
}
@test "branch ref" {
    rejects "$(step "      - uses: actions/checkout@main")"
}
@test "docker tag" {
    rejects "$(step "      - uses: docker://alpine:3")"
}
@test "quoted key" {
    rejects "$(step "      - \"uses\": actions/checkout@v7")"
}
@test "single-quoted key" {
    rejects "$(step "      - 'uses': actions/checkout@v7")"
}
@test "flow mapping" {
    rejects "$(step "      - { uses: actions/checkout@v7 }")"
}
@test "flow mapping, pinned-looking" {
    rejects "$(step "      - { name: x, uses: actions/checkout@${sha} }")"
}
@test "anchor" {
    rejects "$(step "      - &pin
        uses: actions/checkout@${sha}")"
}
@test "alias" {
    rejects "$(step "      - *pin")"
}
@test "anchored flow mapping" {
    rejects "$(step "      - &x { uses: actions/checkout@v7 }")"
}
@test "complex key" {
    rejects "$(step "      - ? uses
        : actions/checkout@v7")"
}
@test "uses after a block ends" {
    rejects "$(step "      - run: |
          echo hi
        uses: actions/checkout@v7")"
}
@test "unpinned after a block at lower indent" {
    rejects "$(step "      - name: x
        run: |
          echo hi
      - uses: actions/checkout@v7")"
}
@test "empty value" {
    rejects "$(step "      - uses:")"
}
@test "commented block opener bypass in jobs" {
    rejects "$(printf '%s\n' 'jobs: # : |' '  a:' '    steps:' '      - uses: actions/checkout@v7')"
}
@test "commented block opener bypass in steps" {
    rejects "$(printf '%s\n' 'jobs:' '  a:' '    steps: # comment: |' '      - uses: actions/checkout@v7')"
}
@test "merge key" {
    rejects "$(printf '%s\n' 'jobs:' '  a:' '    steps:' '      - <<: {name: x}' \
        "        uses: actions/checkout@${sha}")"
}
@test "duplicate key" {
    rejects "$(printf '%s\n' 'jobs:' '  a:' '    steps:' "      - uses: actions/checkout@${sha}" \
        "        uses: actions/checkout@${sha}")"
}
@test "explicit tag" {
    rejects "$(printf '%s\n' 'jobs:' '  a:' '    steps:' "      - uses: !custom actions/checkout@${sha}")"
}
@test "non-scalar mapping key" {
    rejects "$(printf '%s\n' 'jobs:' '  a:' '    steps:' '      - ? [name, value]' \
        "        : actions/checkout@${sha}")"
}
@test "multiple YAML documents" {
    rejects "$(printf '%s\n' 'jobs:' '  a: {}' '---' 'jobs:' '  b: {}')"
}
@test "invalid YAML" {
    rejects "jobs: [unterminated"
}
@test "non-scalar uses value" {
    rejects "$(step "      - uses: [actions/checkout@${sha}]")"
}

@test "block scalar without uses, then pinned" {
    accepts "$(step "      - run: |
          echo hi *.txt &x !y
      - uses: actions/checkout@${sha}")"
}
@test "expression with ! and quotes" {
    accepts "$(step "      - if: \${{ !cancelled() && env.X == 'a b' }}
        uses: actions/checkout@${sha}")"
}
@test "apostrophe in plain scalar" {
    accepts "$(step "      - name: it's fine # note
        uses: actions/checkout@${sha}")"
}
@test "hash inside quotes" {
    accepts "$(step "      - name: \"a #b\"
        uses: actions/checkout@${sha}")"
}
@test "CRLF line endings" {
    accepts "$(printf 'jobs:\r\n  a:\r\n    steps:\r\n      - uses: actions/checkout@%s\r' "${sha}")"
}
@test "comment-bypass: fake sha in comment" {
    rejects "$(step "      - uses: a/b@main # @${sha}")"
}
@test "comment-bypass: empty comment" {
    rejects "$(step "      - uses: a/b@v1 #")"
}
@test "comment-bypass: hash without space" {
    rejects "$(step "      - uses: a/b@v1#@${sha}")"
}
@test "comment-bypass: quoted tag then sha comment" {
    rejects "$(step "      - uses: \"a/b@v1\" # a/b@${sha}")"
}
@test "unterminated quote is ambiguous" {
    rejects "$(step "      - name: 'x # y
        uses: actions/checkout@${sha}")"
}
@test "uses with space before colon" {
    rejects "$(step "      - uses : actions/checkout@v7")"
}
@test "uses as prose" {
    rejects "$(step "      - name: uses a thing
        uses: actions/checkout@${sha}")"
}
@test "docs end marker" {
    rejects "$(printf '%s\n' 'jobs: {}' '...')"
}
@test "document start after first line" {
    rejects "$(printf '%s\n' 'jobs: {}' '---' 'x: 1')"
}
@test "leading document start" {
    accepts "$(printf '%s\n' '---' 'jobs: {}')"
}
@test "alias value" {
    rejects "$(step "      - name: *pin
        uses: actions/checkout@${sha}")"
}
@test "tag on other key" {
    rejects "$(step "      - name: !!str x
        uses: actions/checkout@${sha}")"
}
@test "multi-line flow mapping" {
    rejects "$(step "      - {
          uses: actions/checkout@v7 }")"
}
@test "uses flow value" {
    rejects "$(step "      - uses: [actions/checkout@${sha}]")"
}
@test "uses block value" {
    rejects "$(step "      - uses: |
          actions/checkout@${sha}")"
}
@test "uses sibling-indented duplicate" {
    rejects "$(step "      - uses: actions/checkout@${sha}
        uses: actions/cache@${sha}")"
}
@test "uses in separate steps" {
    accepts "$(step "      - uses: actions/checkout@${sha}
      - uses: actions/cache@${sha}")"
}
@test "tab indentation" {
    rejects "$(printf 'jobs:\n\ta: {}\n')"
}
@test "docker bad digest length" {
    rejects "$(step "        uses: docker://alpine@sha256:0123")"
}
@test "sha uppercase" {
    rejects "$(step "      - uses: actions/checkout@0123456789ABCDEF0123456789ABCDEF01234567")"
}

@test "no files must fail" {
    run "${checker}"
    [ "${status}" -ne 0 ]
}
@test "missing file must fail" {
    run "${checker}" "${BATS_TEST_TMPDIR}/missing.yml"
    [ "${status}" -ne 0 ]
}

# Container images: a digest is required, and it must be listed in images.env.
job_container() { printf 'jobs:\n  a:\n    container: %s\n    steps:\n      - run: "true"\n' "$1"; }
@test "container listed in images.env" {
    printf 'DEBIAN_IMAGE=debian:trixie@sha256:%s\n' "${digest}" >"${BATS_TEST_TMPDIR}/images.env"
    IMAGES="${BATS_TEST_TMPDIR}/images.env" accepts "$(job_container "debian:trixie@sha256:${digest}")"
}
@test "matrix image listed in images.env" {
    printf 'FEDORA_IMAGE=fedora:44@sha256:%s\n' "${digest}" >"${BATS_TEST_TMPDIR}/images.env"
    # shellcheck disable=SC2016 # ${{ }} is the workflow's expression, not the shell's.
    IMAGES="${BATS_TEST_TMPDIR}/images.env" accepts "$(printf 'jobs:\n  a:\n    strategy:\n      matrix:\n        include:\n          - { image: "fedora:44@sha256:%s" }\n    container: ${{ matrix.image }}\n    steps:\n      - run: "true"\n' "${digest}")"
}
@test "container not in images.env" {
    printf 'DEBIAN_IMAGE=debian:trixie@sha256:%s\n' "${digest}" >"${BATS_TEST_TMPDIR}/images.env"
    IMAGES="${BATS_TEST_TMPDIR}/images.env" rejects "$(job_container "debian:bookworm@sha256:${digest}")"
}
@test "container tag" {
    printf 'DEBIAN_IMAGE=debian:trixie@sha256:%s\n' "${digest}" >"${BATS_TEST_TMPDIR}/images.env"
    IMAGES="${BATS_TEST_TMPDIR}/images.env" rejects "$(job_container "debian:trixie")"
}
