# Static HTTP candidates

The opt-in library API `yupana::export::http_candidates_at` returns a typed JSON
snapshot of committed Python FastAPI routes and explicit requests/httpx calls.
It requires `langs-extra`; an unsupported build reports unavailable. It does
not change the default Turtle export, install schemas, or ingest a graph.

Run the synthetic [offline demonstration](../../../demos/static-http/README.md)
for the exact supported syntax, expected records and example exporter command.
The snapshot retains source hashes, revision, extractor/profile version,
occurrence and caller/handler spans, method, full literal path, and unresolved
reasons. Treat every record as tree-sitter candidate evidence.

Before admitting a snapshot as current, call
`yupana::http_candidates::verify_current(root, repository, &snapshot)`. It rejects
a moved HEAD, foreign repository identity, changed source bytes, mismatched
committed blobs and unsafe paths. Historical snapshots do not become current
by substituting a new revision.

Dynamic paths, unknown mounts/bases, ambiguous bindings and explicit generated
headers remain unresolved. Unknown generated-client status never means false.
The bounded profile does not cover client instances, SDKs, cross-file binding or
other frameworks; the coverage manifest discloses those limits. A consumer and
producer sharing a literal path do not establish a runtime dependency.
