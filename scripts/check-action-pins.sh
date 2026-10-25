#!/bin/sh
# Check every `uses:` line in each workflow/action file. POSIX sh + awk only (BSD awk, mawk, gawk).
# Callers must include .github/actions/**/action.y?ml whenever that tree exists.
# Usage: scripts/check-action-pins.sh [--warn] <workflow-or-action.yml>...
#   Strict and fail-closed: this is a line scanner, not a YAML parser, so any YAML feature that could
#   hide a `uses` is an error. --warn annotates unpinned `uses` refs and returns 0 for CI transition
#   mode; structure errors (anchors, aliases, tags, merge keys, complex keys, flow collections,
#   multi-document markers, `uses:` inside a block scalar, a `uses` that is not a plain key,
#   duplicate `uses` keys, ambiguous quoting) always exit 1, as do no files or unreadable files.
set -eu

warn=0
if [ "${1:-}" = "--warn" ]; then
    warn=1
    shift
fi
if [ "$#" -eq 0 ]; then
    echo "::error::check-action-pins.sh: no workflow files given" >&2
    exit 2
fi

prog="$(mktemp "${TMPDIR:-/tmp}/check-action-pins.XXXXXX")"
trap 'rm -f "${prog}"' EXIT

cat >"${prog}" <<'AWK'
function note(kind, msg) {
    printf "::%s file=%s,line=%d::%s\n", kind, FILENAME, FNR, msg
}
function hard(msg) { note("error", msg); nhard++ }

# Drop a trailing comment (`#` preceded by whitespace, outside quotes). A quote only opens at a
# token start; an unclosed quote makes the comment boundary ambiguous (amb=1).
function strip(s,    n, i, c, q, prev) {
    n = length(s); q = ""; amb = 0
    for (i = 1; i <= n; i++) {
        c = substr(s, i, 1)
        prev = (i > 1) ? substr(s, i - 1, 1) : " "
        if (q != "") {
            if (q == "\047") {
                if (c == "\047") { if (substr(s, i + 1, 1) == "\047") i++; else q = "" }
            } else if (c == "\\") i++
            else if (c == "\"") q = ""
            continue
        }
        if ((c == "\047" || c == "\"") && (prev == " " || prev == "\t" || prev == "[" || prev == "{" || prev == ",")) {
            q = c; continue
        }
        if (c == "#" && (prev == " " || prev == "\t")) { s = substr(s, 1, i - 1); break }
    }
    if (q != "") amb = 1
    sub(/[ \t]+$/, "", s)
    return s
}

function pinned(v,    p, h, parts, n) {
    if (v ~ /^\.\/[^ \t]+$/) return 1
    if (v ~ /^docker:\/\/[^ \t@]+@sha256:[0-9a-f]+$/) {
        h = substr(v, index(v, "@sha256:") + 8)
        return length(h) == 64
    }
    n = split(v, parts, "@")
    if (n != 2) return 0
    if (length(parts[2]) != 40 || parts[2] !~ /^[0-9a-f]+$/) return 0
    return parts[1] ~ /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+(\/[^ \t@]+)?$/
}

function imgpinned(v) { return v ~ /@sha256:[0-9a-f]+$/ && length(v) - index(v, "@sha256:") - 7 == 64 }

FNR == 1 { inblock = 0; split("", seen) }

