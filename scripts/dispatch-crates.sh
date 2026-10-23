#!/usr/bin/env bash
# Dispatch only; credentials and registry publication stay in crates.yml.
set -euo pipefail
if [[ ! ${RELEASE_TAG:-} =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "::error::expected a stable vMAJOR.MINOR.PATCH release tag" >&2
  exit 1
fi
case "${DRY_RUN:-}" in
  true|false) ;;
  *) echo "::error::DRY_RUN must be true or false" >&2; exit 1 ;;
esac
gh workflow run crates.yml --ref "$RELEASE_TAG" \
  -f "version=${RELEASE_TAG#v}" -f "dry_run=$DRY_RUN"
