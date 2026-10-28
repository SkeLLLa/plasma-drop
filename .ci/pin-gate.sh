#!/usr/bin/env bash
# The release pin gate, shared by release-prepare.yml and release.yml. Each workflow fetches this
# file as committed at its GITHUB_SHA and runs it with GH_TOKEN, GITHUB_REPOSITORY, GITHUB_SHA and
# RUNNER_TEMP set. It fetches every workflow, the checker and its image list at the same SHA; no
# checkout, no third-party action. Fails closed: no files, no listing or any failed fetch fails
# the gate, and so does an unpinned action in any workflow.
set -euo pipefail

: "${GITHUB_REPOSITORY:?}" "${GITHUB_SHA:?}" "${RUNNER_TEMP:?}"
dir="${RUNNER_TEMP}/workflows"
mkdir -p "${dir}" "${RUNNER_TEMP}/.ci"
fetch() {
    gh api -H "Accept: application/vnd.github.raw" \
        "repos/${GITHUB_REPOSITORY}/contents/$1?ref=${GITHUB_SHA}"
}
names="$(
    gh api "repos/${GITHUB_REPOSITORY}/contents/.github/workflows?ref=${GITHUB_SHA}" \
        --jq '.[] | select(.type == "file") | select(.name | test("\\.ya?ml$")) | .name'
)"
if [ -z "${names}" ]; then
    echo "::error::no workflow files found at ${GITHUB_SHA}"
    exit 1
fi
files=()
while IFS= read -r name; do
    case "${name}" in */* | "")
        echo "::error::unexpected workflow name"
        exit 1
        ;;
    esac
    fetch ".github/workflows/${name}" >"${dir}/${name}"
    files+=("${dir}/${name}")
done <<<"${names}"
fetch .ci/check-action-pins.sh >"${RUNNER_TEMP}/.ci/check-action-pins.sh"
fetch .ci/images.env >"${RUNNER_TEMP}/.ci/images.env"
sh "${RUNNER_TEMP}/.ci/check-action-pins.sh" "${files[@]}"
