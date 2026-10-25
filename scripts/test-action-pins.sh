#!/bin/sh
# Tests for scripts/check-action-pins.sh (POSIX sh). Pinned forms pass; every bypass and every
# unsupported YAML feature fails, including `uses:` text inside a block scalar.
# Set AWK=<path> to run the checker under a specific awk (the checker calls `awk`; a shim is used).
set -eu

cd "$(dirname "$0")/.."
checker="scripts/check-action-pins.sh"
work="$(mktemp -d "${TMPDIR:-/tmp}/plasma-drop-pins.XXXXXX")"
trap 'rm -rf "${work}"' EXIT
if [ -n "${AWK:-}" ]; then
    mkdir "${work}/bin"
    printf '#!/bin/sh\nexec "%s" "$@"\n' "${AWK}" >"${work}/bin/awk"
    chmod +x "${work}/bin/awk"
    PATH="${work}/bin:${PATH}"
fi

sha="0123456789abcdef0123456789abcdef01234567"
digest="0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
passed=0
failed=0

# expect <pass|fail> <name> <yaml>
expect() {
    want="$1" name="$2" file="${work}/case.yml"
    printf "%s\n" "$3" >"${file}"
    got=pass
    "${checker}" "${file}" >/dev/null 2>&1 || got=fail
    if [ "${got}" = "${want}" ]; then
        passed=$((passed + 1))
    else
        failed=$((failed + 1))
        echo "FAIL ${name}: wanted ${want}, got ${got}"
    fi
}

step() { printf 'jobs:\n  a:\n    steps:\n%s\n' "$1"; }

expect pass "sha pin" "$(step "      - uses: actions/checkout@${sha} # v7")"
expect pass "sha pin, path and quotes" "$(step "      - uses: \"owner/repo/sub/dir@${sha}\"")"
expect pass "local path" "$(step "        uses: ./.github/actions/x")"
expect pass "docker digest" "$(step "        uses: docker://alpine@sha256:${digest}")"
expect fail "uses inside run block" "$(step "      - run: |
          echo uses: actions/checkout@v7
          echo '- { uses: x@v1 }'
      - uses: actions/checkout@${sha}")"
expect fail "uses inside folded block, then pinned" "$(step "      - run: >-
          uses: bad@v1

        uses: actions/checkout@${sha}")"
expect pass "globs and empty flow mapping" "permissions: {}
on:
  push:
    paths: ['**/*.swift', '*.md']"

expect fail "tag ref" "$(step "      - uses: actions/checkout@v7")"
expect fail "short sha" "$(step "      - uses: actions/checkout@0123456")"
expect fail "branch ref" "$(step "      - uses: actions/checkout@main")"
expect fail "docker tag" "$(step "      - uses: docker://alpine:3")"
expect fail "quoted key" "$(step "      - \"uses\": actions/checkout@v7")"
expect fail "single-quoted key" "$(step "      - 'uses': actions/checkout@v7")"
expect fail "flow mapping" "$(step "      - { uses: actions/checkout@v7 }")"
expect fail "flow mapping, pinned-looking" "$(step "      - { name: x, uses: actions/checkout@${sha} }")"
expect fail "anchor" "$(step "      - &pin
        uses: actions/checkout@${sha}")"
expect fail "alias" "$(step "      - *pin")"
expect fail "anchored flow mapping" "$(step "      - &x { uses: actions/checkout@v7 }")"
expect fail "complex key" "$(step "      - ? uses
        : actions/checkout@v7")"
expect fail "uses after a block ends" "$(step "      - run: |
          echo hi
        uses: actions/checkout@v7")"
expect fail "unpinned after a block at lower indent" "$(step "      - name: x
        run: |
          echo hi
      - uses: actions/checkout@v7")"
expect fail "empty value" "$(step "      - uses:")"
expect fail "commented block opener bypass in jobs" "$(printf '%s\n' \
    'jobs: # : |' '  a:' '    steps:' '      - uses: actions/checkout@v7')"
expect fail "commented block opener bypass in steps" "$(printf '%s\n' \
    'jobs:' '  a:' '    steps: # comment: |' '      - uses: actions/checkout@v7')"
expect fail "merge key" "$(printf '%s\n' \
    'jobs:' '  a:' '    steps:' '      - <<: {name: x}' \
    "        uses: actions/checkout@${sha}")"
