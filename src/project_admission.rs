//! Governed admission policies carried on the existing projection circuit.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::errors::{Error, Result};
use crate::landing_admission::AdmissionPolicy;

use crate::project_queries::ONTOLOGY_NAMESPACE as NS;

/// A repository identity and the refs selected by policy data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionScope {
    /// Canonical remote authority, never a repository basename.
    pub authority: String,
    /// Full owner/repository slug.
    pub repository: String,
    /// Exact destination branch names.
    pub protected_refs: Vec<String>,
}

/// Selector evidence vocabulary, independent of any deployment's repository set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionSelector {
    /// Explicit bounded repository/ref scope.
    pub repositories: Vec<AdmissionScope>,
}

/// Policy, selector and predicate atoms decoded from governed graph data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectedAdmission {
    /// Policy IRI.
    pub id: String,
    /// Human explanation carried into an advisory or denial.
    pub rationale: String,
    /// Warn or block, independently bounded by the local policy mode.
    pub tier: String,
    /// Warn for advice, deny for block.
    pub effect: String,
    /// Pre-action gate placement.
    pub verification_point: String,
    /// Must-match evidence requirement.
    pub match_type: String,
    /// Repository/ref selection.
    pub selector: AdmissionSelector,
    /// Issuer and current immutable admission contract.
    pub predicate: AdmissionPolicy,
}

impl ProjectedAdmission {
    /// Reject malformed network results and restored cache members equally.
    pub fn validate(&self) -> Result<()> {
        let fail = || Error::Projection(format!("invalid landing-admission policy {}", self.id));
        if self.id.is_empty()
            || self.rationale.trim().is_empty()
            || !matches!(
                (self.tier.as_str(), self.effect.as_str()),
                ("warn", "warn") | ("block", "deny")
            )
            || self.verification_point != "PAG"
            || self.match_type != "must-match"
            || !self.predicate.valid()
            || self.selector.repositories.is_empty()
            || self.selector.repositories.len() > 128
        {
            return Err(fail());
        }
        let mut identities = std::collections::BTreeSet::new();
        for scope in &self.selector.repositories {
            let atom = |v: &str| {
                !v.is_empty()
                    && v.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-._:".contains(&c))
            };
            if !atom(&scope.authority)
                || scope.repository.split('/').count() != 2
                || !scope.repository.split('/').all(atom)
                || scope.protected_refs.is_empty()
                || scope
                    .protected_refs
                    .iter()
                    .any(|r| r.is_empty() || r.chars().any(char::is_whitespace))
                || !identities.insert((&scope.authority, &scope.repository))
            {
                return Err(fail());
            }
        }
        Ok(())
    }

    /// Select a canonical repository and an explicitly resolved destination ref.
    #[must_use]
    pub fn selects(&self, authority: &str, repository: &str, git_ref: &str) -> bool {
        self.selector.repositories.iter().any(|scope| {
            scope.authority == authority
                && scope.repository == repository
                && scope.protected_refs.iter().any(|r| r == git_ref)
        })
    }
}

type Properties = BTreeMap<String, Vec<String>>;

fn error(message: impl std::fmt::Display) -> Error {
    Error::Projection(format!("landing-admission catalogue: {message}"))
}

fn properties(endpoint: &str, iri: &str) -> Result<Properties> {
    if !iri.contains(':')
        || iri
            .chars()
            .any(|c| c.is_whitespace() || "<>\\\"{}".contains(c))
    {
        return Err(error("unsafe subject IRI"));
    }
    let body = crate::project::query(
        endpoint,
        &format!("SELECT ?property ?value WHERE {{ <{iri}> ?property ?value }}"),
    )?;
    let value = serde_json::from_str(&body).map_err(error)?;
    let mut out: Properties = BTreeMap::new();
    for row in crate::project_decode::rows_of(&value)? {
        let key = crate::project_decode::binding_value(row, "property")
            .ok_or_else(|| error("missing property"))?;
        let value = crate::project_decode::binding_value(row, "value")
            .ok_or_else(|| error("missing value"))?;
        let values = out.entry(key).or_default();
        if !values.contains(&value) {
            values.push(value);
        }
    }
    Ok(out)
}

fn one(properties: &Properties, key: &str) -> Result<String> {
    match properties.get(&format!("{NS}{key}")).map(Vec::as_slice) {
        Some([value]) => Ok(value.clone()),
        _ => Err(error(format!("missing or conflicting {key}"))),
    }
}

fn typed(properties: &Properties, class: &str) -> Result<()> {
    if !properties
        .get("http://www.w3.org/1999/02/22-rdf-syntax-ns#type")
        .is_some_and(|types| types.contains(&format!("{NS}{class}")))
    {
        return Err(error(format!("member must be typed {class}")));
    }
    Ok(())
}

/// Indexed discovery followed by exact subject reads; no optional joins that
/// could silently drop an incomplete policy. An empty answer is a known empty
/// catalogue; failed reads remain errors and preserve the prior projection.
pub fn fetch_admission_policies(endpoint: &str) -> Result<Vec<ProjectedAdmission>> {
    let query = format!("PREFIX aegis: <{NS}> SELECT ?policy WHERE {{ ?policy aegis:targets \"landing-admission\" }}");
    let body = crate::project::query(endpoint, &query)?;
    let value = serde_json::from_str(&body).map_err(error)?;
    let mut out = Vec::new();
    for row in crate::project_decode::rows_of(&value)? {
        let id = crate::project_decode::binding_value(row, "policy")
            .ok_or_else(|| error("missing policy identity"))?;
        if out.iter().any(|p: &ProjectedAdmission| p.id == id) {
            continue;
        }
        if out.len() >= 64 {
            return Err(error("catalogue exceeds bounded policy count"));
        }
        let policy = properties(endpoint, &id)?;
        typed(&policy, "Policy")?;
        let selector = properties(endpoint, &one(&policy, "selector")?)?;
        let predicate = properties(endpoint, &one(&policy, "predicate")?)?;
        typed(&selector, "Selector")?;
        typed(&predicate, "Predicate")?;
        let rationale = match policy
            .get("http://www.w3.org/2000/01/rdf-schema#comment")
            .map(Vec::as_slice)
        {
            Some([value]) => value.clone(),
            _ => return Err(error("missing or conflicting rationale")),
        };
        let rule = ProjectedAdmission {
            id,
            rationale,
            tier: one(&policy, "enforcementTier")?,
            effect: one(&policy, "effect")?,
            verification_point: one(&policy, "verificationPoint")?,
            match_type: one(&predicate, "matchType")?,
            selector: serde_json::from_str(&one(&selector, "evidenceSource")?).map_err(error)?,
            predicate: serde_json::from_str(&one(&predicate, "evidenceSource")?).map_err(error)?,
        };
        rule.validate()?;
        out.push(rule);
    }
    Ok(out)
}

impl crate::project::ProjectionRegistry {
    /// None is an old/unavailable channel, not an observed empty catalogue.
    #[must_use]
    pub fn admission_policies(&self) -> Option<&[ProjectedAdmission]> {
        self.admission_policies.as_deref()
    }
}

#[cfg(test)]
#[path = "project_admission_test.rs"]
mod tests;
