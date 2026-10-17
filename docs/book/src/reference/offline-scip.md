# Offline SCIP navigation

`yupana import-scip` reads an existing local protobuf SCIP index. It never
runs an indexer, downloads an index, starts LSP, or changes the resident graph.
It prints JSON with a `scip` tier, explicit coverage, producer and protocol
versions, and source-bound definition/reference occurrences.

```bash
yupana import-scip --root ./checkout --index ./index.scip --binding ./binding.json
# Restrict the output to one exact SCIP identity:
yupana import-scip --root ./checkout --index ./index.scip --binding ./binding.json \
  --symbol 'rust-analyzer cargo example 0.1.0 module/duplicate().'
```

The binding must be captured alongside the index at indexing time. A binding
assembled from today's source bytes cannot establish that an older index was
produced from them. The binding verifies byte consistency; it does not authenticate
the index producer or establish compiler accuracy. Use trusted index artifacts.

```json
{
  "version": 1,
  "repository": "https://example.com/project.git",
  "revision": "FULL_COMMIT_ID",
  "index_sha256": "LOWERCASE_SHA256_OF_PROTOBUF",
  "project_root": "file:///original/indexing/root",
  "producer": "rust-analyzer",
  "producer_version": "VERSION_FROM_INDEX_METADATA",
  "sources": {
    "src/lib.rs": "LOWERCASE_SHA256_OF_SOURCE_BYTES_AT_INDEXING_TIME"
  }
}
```

`repository` must exactly match the checkout's `origin` URL, and `revision`
must exactly match its full `HEAD` commit. Each source hash must match both the
committed blob and the checkout bytes. The source map must equal the complete
indexed document set. This coverage describes that set, not the whole repository:
files omitted by the producer remain outside the index's coverage. The original
project-root URI is bound to index metadata, allowing a checkout to be relocated.
Only UTF-8 source files and languages supported by the current build are accepted.
The language declaration must match the file extension. Symbol relationships,
external symbol documentation and diagnostic payloads are not imported.

Every import is all-or-nothing. Stale revision/bytes, foreign repository, changed
index hash, malformed protobuf/symbol/range, unsafe path, duplicate document,
unsupported encoding/language, or incomplete coverage returns nonzero with
`coverage.accepted: false`, a refusal reason, and no occurrences. A successful
identity lookup with no occurrences means only that the identity is absent from
this validated index. Local SCIP identities are qualified by document path.

Identifier ranges use zero-based lines and UTF-8 byte columns, with exclusive
ends. SCIP UTF-16/UTF-32 positions are checked and normalized; positions inside a
code unit or outside a line are refused. For legacy indexes omitting document
position encoding, this importer uses the required UTF-8 metadata encoding. `ast_body` is a separate tree-sitter
symbol with one-based inclusive line bounds. It is omitted when the enclosing
body cannot be matched unambiguously. No SCIP identifier range replaces an AST
body, and SCIP references are not interpreted as call edges. The returned object
is a validated import-time snapshot, not a continuously refreshed source view.

The checked-in synthetic Rust fixture was generated with the compiler-backed
`rust-analyzer scip` command; see its [provenance](../../../../tests/fixtures/scip/README.md).
The importer uses the official [SCIP protobuf bindings](https://github.com/scip-code/scip),
independently of the initial design inspiration from
[infigraph](https://github.com/intuit/infigraph). No infigraph code was copied.
