#!/usr/bin/env bash
# The publish job fetched the live tag into FETCH_HEAD and refreshed origin/main.
set -euo pipefail
tag_commit=$(git rev-parse 'FETCH_HEAD^{commit}')
test "$tag_commit" = "$(git rev-parse HEAD)"
test "$tag_commit" = "$(git rev-parse "$GITHUB_SHA^{commit}")"
git merge-base --is-ancestor "$tag_commit" refs/remotes/origin/main
version=$(tar -xOf "$1" package/package.json | jq -er 'select(.name == "@phni3j9a/pimate").version')
test "$GITHUB_REF_NAME" = "host-v$version"
test "$version" = "$(jq -er '.version' packages/pimate/package.json)"
cargo metadata --locked --no-deps --format-version 1 | jq -e --arg v "$version" 'any(.packages[]; .name == "pimate-host" and .version == $v)' >/dev/null
