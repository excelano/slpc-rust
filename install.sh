#!/bin/sh
# slipcase — installer shim
#
# Delegates to the cargo-dist-generated installer for the latest release.
# This exists so the install and uninstall one-liners share a URL shape:
#
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/slpc-rust/main/install.sh | sh
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/slpc-rust/main/uninstall.sh | sh
#
# The repository is `slpc-rust` and the tool is `slipcase`, so these URLs carry
# the repository name while everything they install carries the tool's.

set -eu

curl --proto '=https' --tlsv1.2 -LsSf \
    https://github.com/excelano/slpc-rust/releases/latest/download/slipcase-installer.sh | sh
