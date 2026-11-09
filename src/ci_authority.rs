//! Non-executing CI advice from a separately reviewed repository manifest.
//!
//! The caller supplies repository identity and the manifest digest from its
//! trusted review record, never from graph data or the mutable manifest itself.
//! This module grants no execution authority and starts no processes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::errors::{Error, Result};

/// A graph response can select IDs, but cannot contain executable commands.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdProjection {
    /// An affirmative completeness witness is required.
    pub truncated: bool,
    /// Repository-scoped check references.
    pub checks: Vec<CheckReference>,
}

/// Reference to a check owned by a repository, without executable content.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckReference {
    /// Canonical forge-host/owner/repository identity.
    pub repository: String,
    /// Stable ID resolved only through the reviewed manifest.
    pub check_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    repository: String,
    source_sha: String,
    checks: Vec<ManifestCheck>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestCheck {
    id: String,
    argv: Vec<String>,
    scopes: Vec<String>,
    static_only: bool,
    network: bool,
    timeout_seconds: u32,
    output_bytes: u32,
}

/// A recommendation bound to independently supplied source and manifest IDs.
/// It is deliberately data, with no method to execute it.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Advice {
    /// Canonical repository identity.
    pub repository: String,
    /// Reviewed manifest digest, including its argv definitions.
    pub manifest_sha256: String,
    /// Reviewed source revision.
    pub source_sha: String,
    /// Repository-owned check ID.
    pub check_id: String,
    /// Manifest-owned argv, never interpreted as shell text.
    pub argv: Vec<String>,
    /// Proposed child wall-time limit for a later admitted executor.
    pub timeout_seconds: u32,
    /// Proposed output limit for a later admitted executor.
    pub output_bytes: u32,
}

/// A complete nonmatching map is quiet; absence or invalid data is not.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// No complete mapping exists.
    Unknown,
    /// A complete valid map has no check for these paths.
    Quiet,
    /// Bounded static-only recommendations. Execution remains disabled.
    Advise(Vec<Advice>),
}

fn refused() -> Error {
    Error::Projection("CI advice lacks repository-bound reviewed command authority".into())
}

fn hex_id(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn repository_id(value: &str) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
}

fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Build advice without reading files, invoking children, or changing policy.
///
/// `expected_digest` and `expected_source` must come from an independently
/// authenticated review record for `repository`, and not from the graph or
/// manifest under evaluation. This function checks consistency, not review
/// authentication. No caller may treat a returned plan as permission to run it.
pub fn plan(
    repository: &str,
    expected_digest: &str,
    expected_source: &str,
    manifest_bytes: &[u8],
    projection_bytes: &[u8],
    changed: &[PathBuf],
) -> Result<Plan> {
    if !repository_id(repository)
        || !hex_id(expected_digest, 64)
        || !hex_id(expected_source, 40)
        || manifest_bytes.len() > 65_536
        || projection_bytes.len() > 65_536
        || changed.len() > 1_000
        || changed.iter().any(|path| !relative(path))
        || hex::encode(Sha256::digest(manifest_bytes)) != expected_digest
    {
        return Err(refused());
    }
    let manifest: Manifest = serde_json::from_slice(manifest_bytes).map_err(|_| refused())?;
    let projection: IdProjection =
        serde_json::from_slice(projection_bytes).map_err(|_| refused())?;
    if manifest.schema != 1
        || manifest.repository != repository
        || manifest.source_sha != expected_source
        || manifest.checks.len() > 64
        || projection.truncated
        || projection.checks.len() > 64
    {
        return Err(refused());
    }
    let mut commands = BTreeMap::new();
    for check in &manifest.checks {
        if check.id.is_empty()
            || check.id.len() > 128
            || !check
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || check.argv.is_empty()
            || check.argv.len() > 32
            || check
                .argv
                .iter()
                .any(|arg| arg.is_empty() || arg.len() > 4_096 || arg.contains('\0'))
            || !relative(Path::new(&check.argv[0]))
            || !check.static_only
            || check.network
            || check.timeout_seconds == 0
            || check.timeout_seconds > 30
            || check.output_bytes == 0
            || check.output_bytes > 65_536
            || check.scopes.is_empty()
            || check.scopes.len() > 32
            || check
                .scopes
                .iter()
                .any(|scope| !relative(Path::new(scope)) || glob::Pattern::new(scope).is_err())
            || commands.insert(check.id.as_str(), check).is_some()
        {
            return Err(refused());
        }
    }
    let mut selected = BTreeSet::new();
    let mut advice = Vec::new();
    let mut seconds = 0;
    // Validate every reference before returning even a quiet result.
    for reference in &projection.checks {
        if reference.repository != repository || !selected.insert(reference.check_id.as_str()) {
            return Err(refused());
        }
        let check = commands
            .get(reference.check_id.as_str())
            .ok_or_else(refused)?;
        if check.scopes.iter().any(|scope| {
            glob::Pattern::new(scope)
                .is_ok_and(|pattern| changed.iter().any(|path| pattern.matches_path(path)))
        }) {
            seconds += check.timeout_seconds;
            if seconds > 60 {
                return Err(refused());
            }
            advice.push(Advice {
                repository: repository.into(),
                manifest_sha256: expected_digest.into(),
                source_sha: expected_source.into(),
                check_id: check.id.clone(),
                argv: check.argv.clone(),
                timeout_seconds: check.timeout_seconds,
                output_bytes: check.output_bytes,
            });
        }
    }
    Ok(if projection.checks.is_empty() {
        Plan::Unknown
    } else if advice.is_empty() {
        Plan::Quiet
    } else {
        Plan::Advise(advice)
    })
}

#[cfg(test)]
#[path = "ci_authority_test.rs"]
mod tests;