expect fail "duplicate key" "$(printf '%s\n' \
    'jobs:' '  a:' '    steps:' "      - uses: actions/checkout@${sha}" \
    "        uses: actions/checkout@${sha}")"
expect fail "explicit tag" "$(printf '%s\n' \
    'jobs:' '  a:' '    steps:' "      - uses: !custom actions/checkout@${sha}")"
expect fail "non-scalar mapping key" "$(printf '%s\n' \
    'jobs:' '  a:' '    steps:' '      - ? [name, value]' \
    "        : actions/checkout@${sha}")"
expect fail "multiple YAML documents" "$(printf '%s\n' \
    'jobs:' '  a: {}' '---' 'jobs:' '  b: {}')"
expect fail "invalid YAML" "jobs: [unterminated"
expect fail "non-scalar uses value" "$(step "      - uses: [actions/checkout@${sha}]")"

expect pass "block scalar without uses, then pinned" "$(step "      - run: |
          echo hi *.txt &x !y
      - uses: actions/checkout@${sha}")"
expect pass "expression with ! and quotes" "$(step "      - if: \${{ !cancelled() && env.X == 'a b' }}
        uses: actions/checkout@${sha}")"
expect pass "apostrophe in plain scalar" "$(step "      - name: it's fine # note
        uses: actions/checkout@${sha}")"
expect pass "hash inside quotes" "$(step "      - name: \"a #b\"
        uses: actions/checkout@${sha}")"
expect pass "CRLF line endings" "$(printf 'jobs:\r\n  a:\r\n    steps:\r\n      - uses: actions/checkout@%s\r' "${sha}")"
expect fail "comment-bypass: fake sha in comment" "$(step "      - uses: a/b@main # @${sha}")"
expect fail "comment-bypass: empty comment" "$(step "      - uses: a/b@v1 #")"
expect fail "comment-bypass: hash without space" "$(step "      - uses: a/b@v1#@${sha}")"
expect fail "comment-bypass: quoted tag then sha comment" "$(step "      - uses: \"a/b@v1\" # a/b@${sha}")"
expect fail "unterminated quote is ambiguous" "$(step "      - name: 'x # y
        uses: actions/checkout@${sha}")"
expect fail "uses with space before colon" "$(step "      - uses : actions/checkout@v7")"
expect fail "uses as prose" "$(step "      - name: uses a thing
        uses: actions/checkout@${sha}")"
expect fail "docs end marker" "$(printf '%s\n' 'jobs: {}' '...')"
expect fail "document start after first line" "$(printf '%s\n' 'jobs: {}' '---' 'x: 1')"
expect pass "leading document start" "$(printf '%s\n' '---' 'jobs: {}')"
expect fail "alias value" "$(step "      - name: *pin
        uses: actions/checkout@${sha}")"
expect fail "tag on other key" "$(step "      - name: !!str x
        uses: actions/checkout@${sha}")"
expect fail "multi-line flow mapping" "$(step "      - {
          uses: actions/checkout@v7 }")"
expect fail "uses flow value" "$(step "      - uses: [actions/checkout@${sha}]")"
expect fail "uses block value" "$(step "      - uses: |
          actions/checkout@${sha}")"
expect fail "uses sibling-indented duplicate" "$(step "      - uses: actions/checkout@${sha}
        uses: actions/cache@${sha}")"
expect pass "uses in separate steps" "$(step "      - uses: actions/checkout@${sha}
      - uses: actions/cache@${sha}")"
expect fail "tab indentation" "$(printf 'jobs:\n\ta: {}\n')"
expect fail "docker bad digest length" "$(step "        uses: docker://alpine@sha256:0123")"
expect fail "sha uppercase" "$(step "      - uses: actions/checkout@0123456789ABCDEF0123456789ABCDEF01234567")"

if [ ! -r "${work}/case.yml" ] || "${checker}" >/dev/null 2>&1; then
    failed=$((failed + 1))
    echo "FAIL no files must fail"
else
    passed=$((passed + 1))
fi
if "${checker}" "${work}/missing.yml" >/dev/null 2>&1; then
    failed=$((failed + 1))
    echo "FAIL missing file must fail"
else
    passed=$((passed + 1))
fi

echo "${passed} passed, ${failed} failed"
[ "${failed}" -eq 0 ]
