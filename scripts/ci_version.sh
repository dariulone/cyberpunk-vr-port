#!/usr/bin/env bash
#
# The only thing that computes a version. X.Y.Z comes from VERSION; tags are bare, no `v`.
#
#   feature branch / PR   X.Y.Z-dev.<sha>     0.1.6-dev.a1b2c3d
#   main                  X.Y.Z-rc.N.<sha>    0.1.6-rc.4.a1b2c3d   N = commits since VERSION changed
#   tag <semver>          <semver>            0.1.6
#
#   scripts/ci_version.sh [<ref-name> <ref-type> <sha>]   -> key=value lines for $GITHUB_OUTPUT

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

ref_name=${1:-${VERSION_REF_NAME:-${GITHUB_REF_NAME:-$(git rev-parse --abbrev-ref HEAD)}}}
ref_type=${2:-${VERSION_REF_TYPE:-${GITHUB_REF_TYPE:-branch}}}
sha=${3:-${VERSION_SHA:-${GITHUB_SHA:-$(git rev-parse HEAD)}}}

default_branch=${VERSION_DEFAULT_BRANCH:-main}

if [[ ! -f VERSION ]]; then
    echo "VERSION file not found at $repo_root/VERSION" >&2
    exit 1
fi
base=$(tr -d '[:space:]' < VERSION)
if [[ ! $base =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "VERSION must contain a bare X.Y.Z, got: '$base'" >&2
    exit 1
fi

short=$(git rev-parse --short=7 "$sha")
# An all-digit pre-release identifier may not have a leading zero; ~1 short sha in 270 does.
if [[ $short =~ ^[0-9]+$ ]]; then
    short="g$short"
fi

case "$ref_type" in
tag)
    # `v` tolerated on the way in, never emitted.
    tag=${ref_name#v}
    if [[ ! $tag =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
        echo "tag '$ref_name' is not a semver tag" >&2
        exit 1
    fi
    version=$tag
    channel=release
    ;;
*)
    if [[ $ref_name == "$default_branch" ]]; then
        # The bump commit is itself rc.1, so a fresh X.Y.Z never starts at rc.0.
        bump=$(git log -1 --format=%H -- VERSION || true)
        if [[ -n $bump ]]; then
            n=$(git rev-list --count "$bump..$sha")
        else
            n=$(git rev-list --count "$sha")
        fi
        # The sha rides along: force-push main and the same N is re-derived for another commit.
        version="$base-rc.$((n + 1)).$short"
        channel=rc
    else
        version="$base-dev.$short"
        channel=dev
    fi
    ;;
esac

echo "version=$version"
echo "base=$base"
echo "channel=$channel"
echo "short_sha=$short"
echo "tag=$version"
echo "artifact=CyberpunkVRPort-$version"
