#!/usr/bin/env bash
#
# Parse a release version into its upstream and patch parts.
#
# Versioning tracks DataDog's upstream releases: the upstream version goes in the
# layer name (Tero-Datadog-Extension-119) and the AWS layer version integer is
# the patch. See .github/workflows/release-extension.yml.
#
#   119    -> upstream 119, patch "" (take the next version AWS assigns)
#   119.2  -> upstream 119, patch 2  (assert it lands on 2 before publishing)
#
# A leading "v" is optional, so a tag ref can be passed straight through.
#
# Usage:  parse_release_version.sh v119.2   # prints "119 2"
#         parse_release_version.sh --self-test
set -euo pipefail

parse_release_version() {
    local raw="${1#v}"

    if [[ ! "$raw" =~ ^([0-9]+)(\.([0-9]+))?$ ]]; then
        echo "version '$1' is not <upstream> or <upstream>.<patch>" >&2
        return 1
    fi

    local upstream="${BASH_REMATCH[1]}" patch="${BASH_REMATCH[3]:-}"

    # AWS numbers layer versions from 1, so patch 0 can never be published.
    if [ -n "$patch" ] && [ "$patch" -lt 1 ]; then
        echo "version '$1' has patch $patch; patches start at 1" >&2
        return 1
    fi

    echo "$upstream $patch"
}

self_test() {
    local failures=0

    check_ok() { # $1 = input, $2 = expected output
        local got
        if ! got="$(parse_release_version "$1" 2>/dev/null)"; then
            echo "FAIL: '$1' was rejected, expected '$2'"
            failures=$((failures + 1))
            return
        fi
        if [ "$got" != "$2" ]; then
            echo "FAIL: '$1' gave '$got', expected '$2'"
            failures=$((failures + 1))
        fi
    }

    check_rejected() { # $1 = input
        if parse_release_version "$1" >/dev/null 2>&1; then
            echo "FAIL: '$1' was accepted, expected rejection"
            failures=$((failures + 1))
        fi
    }

    check_ok v119 "119 "
    check_ok 119 "119 "
    check_ok v119.2 "119 2"
    check_ok 119.10 "119 10"
    check_ok v100.1 "100 1"

    check_rejected v119.0   # patches start at 1
    check_rejected v119.
    check_rejected v1.2.3
    check_rejected vabc
    check_rejected ""
    check_rejected v-1
    check_rejected "119 2"

    if [ "$failures" -ne 0 ]; then
        echo "$failures check(s) failed"
        return 1
    fi
    echo "all checks passed"
}

if [ "${1:-}" = "--self-test" ]; then
    self_test
else
    parse_release_version "${1:-}"
fi
