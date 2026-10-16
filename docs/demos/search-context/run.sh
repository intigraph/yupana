#!/bin/sh
set -eu
cd "$(dirname "$0")/../../.."
printf 'Candidate source: '
git rev-parse HEAD
log=$(mktemp)
output=$(mktemp)
trap 'rm -f "$log" "$output"' EXIT
cargo test --features quipu --lib brief_sources::search_context -- --nocapture >"$output" 2>"$log" || { cat "$output" "$log"; exit 1; }
cat "$output"
grep -Eq 'test result: ok\. [1-9][0-9]* passed' "$output"
