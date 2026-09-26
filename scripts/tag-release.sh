#!/usr/bin/env bash
# Creates and pushes the vX.Y.Z tag for the version in package.json. Used as the changesets
# "publish" command, which runs on every push to main without pending changesets. If the tag
# already exists on origin, that version was already released and there is nothing to do.
#
# Tags pushed with the workflow GITHUB_TOKEN do not trigger other workflows, so when
# RELEASE_DISPATCH=1 the script also starts release.yml on the tag via workflow_dispatch
# (requires `gh` and GH_TOKEN with actions:write).
set -euo pipefail

version="$(node -p "require('./package.json').version")"
tag="v${version}"
head_sha="$(git rev-parse HEAD)"

# --exit-code: 0 = tag found, 2 = no such tag, anything else = the lookup itself failed.
rc=0
remote_ref="$(git ls-remote --exit-code --tags origin "refs/tags/${tag}")" || rc=$?
case "$rc" in
  0)
    echo "${tag} was already released (origin tag ${remote_ref%%[[:space:]]*}); nothing to tag."
    exit 0
    ;;
  2) ;;
  *)
    echo "error: could not query origin for ${tag} (git ls-remote exit ${rc})" >&2
    exit 1
    ;;
esac

echo "Creating tag ${tag} at ${head_sha}"
git tag --annotate "$tag" --message "gws-rust ${tag}"
git push origin "refs/tags/${tag}"

if [[ "${RELEASE_DISPATCH:-0}" == 1 ]]; then
  echo "Dispatching release.yml for ${tag}"
  gh workflow run release.yml --ref "$tag"
fi

# changesets/action parses this line to report what was released.
echo "New tag: ${tag}"
