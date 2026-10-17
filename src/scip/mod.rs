//! Explicit offline SCIP import. A caller supplies an index plus an attestation
//! captured at indexing time. Every indexed source must match both the committed
//! revision and the working copy. Refusal returns coverage and no semantic facts.
//! The attestation binds bytes; it does not authenticate the producer's claims.
use anyhow::{ensure, Context, Result};
use protobuf::Message;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path};
mod cli;
mod model;
mod ranges;
pub use cli::Args;
pub use model::*;

/// Import a protobuf index without running a compiler or language server.
/// Failure is all-or-nothing, with an explicit coverage record.
pub fn import(
    root: &Path,
    bytes: &[u8],
    binding: Binding,
) -> std::result::Result<Imported, Coverage> {
    let mut coverage = Coverage {
        declared_documents: binding.sources.len(),
        ..Coverage::default()
    };
    match validate(root, bytes, &binding, &mut coverage) {
        Ok(occurrences) => {
            coverage.accepted = true;
            Ok(Imported {
                tier: "scip",
                binding,
                protocol_version: 0,
                coverage,
                occurrences,
            })
        }
        Err(error) => {
            coverage.refusal = Some(format!("{error:#}"));
            Err(coverage)
        }
    }
}

/// SHA256 in the binding's lowercase hex representation.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn validate(
    root: &Path,
    bytes: &[u8],
    binding: &Binding,
    coverage: &mut Coverage,
) -> Result<Vec<Occurrence>> {
    ensure!(binding.version == 1, "unsupported binding version");
    ensure!(!binding.sources.is_empty(), "empty source coverage");
    ensure!(sha256(bytes) == binding.index_sha256, "index hash mismatch");
    let top =
        crate::git::run(root, &["rev-parse", "--show-toplevel"]).context("not a git repository")?;
    ensure!(
        root.canonicalize()? == Path::new(top.trim()).canonicalize()?,
        "root must be repository top level"
    );
    let origin = crate::git::run(root, &["remote", "get-url", "origin"])
        .context("repository origin unavailable")?;
    ensure!(
        origin.trim() == binding.repository && !binding.repository.is_empty(),
        "foreign repository"
    );
    let head = crate::git::head_commit(root).context("repository HEAD unavailable")?;
    ensure!(head == binding.revision, "stale repository revision");
    let index = ::scip::types::Index::parse_from_bytes(bytes).context("malformed SCIP protobuf")?;
    coverage.indexed_documents = index.documents.len();
    let metadata = index.metadata.as_ref().context("missing SCIP metadata")?;
    ensure!(
        metadata.version.value() == 0,
        "unsupported SCIP protocol version"
    );
    ensure!(
        metadata.text_document_encoding.value() == 1,
        "unsupported source text encoding"
    );
    ensure!(
        metadata.project_root == binding.project_root
            && binding.project_root.starts_with("file:///"),
        "project root mismatch"
    );
    let tool = metadata
        .tool_info
        .as_ref()
        .context("missing index producer")?;
    ensure!(
        !tool.name.is_empty() && !tool.version.is_empty(),
        "missing producer name/version"
    );
    ensure!(
        tool.name == binding.producer && tool.version == binding.producer_version,
        "producer mismatch"
    );
    let mut paths = BTreeSet::new();
    let mut facts = Vec::new();
    for doc in &index.documents {
        let path = checked_path(root, &doc.relative_path)?;
        ensure!(
            paths.insert(doc.relative_path.clone()),
            "duplicate document"
        );
        let expected = binding
            .sources
            .get(&doc.relative_path)
            .context("document outside declared coverage")?;
        let source = std::fs::read(&path)?;
        ensure!(
            sha256(&source) == *expected,
            "stale source bytes: {}",
            doc.relative_path
        );
        let committed = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "show",
                &format!("{}:{}", binding.revision, doc.relative_path),
            ])
            .output()?;
        ensure!(
            committed.status.success() && sha256(&committed.stdout) == *expected,
            "source does not match revision: {}",
            doc.relative_path
        );
        let text = std::str::from_utf8(&source).context("non UTF-8 source")?;
        ensure!(
            doc.text.is_empty() || doc.text == text,
            "embedded source mismatch"
        );
        let language = crate::extract::language_for_extension(
            path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default(),
        )
        .context("unsupported source language in this build")?;
        ensure!(
            doc.language.eq_ignore_ascii_case(language)
                || (language == "cpp" && doc.language == "c++"),
            "unsupported/mismatched index language"
        );
        // Legacy producers may omit document position_encoding. Only our
        // explicitly supported UTF-8 metadata fallback is accepted here.
        let encoding = match doc.position_encoding.value() {
            0 => 1,
            n => n,
        };
        let ast = crate::extract::extract_structure(text, language)?;
        for occurrence in &doc.occurrences {
            // Syntax-only occurrences intentionally have no symbol identity.
            if occurrence.symbol.is_empty() {
                continue;
            }
            ::scip::symbol::parse_symbol(&occurrence.symbol)
                .map_err(|_| anyhow::anyhow!("malformed SCIP symbol"))?;
            let identifier = ranges::range(&occurrence.range, text, encoding)?;
            let definition = occurrence.symbol_roles & 1 != 0;
            let symbol = if occurrence.symbol.starts_with("local ") {
                format!("{}::{}", doc.relative_path, occurrence.symbol)
            } else {
                occurrence.symbol.clone()
            };
            let mut bodies: Vec<_> = ast
                .symbols
                .iter()
                .filter(|s| {
                    definition
                        && s.start_line <= identifier.start_line + 1
                        && s.end_line > identifier.end_line
                })
                .collect();
            bodies.sort_by_key(|s| s.end_line - s.start_line);
            let ast_body = bodies
                .first()
                .filter(|s| {
                    bodies.get(1).is_none_or(|next| {
                        next.end_line - next.start_line > s.end_line - s.start_line
                    })
                })
                .map(|s| (*s).clone());
            facts.push(Occurrence {
                symbol,
                file: doc.relative_path.clone(),
                identifier,
                definition,
                ast_body,
            });
        }
        coverage.validated_documents += 1;
    }
    ensure!(
        paths.len() == binding.sources.len(),
        "incomplete document coverage"
    );
    ensure!(!facts.is_empty(), "index has no semantic occurrences");
    Ok(facts)
}

fn checked_path(root: &Path, value: &str) -> Result<std::path::PathBuf> {
    ensure!(
        !value.is_empty()
            && !value.contains('\\')
            && !value
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == ".."),
        "noncanonical document path"
    );
    let path = Path::new(value);
    ensure!(
        path.components().all(|c| matches!(c, Component::Normal(_))),
        "nonrelative document path"
    );
    let mut current = root.to_path_buf();
    for part in path.components() {
        current.push(part);
        ensure!(
            !std::fs::symlink_metadata(&current)?
                .file_type()
                .is_symlink(),
            "symlink document path"
        );
    }
    ensure!(current.is_file(), "document is not a regular file");
    Ok(current)
}
