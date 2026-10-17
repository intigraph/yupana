# Import-scoped call candidates

Two Python modules define `leaf`; the caller imports only `a.leaf`. The same
`callers invoke FIXTURE --json` probe returns both modules on the installed
0.14.0 baseline and only `a.py` on the candidate. The `positive` caller remains
present in both runs. The recording uses temporary local source files, no daemon
and no graph writes. It proves candidate behavior, not installation.

Run `BASELINE_BIN=/path/to/baseline CANDIDATE_BIN=/path/to/candidate python3 probe.py`.
Baseline identity: version 0.14.0, SHA256
`f17bfabc2e0b665b0c05891910cb5f00b3f6ccc1ac2b7215ccf8fc683b776647`.
Candidate executable SHA256:
`ac6ab2069df01c80cf90eeaa579e3e2e96ed59e019e1fa0ac041ec342263724a`.
Demonstrated source commit: `48e41906bb5a821e7347c6992e180a73563565db`.
The [compressed output recording](output.log.gz) and [timing](timing.log) were captured with
util-linux `script` and replayed successfully with `scriptreplay`. Download both
and run `gzip -dc output.log.gz > output.log`, then
`scriptreplay --log-out output.log --log-timing timing.log`.
The [readable transcript](transcript.txt) preserves the recorded output as text.
This is a terminal recording with a readable transcript, not an inline video.
These small sanitized artifacts live with this demo for repository retention.

The candidate adds per-call `call_resolution` rows to CLI JSON and MCP/daemon
neighbor replies. A reply from an older daemon has `null` resolution coverage;
that is UNKNOWN, distinct from a known caller with no call sites. A single `import_candidate`, `local_candidate` or
`name_candidate` remains tree-sitter evidence, not semantic/type-checker proof.
Multiple candidates are `ambiguous`; no surviving definition is `unresolved`.
Dynamic receivers, rebound names, wildcard imports and unsupported bindings are
`unsupported`, with conservative name candidates retained. Candidate counts
measure returned definitions, not precision, recall or complete call coverage.

Supported lexical filtering is initially Python `from module import name`,
explicit aliases, single-level imported module receivers, relative modules and
package initializers. It does not execute imports, resolve environment-dependent
search paths or re-exports, infer receiver types, or replace LSP/CPG precision.
Other languages keep same-language conservative name candidates. Base and
masked tenant overlays use the same filter; exact enclosing definition line and
file prevent duplicate caller names from stealing call sites.

No upstream code was copied. Infigraph's import-aware resolution inspired the
comparison that led to this work; its implementation and heuristics were not
ported. See [the upstream source](https://github.com/intuit/infigraph/blob/cf82f5d7440d9be158e219df1ef256d6359ed1f6/crates/infigraph-core/src/resolve/calls.rs).
