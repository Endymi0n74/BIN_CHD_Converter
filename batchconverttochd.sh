#!/usr/bin/env bash
set -uo pipefail

VERSION="1.0.0"

usage() {
  cat <<'EOF'
Batch Convert to CHD — macOS/Linux
Usage:
  batchconverttochd convert INPUT OUTPUT [-r] [-j N] [--force-cd|--force-dvd] [--delete-source] [-n]
  batchconverttochd extract INPUT OUTPUT [-r] [-j N] [--format auto|cd|dvd|gdi|hd] [--delete-source] [-n]
  batchconverttochd verify INPUT [-r] [-j N] [-n]
EOF
}
die() { printf 'Error: %s\n' "$*" >&2; exit 1; }
ACTION="${1:-}"; [[ "$ACTION" == --version ]] && { echo "$VERSION"; exit 0; }
[[ -z "$ACTION" || "$ACTION" == --help || "$ACTION" == -h ]] && { usage; exit 0; }
[[ "$ACTION" =~ ^(convert|extract|verify)$ ]] || die "unknown action: $ACTION"; shift
INPUT="${1:-}"; [[ -d "$INPUT" ]] || die "invalid input folder: $INPUT"; shift
OUTPUT=""; if [[ "$ACTION" != verify ]]; then OUTPUT="${1:-}"; [[ -n "$OUTPUT" ]] || die "output folder required"; shift; fi
RECURSIVE=0 DELETE=0 FORCE_CD=0 FORCE_DVD=0 DRY=0 JOBS=1 FORMAT=auto
while (($#)); do
  case "$1" in
    -r|--recursive) RECURSIVE=1;; --delete-source) DELETE=1;; --force-cd) FORCE_CD=1;; --force-dvd) FORCE_DVD=1;;
    --format) shift; FORMAT="${1:-}";; -j|--jobs) shift; JOBS="${1:-}";; -n|--dry-run) DRY=1;; -h|--help) usage; exit;; *) die "unknown option: $1";;
  esac; shift
done
command -v chdman >/dev/null || die "chdman not found in PATH; install MAME first"
[[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || die "jobs must be a positive integer"
[[ "$FORMAT" =~ ^(auto|cd|dvd|gdi|hd)$ ]] || die "invalid format: $FORMAT"
((FORCE_CD && FORCE_DVD)) && die "force options are mutually exclusive"
[[ -z "$OUTPUT" ]] || mkdir -p "$OUTPUT"
run() { if ((DRY)); then printf '>'; printf ' %q' "$@"; printf '\n'; else "$@"; fi; }
relative() { printf '%s' "${1#"${INPUT%/}/"}"; }
claim_path() {
  local desired="$1" directory filename stem extension suffix candidate
  directory="$(dirname "$desired")"; filename="$(basename "$desired")"
  if [[ "$filename" == *.* ]]; then stem="${filename%.*}"; extension=".${filename##*.}"; else stem="$filename"; extension=""; fi
  suffix=0
  while :; do
    if ((suffix == 0)); then candidate="$desired"; else candidate="$directory/$stem ($suffix)$extension"; fi
    if [[ ! -e "$candidate" ]] && mkdir "$candidate.batchconvert.lock" 2>/dev/null; then printf '%s' "$candidate"; return; fi
    suffix=$((suffix + 1))
  done
}
convert_one() {
  local f="$1" rel ext base cmd desired out; rel="$(relative "$f")"; ext="$(printf '%s' "${f##*.}" | tr '[:upper:]' '[:lower:]')"; base="${rel%.*}"; desired="$OUTPUT/$base.chd"; mkdir -p "$(dirname "$desired")"; out="$(claim_path "$desired")"
  case "$ext" in cue|gdi|toc) cmd=createcd;; iso) cmd=createdvd;; img) cmd=createhd;; raw) cmd=createraw;; *) return;; esac
  ((FORCE_CD)) && cmd=createcd; ((FORCE_DVD)) && cmd=createdvd; echo "CONVERT $rel -> ${out#"$OUTPUT/"}"
  if run chdman "$cmd" -i "$f" -o "$out"; then rmdir "$out.batchconvert.lock" 2>/dev/null || true; ((DELETE && !DRY)) && rm -f -- "$f"; return 0; else rmdir "$out.batchconvert.lock" 2>/dev/null || true; rm -f -- "$out"; return 1; fi
}
extract_one() {
  local f="$1" rel base cmd suffix info desired out; rel="$(relative "$f")"; base="${rel%.*}"; mkdir -p "$OUTPUT/$(dirname "$base")"
  case "$FORMAT" in cd) cmd=extractcd; suffix=cue;; dvd) cmd=extractdvd; suffix=iso;; gdi) cmd=extractcd; suffix=gdi;; hd) cmd=extracthd; suffix=img;;
    auto) info="$(chdman info -i "$f" 2>/dev/null || true)"; if grep -Eqi 'GDDD|GD-ROM' <<<"$info"; then cmd=extractcd; suffix=gdi; elif grep -qi CD-ROM <<<"$info"; then cmd=extractcd; suffix=cue; elif grep -qi DVD-ROM <<<"$info"; then cmd=extractdvd; suffix=iso; else cmd=extracthd; suffix=img; fi;; esac
  desired="$OUTPUT/$base.$suffix"; out="$(claim_path "$desired")"
  echo "EXTRACT $rel -> ${out#"$OUTPUT/"}"; if run chdman "$cmd" -i "$f" -o "$out"; then rmdir "$out.batchconvert.lock" 2>/dev/null || true; ((DELETE && !DRY)) && rm -f -- "$f"; return 0; else rmdir "$out.batchconvert.lock" 2>/dev/null || true; return 1; fi
}
verify_one() { echo "VERIFY $(relative "$1")"; run chdman verify -i "$1"; }
export INPUT OUTPUT FORMAT FORCE_CD FORCE_DVD DELETE DRY; export -f run relative claim_path convert_one extract_one verify_one
args=("$INPUT"); ((RECURSIVE)) || args+=(-maxdepth 1); [[ "$ACTION" == convert ]] && pattern='\.(cue|gdi|toc|iso|img|raw)$' || pattern='\.chd$'; worker="${ACTION}_one"
failed="$(mktemp)"; trap 'rm -f "$failed"' EXIT; export failed
find "${args[@]}" -type f -print0 | while IFS= read -r -d '' f; do lower="$(printf '%s' "$f" | tr '[:upper:]' '[:lower:]')"; [[ "$lower" =~ $pattern ]] && printf '%s\0' "$f"; done | xargs -0 -n1 -P "$JOBS" bash -c '[[ -z "${1:-}" ]] || "$0" "$1" || echo 1 >>"$failed"' "$worker"
[[ ! -s "$failed" ]] || die "one or more operations failed"; echo Done.
