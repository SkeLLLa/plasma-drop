# GitHub Copilot Review

This repository configures GitHub Copilot code review with repository custom instructions.

## Files

- `.github/copilot-instructions.md`: repository-wide review instructions.
- `.github/instructions/rust-review.instructions.md`: Rust, Cargo, and lint-specific review instructions.
- `.github/instructions/github-workflows-review.instructions.md`: GitHub Actions and release automation review instructions.
- `AGENTS.md`: repository-level guidance for AI coding agents; keep Copilot instructions aligned with it.

GitHub Copilot code review currently reads the first 4,000 characters of each custom instruction
file, so keep these files short and review-focused.

## Requesting a Review

1. Open a pull request on GitHub.
2. In the Reviewers menu, select `Copilot`.
3. Wait for the review comments and suggested changes.
4. After pushing fixes, request another Copilot review if the change materially changed.

Copilot uses the instruction files from the pull request base branch. Changes to the instruction
files only affect reviews after they are merged into the target branch.

## Automatic Reviews

Automatic Copilot reviews are configured in GitHub repository settings, not in this repository's
files. Maintainers can enable them at:

`Settings` -> `Copilot` -> `Code review`

Keep `Use custom instructions when reviewing pull requests` enabled so Copilot applies this
repository's guidance.

## Maintenance

- Keep the repository-wide file focused on behavior that applies to every review.
- Put language or path-specific guidance in `.github/instructions/*.instructions.md`.
- Avoid duplicating instructions across files.
- Update this documentation when GitHub changes the review settings flow or supported instruction
  file behavior.

## References

- https://docs.github.com/copilot/using-github-copilot/code-review/using-copilot-code-review
- https://docs.github.com/copilot/customizing-copilot/adding-repository-custom-instructions-for-github-copilot
