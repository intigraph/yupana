//! Public binding and navigation records. Coordinates are zero-based UTF-8 bytes.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Attestation accompanying a compiler-produced index; never inferred from current bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Binding contract version; currently 1.
    pub version: u32,
    /// Exact origin URL of the repository, without credentials.
    pub repository: String,
    /// Full commit identity at indexing time.
    pub revision: String,
    /// SHA256 of the original protobuf index.
    pub index_sha256: String,
    /// Original index project-root URI (allows relocating a verified checkout).
    pub project_root: String,
    /// Index producer name.
    pub producer: String,
    /// Index producer version.
    pub producer_version: String,
    /// Complete indexed-document set: canonical root-relative path -> source-byte SHA256.
    pub sources: BTreeMap<String, String>,
}

/// A normalized identifier range, separate from an AST body span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Range {
    /// Zero-based start line.
    pub start_line: usize,
    /// Zero-based UTF-8 byte offset on the start line.
    pub start_byte: usize,
    /// Zero-based end line.
    pub end_line: usize,
    /// Exclusive UTF-8 byte offset on the end line.
    pub end_byte: usize,
}

/// A source-bound semantic occurrence.
#[derive(Debug, Clone, Serialize)]
pub struct Occurrence {
    /// Opaque SCIP identity; local identities are qualified by document.
    pub symbol: String,
    /// Root-relative file.
    pub file: String,
    /// Identifier span from SCIP, normalized to byte coordinates.
    pub identifier: Range,
    /// True when SCIP's Definition role bit is set.
    pub definition: bool,
    /// Separately extracted AST body; absent when matching is ambiguous/unavailable.
    pub ast_body: Option<crate::types::Symbol>,
}

/// Explicit coverage of an all-or-nothing import, never an implicit AST fallback.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Coverage {
    /// Whether every indexed document passed all binding checks.
    pub accepted: bool,
    /// Documents declared by the supplied binding.
    pub declared_documents: usize,
    /// Documents in the parsed index (zero if parsing failed).
    pub indexed_documents: usize,
    /// Documents validated before success/refusal.
    pub validated_documents: usize,
    /// Precise refusal reason; no facts are returned on failure.
    pub refusal: Option<String>,
}

/// Validated in-memory index; no daemon, download or persistence is implied.
#[derive(Debug, Serialize)]
pub struct Imported {
    /// Always `scip`; does not imply LSP or compiler execution by Yupana.
    pub tier: &'static str,
    /// Exact attested repository/revision, producer and byte identities.
    pub binding: Binding,
    /// Index protocol version (SCIP currently defines only version 0).
    pub protocol_version: i32,
    /// Explicit document coverage.
    pub coverage: Coverage,
    /// Validated semantic occurrences, retaining SCIP identity.
    pub occurrences: Vec<Occurrence>,
}

impl Imported {
    /// Exact identity lookup. Definitions/references remain separate by their role.
    #[must_use]
    pub fn lookup(&self, symbol: &str) -> Vec<&Occurrence> {
        self.occurrences
            .iter()
            .filter(|o| o.symbol == symbol)
            .collect()
    }
}
