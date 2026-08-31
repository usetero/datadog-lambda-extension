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
        upstream_version=$(tr -d '[:space:]' < "$upstream_path")
        layer_version=$(mapping_version TeroExtensionLayerVersion)
        ;;
    2)
        upstream_version=$1
        layer_version=$2
        ;;
    *)
        echo "Usage: $0 [UPSTREAM_VERSION LAYER_VERSION]" >&2
        exit 2
        ;;
esac

if [[ ! "$upstream_version" =~ ^[0-9]+$ ]] || [[ ! "$layer_version" =~ ^[1-9][0-9]*$ ]]; then
    echo "Upstream and layer versions must be positive integers." >&2
    exit 2
fi

template_upstream=$(mapping_version TeroExtensionLayerUpstream)
template_layer=$(mapping_version TeroExtensionLayerVersion)

if [ "$template_upstream" != "$upstream_version" ]; then
    echo "serverless.yaml targets upstream v${template_upstream:-<missing>}, expected v${upstream_version}." >&2
    exit 1
fi

if [ "$template_layer" != "$layer_version" ]; then
    echo "serverless.yaml targets layer version ${template_layer:-<missing>}, expected ${layer_version}." >&2
    exit 1
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

echo "serverless.yaml targets Tero Datadog extension v${upstream_version}, layer version ${layer_version}."
