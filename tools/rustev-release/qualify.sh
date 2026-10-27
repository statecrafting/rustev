#!/bin/sh
# Spec 056 3.3: fresh public-registry consumers, never publication.
set -eu
[ "$#" -eq 3 ] || { echo 'usage: qualify.sh <version> <manifest> <evidence-dir>' >&2; exit 2; }
case "$1" in *[!0-9.]*) echo 'qualify: malformed version' >&2; exit 2 ;; esac
# Stable family releases only. Reject shell syntax, ranges and leading zeroes.
printf '%s\n' "$1" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || {
  echo 'qualify: malformed version' >&2; exit 2;
}
script=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
command -v python3 >/dev/null || { echo 'qualify: Python 3.11 or later required' >&2; exit 2; }
work=$(mktemp -d "${TMPDIR:-/tmp}/rustev-qualify.XXXXXX")
trap 'rm -rf "$work"' 0
trap 'exit 1' HUP INT TERM
python3 "$script/qualify/harness.py" "$1" "$2" "$3" "$work"
