# Synthetic compiler index fixture

`src/lib.rs` and `Cargo.toml` are dependency-free synthetic source. They contain
two distinct `duplicate` definitions and one reference to each. No project or
homelab source is indexed.

Generated using `rust-analyzer 1.98.0 (88d9e12 2026-08-18)`:

```bash
fixture_dir=$(mktemp -d)
cp -r tests/fixtures/scip/Cargo.toml tests/fixtures/scip/src "$fixture_dir/"
CARGO_NET_OFFLINE=true rust-analyzer scip "$fixture_dir" \
  --output /tmp/synthetic-index.scip --exclude-vendored-libraries --num-threads 1
SCIP_FIXTURE_RAW=/tmp/synthetic-index.scip \
  just test --test scip_import normalize_compiler_fixture -- --ignored
```

The explicit normalizer only replaces the producer's absolute project-root URI
with `file:///offline-scip-fixture`. Definitions, references, producer version,
source positions and symbol identities remain compiler-produced. The regular
suite uses this checked-in artifact without running an indexer. Its temporary
Git repository and source-byte binding are created per test.

Source and generated fixture are distributed under this repository's Apache-2.0
license. The SCIP wire implementation is provided by the upstream `scip` crate
(Apache-2.0), rather than copied generated code.
