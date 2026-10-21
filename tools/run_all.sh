#!/usr/bin/env bash
# Classify every skin folder under DIR (one classifier process per skin), writing OUT/<folder>.json.
# Usage: tools/run_all.sh DIR OUT [extra classifier args...]
set -euo pipefail
dir=$1 out=$2
shift 2
mkdir -p "$out"
for d in "$dir"/*/; do
  name=$(basename "$d")
  mania-skin-classify "$d" -o "$out/$name.json" "$@" || echo "FAILED: $d" >&2
done
