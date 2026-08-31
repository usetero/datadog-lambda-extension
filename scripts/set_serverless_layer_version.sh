#!/bin/bash

set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage: $0 UPSTREAM_VERSION LAYER_VERSION" >&2
    exit 2
fi

upstream_version=$1
layer_version=$2
template_path=${TEMPLATE_PATH:-serverless.yaml}

mapping_version() {
    local mapping=$1
    awk -v mapping="$mapping" '
        $1 == mapping ":" { in_mapping = 1; next }
        in_mapping && $1 == "Version:" { print $2; exit }
        in_mapping && $0 ~ /^[^[:space:]]/ { exit }
    ' "$template_path"
}

if [[ ! "$upstream_version" =~ ^[0-9]+$ ]] || [[ ! "$layer_version" =~ ^[1-9][0-9]*$ ]]; then
    echo "Upstream and layer versions must be positive integers." >&2
    exit 2
fi

current_upstream=$(mapping_version TeroExtensionLayerUpstream)
current_layer=$(mapping_version TeroExtensionLayerVersion)
if [ "$upstream_version" -lt "$current_upstream" ] \
    || { [ "$upstream_version" -eq "$current_upstream" ] && [ "$layer_version" -lt "$current_layer" ]; }; then
    echo "Refusing to move serverless.yaml backward from v${current_upstream}:${current_layer} to v${upstream_version}:${layer_version}." >&2
    exit 1
fi

UPSTREAM_VERSION=$upstream_version LAYER_VERSION=$layer_version perl -0pi -e '
    $upstream_updates += s/(^\s+TeroExtensionLayerUpstream:\s*\n\s+Version:\s*)\d+/${1}$ENV{UPSTREAM_VERSION}/m;
    $layer_updates += s/(^\s+TeroExtensionLayerVersion:\s*\n\s+Version:\s*)\d+/${1}$ENV{LAYER_VERSION}/m;
    END {
        die "Expected exactly one upstream and layer version mapping\n"
            unless $upstream_updates == 1 && $layer_updates == 1;
    }
' "$template_path"

TEMPLATE_PATH=$template_path "$(dirname "$0")/verify_serverless_layer.sh" \
    "$upstream_version" "$layer_version"
