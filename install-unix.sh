#!/usr/bin/env bash
set -euo pipefail
prefix="${PREFIX:-/usr/local}"
install -d "$prefix/bin"
install -m 0755 "$(dirname "$0")/batchconverttochd.sh" "$prefix/bin/batchconverttochd"
echo "Installed: $prefix/bin/batchconverttochd"
