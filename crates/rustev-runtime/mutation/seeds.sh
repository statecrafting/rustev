#!/bin/sh
# Spec 003 acceptance: seeded defects in the runtime's guarantees must each
# be detected by the tests. Works on a temporary copy; see seeds.py.
set -eu
exec python3 "$(dirname "$0")/seeds.py" "$@"
