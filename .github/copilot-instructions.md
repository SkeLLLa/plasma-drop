# GitHub Copilot Review Instructions

When reviewing this repository, treat it as a production Rust CLI/service for KDE Plasma 6.
Prioritize correctness, user safety, packaging behavior, and maintainability over stylistic churn.

Project context:
- The crate name is `plasma-drop`.
- Repository-level agent guidance lives in `AGENTS.md`; keep Copilot guidance aligned with it.
- The app manages a dropdown launcher window through KWin scripting, D-Bus, global shortcuts, and a user systemd service.
- Runtime assets in `resources/` are embedded with `include_str!`; package and install changes must preserve `cargo install --locked plasma-drop`.
- Native package metadata lives in `Cargo.toml` under `package.metadata.deb` and `package.metadata.generate-rpm`.
- The primary local validation command is `make check`.

Review focus:
- Flag behavior regressions in window matching, KWin script installation, D-Bus calls, global shortcut handling, config loading, and process lifecycle.
- Check error handling paths for actionable context. Prefer `anyhow::Context` on I/O, config, subprocess, and D-Bus boundaries.
- Check that config changes remain backward compatible or include clear migration behavior.
- Verify that `Cargo.lock` remains committed for application releases.
- Check that packaging changes keep binary archive, `deb`, `rpm`, and `cargo install` paths aligned.
- Prefer small, directly relevant suggestions. Do not request broad refactors unless they remove a concrete bug or substantial maintenance risk.

Expected validation:
- For Rust source or manifest changes, expect `make check`.
- For documentation-only changes, expect at least spelling/link review and no broken references.
- For packaging or release workflow changes, expect package-list or release artifact verification where practical.

Comment style:
- Lead with concrete bugs, security issues, regressions, or missing tests.
- Include file and line references when possible.
- Explain the user-visible impact and the smallest practical fix.
- Avoid comments that only restate the code or enforce personal style preferences.