{
    line = $0
    sub(/\r$/, "", line)

    if (line ~ /^---([ \t]|$)/ && FNR > 1) { hard("multiple YAML documents are not allowed"); next }
    if (line ~ /^\.\.\.([ \t]|$)/) { hard("YAML document end marker is not allowed"); next }

    if (line ~ /^[ \t]*$/) next
    indent = match(line, /[^ ]/) - 1
    if (inblock) {
        if (indent > pc) {
            if (line ~ /uses:/) hard("`uses:` inside a block scalar is not allowed")
            next
        }
        inblock = 0
    }
    if (line ~ /^[ ]*\t/) { hard("tab indentation is not allowed"); next }
    if (line ~ /^[ ]*#/) next

    s = strip(line)
    if (amb) { hard("ambiguous quoting; cannot find the end of the line"); next }

    # Blank expressions and quoted strings so their contents cannot look like YAML syntax.
    b = s
    gsub(/\$\{\{[^}]*\}\}/, "E", b)
    gsub(/"([^"\\]|\\.)*"/, "Q", b)
    gsub(/\047[^\047]*\047/, "Q", b)

    # Peel leading `- ` markers: kc is the column of the key, dc the column of the last dash.
    r = s; rb = b; col = 0; dc = -1
    while (match(rb, /^[ ]*-([ \t]+|$)/)) {
        dc = col + index(rb, "-") - 1
        col += RLENGTH
        r = substr(r, RLENGTH + 1); rb = substr(rb, RLENGTH + 1)
    }
    m = match(rb, /[^ ]/); kc = col + (m ? m - 1 : 0)
    sub(/^[ ]+/, "", r); sub(/^[ ]+/, "", rb)

    for (c in seen) if (c + 0 > indent) delete seen[c]

    bad = 0
    if (b ~ /<</) { hard("YAML merge keys are not allowed"); bad = 1 }
    if (rb ~ /^[&*!]/ || rb ~ /[:,\[{?-][ \t]+[&*!]/ || rb ~ /[\[{,][&*!]/) {
        hard("YAML anchors, aliases and tags are not allowed"); bad = 1
    }
    if (rb ~ /^\?([ \t]|$)/) { hard("complex YAML mapping keys are not allowed"); bad = 1 }
    t = rb; nopen = gsub(/[\[{]/, "", t)
    t = rb; nclose = gsub(/[\]}]/, "", t)
    if (nopen != nclose) { hard("multi-line flow collections are not allowed"); bad = 1 }
    if (bad) next

    # `container:` / `image:` values (job containers, services, matrix entries) need a digest.
    t = s
    while (match(t, /(^|[ \t{,-])(image|container):[ \t]*("[^"]*"|\047[^\047]*\047|[^ \t,}]+)/)) {
        seg = substr(t, RSTART, RLENGTH)
        t = substr(t, RSTART + RLENGTH)
        sub(/^[ \t{,-]*(image|container):[ \t]*/, "", seg)
        if (length(seg) >= 2 && ((seg ~ /^".*"$/) || (seg ~ /^\047.*\047$/))) seg = substr(seg, 2, length(seg) - 2)
        if (seg == "" || seg ~ /\$\{\{/) continue
        if (!imgpinned(seg)) {
            note(warnmode ? "warning" : "error", "container image is not pinned to an @sha256 digest")
            nsoft++
        }
    }

    if (s ~ /(^|[^A-Za-z0-9_-])uses($|[^A-Za-z0-9_-])/) {
        if (r ~ /^uses:[ \t]+[^ \t]/) {
            v = r; sub(/^uses:[ \t]+/, "", v)
            if (v ~ /^[\[{|>]/) { hard("`uses` value must be a plain scalar"); next }
            if (length(v) >= 2 && ((v ~ /^".*"$/) || (v ~ /^\047.*\047$/))) v = substr(v, 2, length(v) - 2)
            if (seen[kc]) hard("duplicate `uses` key")
            seen[kc] = 1
            if (!pinned(v)) {
                note(warnmode ? "warning" : "error", "uses value is not pinned")
                nsoft++
            }
        } else if (r ~ /^uses:[ \t]*$/) {
            hard("`uses` has no value on its line")
        } else {
            hard("`uses` must be a plain `uses: value` key")
        }
        next
    }

    if (rb ~ /(^|:[ \t]+)[|>][-+0-9]*$/) {
        inblock = 1
        pc = (rb ~ /^[|>]/ && dc >= 0) ? dc : kc
    }
}

END {
    nhard += prehard
    if (nhard > 0) {
        printf "%d YAML structure error(s), %d unpinned action ref(s).\n", nhard, nsoft > "/dev/stderr"
        exit 1
    }
    if (nsoft > 0 && !warnmode) {
        printf "%d action reference(s) are not pinned to a 40-hex commit SHA, local path or docker digest (action or container image).\n", nsoft > "/dev/stderr"
        exit 1
    }
    if (nsoft == 0) print "All actions are pinned."
}
AWK

prehard=0
n=$#
while [ "${n}" -gt 0 ]; do
    f="$1"
    shift
    if [ -f "${f}" ] && [ -r "${f}" ]; then
        set -- "$@" "${f}"
    else
        echo "::error file=${f}::file is missing or unreadable"
        prehard=$((prehard + 1))
    fi
    n=$((n - 1))
done

awk_status=0
awk -v warnmode="${warn}" -v prehard="${prehard}" -f "${prog}" "$@" /dev/null || awk_status=$?

# Every pinned container image must be one of packaging/ci/images.env, so CI and the local
# `mise run linux:container:*` tasks run the same bits.
images="$(dirname "$0")/../packaging/ci/images.env"
if [ -f "${images}" ] && [ "${awk_status}" -eq 0 ]; then
    known="$(sed -n 's/^[A-Z_]*_IMAGE=//p' "${images}")"
    for f in "$@"; do
        grep -E '(^|[^A-Za-z0-9_-])(image|container):' "${f}" |
            grep -oE '[A-Za-z0-9._/-]+:[A-Za-z0-9._-]+@sha256:[0-9a-f]{64}' |
            while read -r ref; do
                printf '%s\n' "${known}" | grep -qxF "${ref}" || {
                    echo "::error file=${f}::image ${ref} is not in packaging/ci/images.env" >&2
                    echo bad
                }
            done
    done | grep -q bad && exit 1
fi
exit "${awk_status}"
