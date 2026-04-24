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
- Release workflows that need to trigger follow-up GitHub workflows must use the documented repository secret instead of relying on the default `GITHUB_TOKEN`.
- Prefer pinned action major versions and minimal shell scripting. Complex release logic should stay readable and fail early.
