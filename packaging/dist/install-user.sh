#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
CONFIG_ROOT="${XDG_CONFIG_HOME:-${HOME}/.config}"
APP_CONFIG_DIR="${CONFIG_ROOT}/plasma-drop"
SYSTEMD_USER_DIR="${CONFIG_ROOT}/systemd/user"
SOURCE_SERVICE="${ROOT_DIR}/share/systemd/user/plasma-drop.service"
TARGET_SERVICE="${SYSTEMD_USER_DIR}/plasma-drop.service"
SOURCE_CONFIG="${ROOT_DIR}/share/plasma-drop/examples/config.toml"
TARGET_CONFIG="${APP_CONFIG_DIR}/config.toml"

install -d "${BIN_DIR}" "${APP_CONFIG_DIR}" "${SYSTEMD_USER_DIR}"
install -m 755 "${ROOT_DIR}/bin/plasma-drop" "${BIN_DIR}/plasma-drop"
install -m 644 "${SOURCE_CONFIG}" "${APP_CONFIG_DIR}/config.example.toml"

if [[ ! -f "${TARGET_CONFIG}" ]]; then
  install -m 644 "${SOURCE_CONFIG}" "${TARGET_CONFIG}"
fi

sed "s|/usr/bin/plasma-drop|${BIN_DIR}/plasma-drop|g" "${SOURCE_SERVICE}" > "${TARGET_SERVICE}"
chmod 644 "${TARGET_SERVICE}"

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user daemon-reload || true
fi

cat <<'EOF'
Installed plasma-drop into ~/.local/bin and copied the sample configuration into:
  ~/.config/plasma-drop/config.toml
  ~/.config/plasma-drop/config.example.toml

Next steps:
  1. Edit ~/.config/plasma-drop/config.toml
  2. systemctl --user enable --now plasma-drop.service
  3. journalctl --user -u plasma-drop.service -f
EOF
