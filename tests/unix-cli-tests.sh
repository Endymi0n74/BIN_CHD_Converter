#!/usr/bin/env bash
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/bin" "$tmp/roms"
cat >"$tmp/bin/chdman" <<'EOF'
#!/usr/bin/env bash
out=""
while (($#)); do [[ "$1" == "-o" ]] && { shift; out="$1"; }; shift; done
sleep 0.1
[[ -z "$out" ]] || : >"$out"
EOF
chmod +x "$tmp/bin/chdman"
: >"$tmp/roms/game.iso"
: >"$tmp/roms/game.img"
PATH="$tmp/bin:$PATH" "$repo/batchconverttochd.sh" convert "$tmp/roms" "$tmp/roms" -j 2
test -f "$tmp/roms/game.chd"
test -f "$tmp/roms/game (1).chd"
test -z "$(find "$tmp" -name '*.batchconvert.lock' -print -quit)"
echo "unix CLI tests passed"
