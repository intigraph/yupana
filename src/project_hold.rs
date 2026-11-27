//! Governed quiet-time build holds projected from Quipu.
//!
//! A media quiet-time hold must not stop launches or light work; it only
//! refuses heavy local builds (operator directive, 2026-11-27: "You can still
//! do work, just refrain from heavy builds"). The launcher no longer refuses
//! anything for it, so the refusal lives HERE, at the pre-action seam, and every
//! tunable is DATA: a Policy aimed at `aegis:HeavyBuildCommand` composes a live
//! Bash-command selector, an exact regex predicate, and a `quiet_time_hold`
//! `OperatingPoint` that names the hold signal — the textfile the fleet writes,
//! the exact gauge series in it, and how old that file may be before the signal
//! counts as UNKNOWN. Yupana owns only the evaluator.
//!
//! A separate target from `aegis:MemoryHeavyCommand` on purpose: an older
//! binary's memory query never selects a hold policy, so minting one cannot
//! break that binary's whole projection with a decode error.

use serde::{Deserialize, Serialize};

use crate::constraint::{ConstraintClass, VerificationPoint};
use crate::errors::{Error, Result};
use crate::policy::Mode;

/// The only operating-point kind this plane evaluates.
pub const HOLD_KIND: &str = "quiet_time_hold";

/// The graph catalogue query for quiet-time build holds.
pub const HOLD_POLICY_QUERY: &str = "\
PREFIX aegis: <http://aegis.gastown.local/ontology/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?policy ?label ?pattern ?matchType ?effect ?constraintClass
       ?verificationPoint ?kind ?series ?signal ?maxAge ?retry ?rationale WHERE {
  ?policy a aegis:Policy ;
          aegis:targets \"aegis:HeavyBuildCommand\" ;
          aegis:boundary \"action\" ;
          aegis:selector ?selector ;
          aegis:predicate ?predicate ;
          aegis:operatingPoint ?operatingPoint ;
          aegis:effect ?effect ;
          aegis:constraintClass ?constraintClass ;
          aegis:verificationPoint ?verificationPoint .
  ?selector aegis:evidenceSource \"bash-command\" ;
            aegis:tier \"live\" .
  ?predicate aegis:evidenceSource ?pattern ;
             aegis:matchType ?matchType ;
             aegis:tier \"live\" .
  ?operatingPoint aegis:kind ?kind ;
                  aegis:calibrationBasis ?series ;
                  aegis:evidenceSource ?signal ;
                  aegis:maxAgeSeconds ?maxAge .
  OPTIONAL { ?operatingPoint aegis:retryAdvice ?retry }
  OPTIONAL { ?policy rdfs:label ?label }
  OPTIONAL { ?policy rdfs:comment ?rationale }
}";

/// One quiet-time build-hold rule, fully decoded and validated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoldPolicy {
    /// Stable policy IRI.
    pub id: String,
    /// Human-readable policy name.
    pub label: String,
    /// Exact regex selecting heavy-build command lines.
    pub command_regex: String,
    /// The exact gauge series (name plus labels) that carries the hold, e.g.
    /// `aegis_quiet_time_hold_active{effect="no-heavy-builds"}`.
    pub series: String,
    /// The Prometheus textfile the series is read from.
    pub signal_path: String,
    /// Older than this, the signal is UNKNOWN (and the build refused).
    pub max_age_secs: u64,
    /// Governed advice on when to retry; a default is rendered when absent.
    pub retry: Option<String>,
    /// Governed response when the hold is active.
    pub effect: String,
    /// Constraint class, independent of response.
    pub class: ConstraintClass,
    /// The point where the graph says this policy runs.
    pub verification_point: VerificationPoint,
    /// Optional governed explanation.
    pub rationale: Option<String>,
}

impl crate::project::ProjectionRegistry {
    /// Governed quiet-time build holds.
    #[must_use]
    pub fn hold_policies(&self) -> &[HoldPolicy] {
        &self.hold_policies
    }
}

impl HoldPolicy {
    /// Whether this policy selects `command`.
    #[must_use]
    pub fn matches(&self, command: &str) -> bool {
        regex::Regex::new(&self.command_regex).is_ok_and(|r| r.is_match(command))
    }

