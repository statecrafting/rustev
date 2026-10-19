#!/bin/sh
# Spec 032 acceptance: seeded defects in evidence quality must each be
# detected by the tests. Works on a temporary copy; see seeds.py.
set -eu
exec python3 "$(dirname "$0")/seeds.py" "$@"
