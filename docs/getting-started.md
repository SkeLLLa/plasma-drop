# Getting Started

## Requirements

- Linux desktop session
- KDE Plasma 6 on Wayland
- Session D-Bus
- `KWin` scripting support

This project does not target X11 or non-KWin compositors.

## Build

```bash
cargo build
```

## Install with `cargo`

You can install `plasma-drop` directly from crates.io:

```bash
cargo install --locked plasma-drop
```

After that, generate the starter files from the installed binary:

```bash
plasma-drop init --systemd
```

That gives `cargo install` users the same starter assets that package installs ship separately.
Use `plasma-drop init` if you do not want a `systemd --user` unit created.

## Manual Installation

### From the binary archive

After extracting the release tarball:

```bash
cd plasma-drop-<version>-x86_64-unknown-linux-gnu
./install-user.sh
```

That installs:

- `plasma-drop` into `~/.local/bin`
- a starter config into `~/.config/plasma-drop/config.toml`
- a user service into `~/.config/systemd/user/plasma-drop.service` for `systemd --user` setups

### From `deb` or `rpm`

Install the package with your distro package manager, then copy the example config into place:

```bash
mkdir -p ~/.config/plasma-drop
cp /usr/share/plasma-drop/examples/config.toml ~/.config/plasma-drop/config.toml
```

The native packages install:

- `/usr/bin/plasma-drop`
- `/usr/lib/systemd/user/plasma-drop.service`
- `/usr/share/plasma-drop/examples/config.toml`

### Enable the service

If you use `systemd --user`, enable the user service after adjusting the config:

```bash
systemctl --user daemon-reload
systemctl --user enable --now plasma-drop.service
journalctl --user -u plasma-drop.service -f
```

### Start without `systemd`

If your session does not use `systemd --user`, start `plasma-drop` directly from your session
startup mechanism instead:

```bash
plasma-drop --config ~/.config/plasma-drop/config.toml
```

Examples:

- add that command to Plasma autostart
- launch it from your compositor/session startup script
- run it manually in a terminal while testing config changes

For foreground debugging, add verbosity:

```bash
plasma-drop --config ~/.config/plasma-drop/config.toml -v
```

Useful helper commands from the binary:

```bash
plasma-drop init --systemd
plasma-drop print-example-config
plasma-drop print-systemd-service
```

## Run

By default the app reads:

```text
~/.config/plasma-drop/config.toml
```

You can override that with:

```bash
cargo run -- --config /absolute/path/to/config.toml
```

## Typical Session Flow

1. The process loads config and validates placement.
2. It connects to session D-Bus and loads the `KWin` helper script.
3. It clears old `plasma_drop_hotkey_*` shortcuts.
4. It registers shortcuts for configured apps.
5. Each shortcut press either finds an existing window or launches one, then moves it into the configured visible rect.

## Sample Config

See:

- `resources/example-config.toml`

That file includes native app examples and a Flatpak launch example.
- Installed packages also ship the same example as `/usr/share/plasma-drop/examples/config.toml`.
