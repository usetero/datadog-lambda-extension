#!/bin/bash

set -uo pipefail

if [ "$#" -lt 2 ] || [ "$#" -gt 3 ]; then
    echo "Usage: $0 UPSTREAM_VERSION LAYER_VERSION [DEV_SUFFIX]" >&2
    exit 2
fi

upstream_version=$1
layer_version=$2
dev_suffix=${3:-}
script_dir=$(cd "$(dirname "$0")" && pwd)
regions_path=${LAYER_REGIONS_PATH:-"$script_dir/../.lambda-layer-regions"}

if [[ ! "$upstream_version" =~ ^[0-9]+$ ]] || [[ ! "$layer_version" =~ ^[1-9][0-9]*$ ]]; then
    echo "Upstream and layer versions must be positive integers." >&2
    exit 2
fi

if [ -n "$dev_suffix" ] && [ "$dev_suffix" != "-dev" ]; then
    echo "DEV_SUFFIX must be empty or -dev." >&2
    exit 2
fi

regions=()
if [ -n "${REGIONS:-}" ]; then
    IFS=',' read -r -a regions <<< "$REGIONS"
else
    while IFS= read -r region; do
        [ -n "$region" ] && regions+=("$region")
    done < "$regions_path"
fi

failures=0
for raw_region in "${regions[@]}"; do
    region=${raw_region//[[:space:]]/}
    for architecture in x86_64 arm64; do
        arch_suffix=""
        [ "$architecture" = "arm64" ] && arch_suffix="-ARM"
        layer_name="Tero-Datadog-Extension-${upstream_version}${arch_suffix}${dev_suffix}"

        if ! "$script_dir/verify_published_layer.sh" \
            "$region" "$layer_name" "$layer_version" "$architecture"; then
            failures=$((failures + 1))
        fi
    done
done

if [ "$failures" -ne 0 ]; then
    echo "$failures layer target(s) failed verification." >&2
    exit 1
fi

echo "Verified Tero Datadog extension v${upstream_version}:${layer_version}${dev_suffix} in ${#regions[@]} regions."
