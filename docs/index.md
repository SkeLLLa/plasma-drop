# plasma-drop

`plasma-drop` is a KDE Plasma 6 dropdown app launcher for Wayland sessions driven by `KWin` scripting and global shortcuts.

## What It Does

- Loads a TOML config describing managed apps.
- Registers one global shortcut per configured app through `KWin`.
- Finds or launches matching windows and toggles them in and out of view.
- Supports optional per-app `slide`, `fade`, and `slide-fade` transitions.
- Ensures only one managed app is visible at a time.

## Guide Pages

- [`getting_started`]: runtime requirements, build, and launch flow.
- [`configuration`]: config model, matching rules, placement, and animation settings.
- [`development`]: local workflow, checks, and repo layout.
- [`copilot-review`](copilot-review.md): GitHub Copilot review instructions and maintainer workflow.
- [`distribution`](distribution.md): release artifact layout plus manual install steps for archive, `deb`, and `rpm` installs.
  It also documents the `cargo install` path.

## Related Project

`plasma-drop` is heavily inspired by
[windows-terminal-quake](https://github.com/flyingpie/windows-terminal-quake). Use that app if
you need a GUI, a Windows version, or more configuration options.
