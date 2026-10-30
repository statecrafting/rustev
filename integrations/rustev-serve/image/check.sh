#!/bin/sh
# Checks a built rustev-serve image against spec 042 3.16 for one platform:
# non-root user and group 65532, the fixed entrypoint, no shell, and a binary
# that starts on a read-only root filesystem. Usage:
#   integrations/rustev-serve/image/check.sh <image-ref> <linux/amd64|linux/arm64>
set -eu
fail() { echo "check.sh: $*" >&2; exit 1; }
test "$#" = 2 || fail 'usage: check.sh <image-ref> <linux/amd64|linux/arm64>'
image=$1 platform=$2
case "$platform" in linux/amd64|linux/arm64) ;; *) fail "unsupported platform $platform" ;; esac

got=$(docker image inspect --format '{{.Os}}/{{.Architecture}}' "$image")
test "$got" = "$platform" || fail "platform is $got, expected $platform"
got=$(docker image inspect --format '{{.Config.User}}' "$image")
test "$got" = '65532:65532' || fail "user is '$got', expected 65532:65532"
got=$(docker image inspect --format '{{json .Config.Entrypoint}}' "$image")
test "$got" = '["/usr/local/bin/rustev-serve","--config","/etc/rustev/serve.json"]' \
  || fail "entrypoint is $got"
got=$(docker image inspect --format '{{json .Config.Cmd}}' "$image")
test "$got" = 'null' || test "$got" = '[]' || fail "cmd is $got, expected none"

# No shell: starting one must fail.
for sh in /bin/sh /bin/bash /busybox/sh; do
  if docker run --rm --platform "$platform" --read-only --entrypoint "$sh" "$image" -c true >/dev/null 2>&1; then
    fail "the image has a shell at $sh"
  fi
done

# The binary runs as 65532 on a read-only root filesystem: without its
# configuration file it reports the missing file and exits 3.
set +e
out=$(docker run --rm --platform "$platform" --read-only "$image" 2>&1)
status=$?
set -e
test "$status" = 3 || fail "missing configuration exited $status, expected 3: $out"
case "$out" in "rustev-serve: "*) ;; *) fail "unexpected startup output: $out" ;; esac
usage=$(docker run --rm --platform "$platform" --read-only --entrypoint /usr/local/bin/rustev-serve "$image" 2>&1 || true)
case "$usage" in *"usage: rustev-serve --config FILE"*) ;; *) fail "binary usage output: $usage" ;; esac
echo "check.sh: $image ($platform) passes"
