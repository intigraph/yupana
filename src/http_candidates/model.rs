//! Source-bound static evidence; none of these records assert runtime edges.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// Span in the source-bound candidate evidence envelope.
pub struct Span {
    /// Start byte.
    pub start_byte: usize,
    /// End byte.
    pub end_byte: usize,
    /// Start line.
    pub start_line: usize,
    /// End line.
    pub end_line: usize,
    /// Start column byte.
    pub start_column_byte: usize,
    /// End column byte.
    pub end_column_byte: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// Owner in the source-bound candidate evidence envelope.
pub struct Owner {
    /// Name.
    pub name: String,
    /// Scope.
    pub scope: Vec<String>,
    /// Span.
    pub span: Span,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// Authority in the source-bound candidate evidence envelope.
pub struct Authority {
    /// Scheme.
    pub scheme: String,
    /// Host.
    pub host: String,
    /// Port.
    pub port: Option<u16>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// Candidate in the source-bound candidate evidence envelope.
pub struct Candidate {
    /// Id.
    pub id: String,
    /// Role.
    pub role: String,
    /// Tier.
    pub tier: String,
    /// Confidence.
    pub confidence: String,
    /// Method.
    pub method: Option<String>,
    /// Declared path.
    pub declared_path: Option<String>,
    /// Path.
    pub path: Option<String>,
    /// Authority.
    pub authority: Option<Authority>,
    /// Query present.
    pub query_present: Option<bool>,
    /// Occurrence span.
    pub occurrence_span: Span,
    /// Prefix spans.
    pub prefix_spans: Vec<Span>,
    /// Caller.
    pub caller: Option<Owner>,
    /// Handler.
    pub handler: Option<Owner>,
    /// Resolution.
    pub resolution: String,
    /// Unresolved reasons.
    pub unresolved_reasons: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// `FileCandidates` in the source-bound candidate evidence envelope.
pub struct FileCandidates {
    /// Module.
    pub module: String,
    /// Source sha256.
    pub source_sha256: String,
    /// Language.
    pub language: String,
    /// Snapshot state.
    pub snapshot_state: String,
    /// Candidates.
    pub candidates: Vec<Candidate>,
    /// Diagnostics.
    pub diagnostics: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// Extractor in the source-bound candidate evidence envelope.
pub struct Extractor {
    /// Name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Profile version.
    pub profile_version: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
/// Snapshot in the source-bound candidate evidence envelope.
pub struct Snapshot {
    /// Contract.
    pub contract: String,
    /// Profile.
    pub profile: String,
    /// Extractor.
    pub extractor: Extractor,
    /// Repository.
    pub repository: String,
    /// Revision.
    pub revision: String,
    /// Observed at.
    pub observed_at: String,
    /// Files.
    pub files: Vec<FileCandidates>,
    /// Coverage.
    pub coverage: Coverage,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
/// Coverage in the source-bound candidate evidence envelope.
pub struct Coverage {
    /// Limitations.
    pub limitations: Vec<String>,
    /// Profiled files.
    pub profiled_files: usize,
    /// Unsupported files.
    pub unsupported_files: usize,
    /// Parse errors.
    pub parse_errors: usize,
    /// Literal records.
    pub literal_records: usize,
    /// Unresolved records.
    pub unresolved_records: usize,
}
