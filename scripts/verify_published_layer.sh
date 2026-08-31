#!/bin/bash

set -euo pipefail

if [ "$#" -lt 4 ] || [ "$#" -gt 5 ]; then
    echo "Usage: $0 REGION LAYER_NAME LAYER_VERSION ARCHITECTURE [CODE_SHA256]" >&2
    exit 2
fi

region=$1
layer_name=$2
layer_version=$3
architecture=$4
expected_code_sha256=${5:-}

case "$architecture" in
    x86_64|arm64) ;;
    *)
        echo "Unsupported architecture: $architecture" >&2
        exit 2
        ;;
esac

if [[ ! "$layer_version" =~ ^[1-9][0-9]*$ ]]; then
    echo "Layer version must be a positive integer." >&2
    exit 2
fi

layer=$(aws lambda get-layer-version \
    --region "$region" \
    --layer-name "$layer_name" \
    --version-number "$layer_version" \
    --output json)

layer_arn=$(jq -r '.LayerVersionArn' <<< "$layer")
if ! jq -e --arg architecture "$architecture" \
    '(.CompatibleArchitectures // []) | index($architecture) != null' \
    <<< "$layer" >/dev/null; then
    echo "$layer_arn is not compatible with $architecture." >&2
    exit 1
fi

if [ -n "$expected_code_sha256" ]; then
    code_sha256=$(jq -r '.Content.CodeSha256' <<< "$layer")
    if [ "$code_sha256" != "$expected_code_sha256" ]; then
        echo "$layer_arn has code hash $code_sha256, expected $expected_code_sha256." >&2
        exit 1
    fi
fi

policy=$(aws lambda get-layer-version-policy \
    --region "$region" \
    --layer-name "$layer_name" \
    --version-number "$layer_version" \
    --query Policy \
    --output text)

if ! jq -e --arg resource "$layer_arn" '
    def allows_action($wanted):
        if type == "array" then index($wanted) != null else . == $wanted end;
    def is_public:
        . == "*" or (type == "object" and .AWS == "*");
    any(.Statement[];
        .Effect == "Allow"
        and (.Action | allows_action("lambda:GetLayerVersion"))
        and (.Principal | is_public)
        and .Resource == $resource)
' <<< "$policy" >/dev/null; then
    echo "$layer_arn does not have public lambda:GetLayerVersion access." >&2
    exit 1
fi

echo "Verified public $architecture layer: $layer_arn"