    /// Whether the policy may block under the deployment's ambient mode.
    #[must_use]
    pub fn blocks(&self, mode: Mode) -> bool {
        self.class.blocks(mode) && !matches!(self.effect.as_str(), "warn" | "record" | "allow")
    }
}

/// Fetch the governed quiet-time build-hold catalogue.
pub fn fetch_hold_policies(endpoint: &str) -> Result<Vec<HoldPolicy>> {
    decode_hold_policies(&crate::project::query(endpoint, HOLD_POLICY_QUERY)?)
}

/// Both command-policy catalogues the pre-bash arms read, one sync for both.
pub fn fetch_command_policies(
    endpoint: &str,
) -> Result<(Vec<crate::project_memory::MemoryPolicy>, Vec<HoldPolicy>)> {
    Ok((
        crate::project_memory::fetch_memory_policies(endpoint)?,
        fetch_hold_policies(endpoint)?,
    ))
}

/// Decode Quipu's W3C SPARQL-results response.
pub fn decode_hold_policies(body: &str) -> Result<Vec<HoldPolicy>> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| Error::Projection(format!("hold-policy results are not JSON: {e}")))?;
    let rows = crate::project_decode::rows_of(&value)?;
    let mut out: Vec<HoldPolicy> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let policy = decode_row(i, row)?;
        if let Some(existing) = out.iter().find(|p| p.id == policy.id) {
            if existing != &policy {
                return Err(Error::Projection(format!(
                    "hold policy `{}` has conflicting values across SPARQL rows",
                    policy.id
                )));
            }
        } else {
            out.push(policy);
        }
    }
    Ok(out)
}

fn decode_row(i: usize, row: &serde_json::Value) -> Result<HoldPolicy> {
    let get = |key: &str| crate::project_decode::binding_value(row, key);
    let required = |key: &str| {
        get(key).ok_or_else(|| {
            Error::Projection(format!(
                "hold-policy row {i}: missing required binding `{key}`"
            ))
        })
    };
    let id = required("policy")?;
    let bad = |what: String| Error::Projection(format!("hold policy `{id}` {what}"));
    let pattern = required("pattern")?;
    regex::Regex::new(&pattern).map_err(|e| bad(format!("has invalid command regex: {e}")))?;
    if required("matchType")? != "must-match" {
        return Err(bad("must use matchType `must-match`".into()));
    }
    if required("kind")? != HOLD_KIND {
        return Err(bad(format!(
            "must declare operating-point kind `{HOLD_KIND}`"
        )));
    }
    let series = required("series")?;
    if series.trim().is_empty() || series.contains(char::is_whitespace) {
        return Err(bad("must name one exact gauge series".into()));
    }
    let signal_path = required("signal")?;
    if !std::path::Path::new(&signal_path).is_absolute() {
        return Err(bad("must read its signal from an absolute path".into()));
    }
    let max_age_secs = required("maxAge")?
        .parse::<u64>()
        .ok()
        .filter(|s| *s > 0)
        .ok_or_else(|| bad("maxAgeSeconds must be a positive integer".into()))?;
    let class_text = required("constraintClass")?;
    let class = ConstraintClass::parse(&class_text)
        .ok_or_else(|| bad(format!("has unknown constraintClass `{class_text}`")))?;
    let point_text = required("verificationPoint")?;
    let verification_point = VerificationPoint::parse(&point_text)
        .ok_or_else(|| bad(format!("has unknown verificationPoint `{point_text}`")))?;
    if verification_point != VerificationPoint::Pag {
        return Err(bad("is evaluated by pre-bash and must declare PAG".into()));
    }
    Ok(HoldPolicy {
        label: get("label")
            .unwrap_or_else(|| id.rsplit('/').next().unwrap_or(id.as_str()).to_string()),
        command_regex: pattern,
        series,
        signal_path,
        max_age_secs,
        retry: get("retry"),
        effect: required("effect")?,
        class,
        verification_point,
        rationale: get("rationale"),
        id,
    })
}

#[cfg(test)]
#[path = "project_hold_test.rs"]
mod project_hold_test;
