#!/bin/sh
# Spec 005 acceptance: seeded defects in the rules backend's identity,
# accounting, cancellation and validation must each be detected by its
# tests. Works on a temporary copy; see seeds.py.
set -eu
exec python3 "$(dirname "$0")/seeds.py" "$@"
