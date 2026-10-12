#!/usr/bin/env bash
# Builds vmangos with the Skatecraft and independently negotiated Tarnished extensions.
# setup/setup.sh runs this for server/; for another vmangos-deploy checkout:
#   setup/build-server.sh ~/path/to/vmangos-deploy && docker compose -f ~/path/to/vmangos-deploy/compose.yaml up -d
# The first build compiles vmangos (a few minutes). SKATECRAFT_BUILD_JOBS sets its parallelism
# (default 4, so the PC stays usable).
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
DEPLOY=$(cd "${1:-$ROOT/server}" && pwd)
PATCH="$ROOT/setup/vmangos/skatecraft.patch"
TARNISHED_PATCH="$ROOT/setup/vmangos/tarnished.patch"
# The vmangos commit the patch is made against.
VMANGOS_REV=0e3ff01e76d4758e8a7c3108b2717cc785ed56fa
JOBS=${SKATECRAFT_BUILD_JOBS:-4}
# Include the pinned core and both ordered patches in the cache identity.
IMAGE="world-of-skatecraft/vmangos-server:5875-$( { printf '%s\n' "$VMANGOS_REV"; sha256sum "$PATCH" "$TARNISHED_PATCH" | cut -d' ' -f1; } | sha256sum | cut -c1-12)"
[[ "$JOBS" =~ ^[1-9][0-9]*$ ]] || { echo "SKATECRAFT_BUILD_JOBS must be a positive integer"; exit 1; }

[ -f "$DEPLOY/compose.yaml" ] || { echo "no compose.yaml in $DEPLOY"; exit 1; }
if ! docker image inspect "$IMAGE" >/dev/null 2>&1; then
    echo "Building $IMAGE (vmangos $VMANGOS_REV with Skatecraft and Tarnished, $JOBS jobs)"
    mkdir -p "$ROOT/.setup"
    CONTEXT="$ROOT/.setup/server-build-$$"
    mkdir "$CONTEXT"
    trap 'rm -rf "$CONTEXT"' EXIT
    cp -r "$DEPLOY/docker" "$CONTEXT/"
    cp "$PATCH" "$CONTEXT/docker/patches/zz-skatecraft.patch"
    cp "$TARNISHED_PATCH" "$CONTEXT/docker/patches/zzz-tarnished.patch"
    sed "s/make -j\"\$(nproc)\"/make -j$JOBS/" "$CONTEXT/docker/server/Dockerfile" > "$CONTEXT/Dockerfile.skatecraft"
    docker build -t "$IMAGE" -f "$CONTEXT/Dockerfile.skatecraft" \
        --build-arg VMANGOS_REVISION="$VMANGOS_REV" \
        --build-arg VMANGOS_CLIENT_VERSION=5875 \
        --build-arg VMANGOS_FAIL_ON_PATCH_ERROR=1 \
        "$CONTEXT"
fi
sed -i -E "s#image: (ghcr\.io/mserajnik/vmangos-server:5875|world-of-skatecraft/vmangos-server:[^[:space:]]+)#image: $IMAGE#" \
    "$DEPLOY/compose.yaml"
echo "$DEPLOY/compose.yaml now runs $IMAGE"
