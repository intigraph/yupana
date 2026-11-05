# Former-name contracts

Current product names are Yupana and Tambo. This is the per-path allowlist for
remaining former names, including the explicit historical filenames below and
this ledger. Scan all tracked readable files case-insensitively; match the former
structural tool name as a whole word. New current-product uses are not allowed.

| Path | Reason |
| --- | --- |
| `CHANGELOG.md` | Published change titles retain their original wording. |
| `docs/rename-from-hank.md` | Explicit rename note and dated migration inventory. |
| `docs/book/src/docs-map.md` | Links the explicit rename note and explains the legacy alias. |
| `docs/RELEASING.md` | Documents the relative legacy command symlink shipped in release archives. |
| `docs/book/src/getting-started/installation.md` | Documents installation of the legacy command alias. |
| `docs/installation-reference.md` | Documents that same archive alias. |
| `docs/knowledge/README.md` | Explicitly states that historical identity keys are not migrated. |
| `.github/workflows/release-binaries.yml` | Deliberately packages and executes the relative legacy alias on every native release target. |
| `install.sh` | Installer command inventory must include the alias actually shipped. |
| `scripts/test-install-release.py` | Tests that command inventory, including the legacy alias. |
| `scripts/install-local.sh` | Atomically installs the legacy alias beside the current executable. |
| `scripts/install-release.sh` | Validates the archive alias and installs it atomically. |
| `scripts/probe-plate-session-scope.sh` | Explicit executable fallback for hosts with only the legacy command. |
| `src/config.rs` | Reads the legacy config table when the current table is absent. |
| `src/config_test.rs` | Tests legacy config and current-table precedence. |
| `src/hook/disk_guard.rs` | Recognizes the former installer's commands as well as Tambo; regression test preserves compatibility. |
| `src/hook/memory_guard.rs` | The same command-routing compatibility contract and regression test. |
| `src/hook/mod.rs` | Prunes markers left by pre-rename binaries, preserving the old marker prefix. |
| `src/hook/hook_test.rs` | Proves those old failure markers are removed. |
| `tests/install_local.py` | Tests existing alias upgrade, atomic replacement and failure preservation. |
| `tests/install_release.py` | Tests archive alias validation and installed alias resolution. |

The composite publication action's former-name historical comment is removed
by a separate workflow commit. Full native crate CI and public default-branch
scans remain required for acceptance.
