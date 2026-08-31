#!/bin/bash

set -euo pipefail

template_path=${TEMPLATE_PATH:-serverless.yaml}
upstream_path=${UPSTREAM_VERSION_PATH:-.upstream-version}

mapping_version() {
    local mapping=$1
    awk -v mapping="$mapping" '
        $1 == mapping ":" { in_mapping = 1; next }
        in_mapping && $1 == "Version:" { print $2; exit }
        in_mapping && $0 ~ /^[^[:space:]]/ { exit }
    ' "$template_path"
}

case "$#" in
    0)
        merged_upstream=$(tr -d '[:space:]' < "$upstream_path")
        expected_upstream=""
        expected_layer=""
        ;;
    2)
        merged_upstream=$(tr -d '[:space:]' < "$upstream_path")
        expected_upstream=$1
        expected_layer=$2
        ;;
    *)
        echo "Usage: $0 [UPSTREAM_VERSION LAYER_VERSION]" >&2
        exit 2
        ;;
esac

template_upstream=$(mapping_version TeroExtensionLayerUpstream)
template_layer=$(mapping_version TeroExtensionLayerVersion)

if [[ ! "$merged_upstream" =~ ^[0-9]+$ ]] \
    || [[ ! "$template_upstream" =~ ^[0-9]+$ ]] \
    || [[ ! "$template_layer" =~ ^[1-9][0-9]*$ ]]; then
    echo "Merged, template upstream, and layer versions must be positive integers." >&2
    exit 2
fi

if [ "$template_upstream" -gt "$merged_upstream" ]; then
    echo "serverless.yaml targets upstream v$template_upstream, but main contains only v$merged_upstream." >&2
    exit 1
fi

if [ -n "$expected_upstream" ]; then
    if [[ ! "$expected_upstream" =~ ^[0-9]+$ ]] || [[ ! "$expected_layer" =~ ^[1-9][0-9]*$ ]]; then
        echo "Expected upstream and layer versions must be positive integers." >&2
        exit 2
    fi
    if [ "$template_upstream" != "$expected_upstream" ]; then
        echo "serverless.yaml targets upstream v$template_upstream, expected v$expected_upstream." >&2
        exit 1
    fi
    if [ "$template_layer" != "$expected_layer" ]; then
        echo "serverless.yaml targets layer version $template_layer, expected $expected_layer." >&2
        exit 1
    fi
fi

prod_reference="layer:Tero-Datadog-Extension-\${UpstreamVersion}-ARM:\${LayerVersion}"
dev_reference="layer:Tero-Datadog-Extension-\${UpstreamVersion}-ARM-dev:\${LayerVersion}"

if ! grep -Fq "$prod_reference" "$template_path"; then
    echo "serverless.yaml does not use the versioned production ARM layer name." >&2
    exit 1
fi

if ! grep -Fq "$dev_reference" "$template_path"; then
    echo "serverless.yaml does not use the versioned development ARM layer name." >&2
    exit 1
fi

echo "serverless.yaml targets Tero Datadog extension v${template_upstream}, layer version ${template_layer}; main contains v${merged_upstream}."
