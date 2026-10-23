//! Exact-target admission evidence for a governed landing predicate.
//!
//! This vocabulary knows no risk tiers, repositories or review exemptions.
//! The admitted checks and issuer key come from policy data. The canonical
//! admission producer remains responsible for evaluating current requirements.
use crate::action_certification::{verify, SignedActionRecord};
use serde::{Deserialize, Serialize};

/// The immutable inputs selected by the current governed policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionPolicy {
    /// Digest of the complete admitted policy/producer contract.
    pub revision: String,
    /// Ed25519 public key pinned by the policy owner, not by the proof.
    pub issuer_public_key: String,
    /// Maximum evidence age, checked without clock-skew grace.
    pub max_age_secs: u64,
    /// Each named check must explicitly expect and observe boolean true.
    pub required_checks: Vec<String>,
}

/// Independently resolved immutable operation being attempted.
#[derive(Debug, Clone)]
pub struct AdmissionTarget<'a> {
    /// Remote authority, including any nondefault host identity.
    pub authority: &'a str,
    /// Full owner/repository slug; a basename is insufficient.
    pub repository: &'a str,
    /// Exact pull request number.
    pub pull_request: u64,
    /// Head bound by the actual merge operation's compare-and-swap.
    pub head: &'a str,
    /// Current base admitted by the helper's preflight.
    pub base: &'a str,
    /// Protected branch receiving the merge.
    pub base_ref: &'a str,
    /// SHA256 of the exact command evidence selected at this hook boundary.
    pub command_sha256: &'a str,
}

/// A predicate result, before advise/enforce response policy is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionVerdict {
    /// The pinned issuer admitted these exact immutable inputs.
    Satisfied,
    /// Evidence was not supplied. This never means an exemption.
    Missing,
    /// Policy or selected target cannot be evaluated honestly.
    Unknown,
    /// Supplied evidence does not satisfy the selected predicate.
    Unsatisfied,
    /// Authentic exact-target evidence names a different producer contract.
    StaleBinding,
}

fn hex_digest(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl AdmissionPolicy {
    /// Reject incomplete contracts and ambiguous duplicate check identifiers.
    #[must_use]
    pub fn valid(&self) -> bool {
        hex_digest(&self.revision, 64)
            && hex_digest(&self.issuer_public_key, 64)
            && (1..=3600).contains(&self.max_age_secs)
            && !self.required_checks.is_empty()
            && self.required_checks.iter().all(|id| !id.trim().is_empty())
            && self
                .required_checks
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.required_checks.len()
    }
}

/// Evaluate a signed admission without performing network, filesystem or clock I/O.
///
/// A generic signed `certified` trace is insufficient: it must explicitly say
/// `admitted`, carry every required affirmative check, and bind all current
/// target fields. Missing target identity is UNKNOWN, never inferred from a
/// proof controlled by the caller. Transport identity is signed provenance,
/// not inferred from a synthetic `gh pr merge` command.
#[must_use]
pub fn evaluate(
    policy: &AdmissionPolicy,
    target: &AdmissionTarget<'_>,
    proof: Option<&SignedActionRecord>,
    now: u64,
) -> AdmissionVerdict {
    if !policy.valid()
        || target.authority.is_empty()
        || target.repository.split('/').count() != 2
        || target.repository.split('/').any(str::is_empty)
        || target.pull_request == 0
        || !hex_digest(target.head, 40)
        || !hex_digest(target.base, 40)
        || target.base_ref.is_empty()
        || !hex_digest(target.command_sha256, 64)
    {
        return AdmissionVerdict::Unknown;
    }
    let Some(proof) = proof else {
        return AdmissionVerdict::Missing;
    };
    let Ok(key) = hex::decode(&policy.issuer_public_key) else {
        return AdmissionVerdict::Unknown;
    };
    if !verify(proof, &key)
        || proof.verb != "merge"
        || proof.result != "admitted"
        || proof.certification_status != "certified"
        || proof.remote_authority != target.authority
        || proof.repo != target.repository
        || proof.sha != target.head
        || proof.git_ref != target.base_ref
        || proof
            .scope_provenance
            .get("command_sha256")
            .and_then(serde_json::Value::as_str)
            != Some(target.command_sha256)
        || proof.ts > now
        || now - proof.ts > policy.max_age_secs
        || proof
            .scope_provenance
            .get("pr")
            .and_then(serde_json::Value::as_u64)
            != Some(target.pull_request)
        || proof
            .scope_provenance
            .get("base")
            .and_then(serde_json::Value::as_str)
            != Some(target.base)
        || proof
            .scope_provenance
            .get("transport")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
        || proof
            .checks
            .iter()
            .map(|check| &check.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != proof.checks.len()
        || policy.required_checks.iter().any(|id| {
            !proof.checks.iter().any(|check| {
                &check.id == id
                    && check.expected == true
                    && check.observed == true
                    && !check.evidence_ref.trim().is_empty()
            })
        })
    {
        return AdmissionVerdict::Unsatisfied;
    }
    let revision = proof
        .scope_provenance
        .get("policy_revision")
        .and_then(serde_json::Value::as_str);
    if revision.is_none_or(|value| !hex_digest(value, 64)) {
        return AdmissionVerdict::Unsatisfied;
    }
    if revision != Some(&policy.revision) {
        return AdmissionVerdict::StaleBinding;
    }
    AdmissionVerdict::Satisfied
}

#[cfg(test)]
#[path = "landing_admission_test.rs"]
mod tests;
