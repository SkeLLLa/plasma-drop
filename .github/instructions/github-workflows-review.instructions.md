---
applyTo: ".github/**/*.yml,.github/**/*.yaml,release-plz.toml"
---

# GitHub Workflow Review Instructions

Review GitHub workflow and release automation changes with these additional checks.

- Keep CI fast and focused on `make check` for pull requests.
- Check that release workflows only run on the intended events and branches.
- Do not grant broader permissions than a job needs.
- Treat release credentials and package signing material as secrets. Do not expose tokens in logs, artifact names, or command output.
- Verify that package build changes keep binary archive, `deb`, and `rpm` artifacts versioned consistently.
- Release workflows start follow-up workflows only through `workflow_dispatch` (or `repository_dispatch`), the events the default `GITHUB_TOKEN` can trigger; don't add a PAT or App secret for it, and don't rely on pushes, tags or releases made with `GITHUB_TOKEN` to start a workflow.
- `release.yml` must keep its `inputs.sha` check in `pin-gate` and the live-`master` check before tagging in the `release` job (a run, or a re-run, whose commit `master` has left tags nothing unless the tag is already on that commit), and `release-prepare.yml` must never push or dispatch unless master's HEAD equals `GITHUB_SHA`.
- Prefer pinned action major versions and minimal shell scripting. Complex release logic should stay readable and fail early.
