#!/usr/bin/env bash

set -euo pipefail

if [[ "${CREATE_DEB_DEBUG:-}" == "1" ]]; then
  set -x
fi

declare -a COMMANDS=(
  curl
  unzip
  mktemp
  find
)

for cmd in "${COMMANDS[@]}"; do
  if ! command -v "$cmd" &> /dev/null; then
    echo "$cmd could not be found" >&2
    exit 1
  fi
done

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <gitea-run-number> <git-sha>" >&2
    echo "Example:" >&2
    echo "  GITEA_USER=me GITEA_TOKEN=secret ./scripts/download-and-upload-debs.sh 123 \$(git rev-parse HEAD)" >&2
    exit 1
fi

if [ -z "${GITEA_USER:-}" ]; then
    echo "GITEA_USER is not set" >&2
    exit 1
fi

if [ -z "${GITEA_TOKEN:-}" ]; then
    echo "GITEA_TOKEN is not set" >&2
    exit 1
fi

declare -r RUN_NUMBER="$1"
declare -r GIT_SHA="$2"

TMPDIR="$(mktemp -d)"

for variant in debian-bookworm debian-trixie ubuntu-jammy ubuntu-noble ubuntu-resolute; do
    echo "Downloading and uploading debs for variant: $variant"
    curl "https://git.pvv.ntnu.no/Projects/muscl/actions/runs/$RUN_NUMBER/artifacts/muscl-deb-$variant-$GIT_SHA.zip" --output "$TMPDIR/muscl-deb-$variant-$GIT_SHA.zip"

    unzip "$TMPDIR/muscl-deb-$variant-$GIT_SHA.zip" -d "$TMPDIR/muscl-deb-$variant-$GIT_SHA"

    DISTRO_VERSION_NAME="$(echo "$variant" | cut -d'-' -f2)"

    DEB_NAME=$(find "$TMPDIR/muscl-deb-$variant-$GIT_SHA"/*.deb -print0 | xargs -0 -n1 basename | cut -d'_' -f1 | head -n1)
    DEB_VERSION=$(find "$TMPDIR/muscl-deb-$variant-$GIT_SHA"/*.deb -print0 | xargs -0 -n1 basename | cut -d'_' -f2 | head -n1)
    DEB_ARCH=$(find "$TMPDIR/muscl-deb-$variant-$GIT_SHA"/*.deb -print0 | xargs -0 -n1 basename | cut -d'_' -f3 | cut -d'.' -f1 | head -n1)

    # echo "[DELETE] https://git.pvv.ntnu.no/api/packages/Projects/debian/pool/$DISTRO_VERSION_NAME/main/$DEB_NAME/$DEB_VERSION/$DEB_ARCH"
    # curl \
    #   -X DELETE \
    #   --user "$GITEA_USER:$GITEA_TOKEN" \
    #   "https://git.pvv.ntnu.no/api/packages/Projects/debian/pool/$DISTRO_VERSION_NAME/main/$DEB_NAME/$DEB_VERSION/$DEB_ARCH"

    echo "[PUT] https://git.pvv.ntnu.no/api/packages/Projects/debian/pool/$DISTRO_VERSION_NAME/main/upload"
    curl \
      -X PUT \
      --user "$GITEA_USER:$GITEA_TOKEN" \
      --upload-file "$TMPDIR/muscl-deb-$variant-$GIT_SHA/${DEB_NAME}_${DEB_VERSION}_${DEB_ARCH}.deb" \
      "https://git.pvv.ntnu.no/api/packages/Projects/debian/pool/$DISTRO_VERSION_NAME/main/upload"
done

rm -rf "$TMPDIR"
