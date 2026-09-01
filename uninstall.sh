#!/bin/sh
# slipcase — uninstaller
#
# Removes the slipcase binary installed by install.sh. slipcase stores nothing
# else on disk (no config, no history), so this is the entire cleanup.
#
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/slpc-rust/main/uninstall.sh | sh

set -eu

if [ -n "${CARGO_HOME:-}" ]; then
    install_dir="$CARGO_HOME/bin"
else
    install_dir="$HOME/.cargo/bin"
fi

target="$install_dir/slipcase"

if [ -e "$target" ]; then
    rm -f "$target"
    echo "Removed $target"
elif command -v slipcase >/dev/null 2>&1; then
    found="$(command -v slipcase)"
    echo "slipcase is installed at $found, not the expected location ($target)."
    echo "Remove it manually if you want it gone."
    exit 1
else
    echo "slipcase is not installed."
fi
