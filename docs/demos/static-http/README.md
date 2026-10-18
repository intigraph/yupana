# Source-bound static HTTP candidates

[Recorded terminal demonstration](output.log) with [native timing](timing.log)
shows two literal candidates, one unresolved route and unchanged committed
evidence after a working-tree replacement. Replay with `scriptreplay -t timing.log output.log`.

The bounded Python profile reads syntax nodes for FastAPI/APIRouter routes and
explicit requests/httpx module calls. These are tree-sitter candidates, not
runtime dependencies. No network, indexer, graph write or schema load occurs.

```sh
cargo build --features langs-extra --example http_candidates
python3 docs/demos/static-http/run.py target/debug/examples/http_candidates
```

`export::http_candidates_at(root, repository, revision)` returns a typed snapshot
from committed Git blobs. The caller supplies the established repository identity.
`http_candidates::verify_current` refuses a foreign repository identity, moved
HEAD, changed working bytes, unsafe source path or unverified file. A historical
snapshot remains historical; callers must verify before downstream admission.
The buffer API `extract` always labels its input `working_tree`.

Records preserve explicit method, full literal prefixes/path, source SHA256,
revision, extractor/profile version, separate occurrence/caller/handler byte
ranges and ordered mount provenance. Candidate IDs include the full source and
occurrence/mount/method/path/authority identity. Multi-method registrations and
multiple mounts remain distinct. Unknown methods never default to GET.

Supported imports are module-level `from fastapi import FastAPI, APIRouter` and
requests/httpx module imports, including aliases. Router bindings, mounts and
handler resolution are same-file only. Rebinding invalidates aliases
conservatively for the whole file, including variadic parameters and wildcard
imports. Explicit registration resolves only stable, undecorated module-level
handler names defined before the call. Nested, shadowed or rebound handlers retain
`unknown_handler`; decorator routes retain their directly attached handler span.
Plain unescaped string literals are supported;
interpolation, cross-file mounts, unknown bases and ambiguous handlers stay
unresolved. Malformed Python returns explicit diagnostics and no candidates.
Generated-client status remains unknown except an explicit `# @generated` header,
which makes records unresolved. Absence of that header never means nongenerated.
Client instances, SDKs, generic `request()` and other frameworks are outside this
profile's declared coverage. A build without `langs-extra` reports unavailable.

Paths retain case, escaped bytes, slashes and template text. Templates are not
wildcards. Consumer queries record presence only; credentials/raw query values
are excluded. The downstream governed profile owns service mapping, schema and
match policy. This source API does not load that profile or create RDF vocabulary.

The design was informed by the static contract extraction in
[Infigraph at the studied revision](https://github.com/intuit/infigraph/tree/cf82f5d7440d9be158e219df1ef256d6359ed1f6).
No upstream code was copied. Its wildcard method fallback is deliberately absent.
