# Configuration

The config file is TOML and contains one or more `[[app]]` entries.

## Core Fields

- `name`: unique app identifier used internally
- `hotkey`: shortcut string such as `super+f9`
- `filename`: identity matcher for existing windows
- `command`: explicit spawn command array
- `process_name`: optional regex matcher against `KWin` identity fields
- `window_title`: optional regex matcher against the caption
- `attach_mode`: `find` or `find-or-start`
- `working_directory`: optional absolute working directory
- `hide_decorations`: optional boolean; hide the `KWin` title bar/border while plasma-drop manages the window

## Matching Behavior

Existing windows are matched using `KWin` window metadata:

- `desktopFileName`
- `resourceClass`
- `resourceName`
- `caption`

`filename` is used as a simple case-insensitive identity matcher.
`process_name` and `window_title` allow narrower regex-based matching.

## Launch Behavior

If `command` is present, it is used exactly as the spawn command.
If `command` is omitted, spawning falls back to `filename` plus legacy `arguments`.

This split is important for wrapper-based apps such as Flatpak, where launch identity and window identity can differ.

## Window Decorations

Set `hide_decorations = true` in an `[[app]]` entry to remove the `KWin` title bar and border while plasma-drop controls that window.

plasma-drop records the window's original decoration state when it attaches and restores it on shutdown. Existing undecorated windows remain undecorated.

## Placement

Each app may define:

```toml
[app.placement]
width = "50%"
height = "100%"
position = "left"
offset_x = "0px"
offset_y = "0px"
```

Supported metrics:

- percentages like `"50%"`
- pixel values like `"640px"`

Placement is resolved relative to screen `0` in the current implementation.

## Animation

Each app may also define an optional animation block:

```toml
[app.animation]
style = "slide-fade"
easing = "ease-out"
duration_ms = 180
```

Supported values:

- `style`: `none`, `slide`, `fade`, or `slide-fade`
- `easing`: `linear`, `ease-out`, or `ease-in-out`
- `duration_ms`: integer duration from `0` to `2000`

Defaults:

- `style = "none"`
- `easing = "ease-out"`
- `duration_ms = 150`

Current status:

- `slide` animates window geometry between the hidden and visible rects
- `fade` animates window opacity using the `KWin` script bridge
- `slide-fade` combines both tracks
- `none` keeps the original instant behavior
