//! Bounded command-class disk summaries; no per-run graph subjects.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The complete class vocabulary. Arguments, repository and cwd never select
/// a new graph identity. Unknown commands/subcommands use fixed buckets.
pub const CLASSES: &[&str] = &[
    "cargo:build",
    "cargo:test",
    "cargo:check",
    "cargo:clippy",
    "cargo:doc",
    "cargo:install",
    "cargo:clean",
    "cargo:other",
    "rustc:compile",
    "npm:install",
    "npm:ci",
    "npm:run",
    "npm:other",
    "pnpm:install",
    "pnpm:add",
    "pnpm:run",
    "pnpm:other",
    "yarn:install",
    "yarn:add",
    "yarn:run",
    "yarn:other",
    "docker:build",
    "docker:pull",
    "docker:load",
    "docker:other",
    "podman:build",
    "podman:pull",
    "podman:load",
    "podman:other",
    "dd:write",
    "fallocate:allocate",
    "cp:copy",
    "rsync:sync",
    "tar:archive",
    "tambo:install",
    "tambo:other",
    "other:other",
];

#[must_use]
pub fn command_class(signature: &str) -> &'static str {
    let head = signature.split('|').next().unwrap_or_default();
    if let Some(class) = CLASSES.iter().copied().find(|class| *class == head) {
        return class;
    }
    let binary = head.split(':').next().unwrap_or_default();
    match binary {
        "cargo" => "cargo:other",
        "rustc" => "rustc:compile",
        "npm" => "npm:other",
        "pnpm" => "pnpm:other",
        "yarn" => "yarn:other",
        "docker" => "docker:other",
        "podman" => "podman:other",
        "dd" => "dd:write",
        "fallocate" => "fallocate:allocate",
        "cp" => "cp:copy",
        "rsync" => "rsync:sync",
        "tar" => "tar:archive",
        "tambo" | "caboodle" => "tambo:other",
        _ => "other:other",
    }
}

#[must_use]
pub fn subject(filesystem: &str, class: &str) -> String {
    let digest = Sha256::digest(format!("disk-summary-v1\0{filesystem}\0{class}"));
    format!(
        "{}disk-summary-{}",
        crate::export::ONTO,
        hex::encode(digest)
    )
}

/// Upper endpoint of a power-of-two bucket. It never understates a signed
/// sample. There are at most 129 endpoints, independent of the run count.
fn bucket(delta: i64) -> i64 {
    if delta == 0 {
        return 0;
    }
    let magnitude = delta.unsigned_abs();
    if delta < 0 {
        let lower = 1u64 << (63 - magnitude.leading_zeros());
        return -(i128::from(lower)) as i64;
    }
    magnitude
        .checked_next_power_of_two()
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or(i64::MAX)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub version: u8,
    pub run_count: u64,
    pub max_delta_bytes: i64,
    pub last_seen: String,
    pub histogram: BTreeMap<i64, u64>,
}

impl Summary {
    pub fn first(delta: i64, observed_at: &str) -> Result<Self, String> {
        let value = Self {
            version: 1,
            run_count: 1,
            max_delta_bytes: delta,
            last_seen: observed_at.into(),
            histogram: BTreeMap::from([(bucket(delta), 1)]),
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.run_count == 0
            || self.run_count > i64::MAX as u64
            || self.histogram.len() > 129
            || self.histogram.is_empty()
            || self.last_seen.len() > 40
            || chrono::DateTime::parse_from_rfc3339(&self.last_seen).is_err()
        {
            return Err("invalid disk summary version, bounds or timestamp".into());
        }
        let mut count = 0u64;
        for (&endpoint, &n) in &self.histogram {
            if n == 0 || bucket(endpoint) != endpoint {
                return Err("invalid disk summary histogram bucket".into());
            }
            count = count.checked_add(n).ok_or("disk summary count overflow")?;
        }
        if count != self.run_count
            || self.histogram.last_key_value().map(|(k, _)| *k)
                != Some(bucket(self.max_delta_bytes))
        {
            return Err("inconsistent disk summary count or maximum".into());
        }
        Ok(())
    }

    pub fn observe(&self, delta: i64, observed_at: &str) -> Result<Self, String> {
        self.validate()?;
        let time = chrono::DateTime::parse_from_rfc3339(observed_at)
            .map_err(|_| "invalid disk sample timestamp")?;
        let mut next = self.clone();
        next.run_count = next
            .run_count
            .checked_add(1)
            .ok_or("disk summary count overflow")?;
        let count = next.histogram.entry(bucket(delta)).or_default();
        *count = count.checked_add(1).ok_or("disk summary bucket overflow")?;
        next.max_delta_bytes = next.max_delta_bytes.max(delta);
        let previous = chrono::DateTime::parse_from_rfc3339(&next.last_seen)
            .map_err(|_| "invalid disk summary timestamp")?;
        if time > previous {
            next.last_seen = observed_at.into();
        }
        next.validate()?;
        Ok(next)
    }

    /// Conservative nearest-rank lifetime p90, clipped by the exact maximum.
    /// Projection uses the exact maximum as a floor: pooling repositories into
    /// classes must not undercut an existing per-signature p90.
    #[must_use]
    pub fn p90_upper_bytes(&self) -> i64 {
        let rank = (9 * u128::from(self.run_count)).div_ceil(10);
        let mut seen = 0u128;
        for (&value, &count) in &self.histogram {
            seen += u128::from(count);
            if seen >= rank {
                return value.min(self.max_delta_bytes);
            }
        }
        self.max_delta_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TIME: &str = "2026-01-01T00:00:00Z";

    #[test]
    fn cardinality_ignores_arguments_repositories_and_cwd() {
        let mut classes = std::collections::BTreeSet::new();
        for i in 0..10_000 {
            for binary in ["cargo", "cp", "dd", "npm", "unrecognised"] {
                let signature = format!("{binary}:argument-{i}|repo:repo-{i}|cwd:subdir-{i}");
                let class = command_class(&signature);
                assert!(CLASSES.contains(&class));
                classes.insert(class);
            }
        }
        assert_eq!(classes.len(), 5);
        assert_eq!(command_class("cargo:build|repo:a|cwd:root"), "cargo:build");
        assert_eq!(
            subject("root:a", "cargo:build"),
            subject("root:a", "cargo:build")
        );
        assert_ne!(
            subject("root:a", "cargo:build"),
            subject("root:b", "cargo:build")
        );
    }

    #[test]
    fn signed_extremes_remain_bounded_and_conservative() {
        let values = [i64::MIN, -999, -3, -1, 0, 1, 3, 999, i64::MAX];
        let mut summary = Summary::first(values[0], TIME).unwrap();
        for &delta in &values[1..] {
            summary = summary.observe(delta, TIME).unwrap();
        }
        assert_eq!(summary.run_count, 9);
        assert_eq!(summary.max_delta_bytes, i64::MAX);
        assert!(summary.histogram.len() <= 129);
        for &value in &values {
            assert!(bucket(value) >= value);
        }
        assert_eq!(
            serde_json::from_str::<Summary>(&serde_json::to_string(&summary).unwrap()).unwrap(),
            summary
        );
    }

    #[test]
    fn histogram_upper_p90_and_projection_max_never_undercut_reference() {
        let mut deltas = Vec::new();
        let mut summary = Summary::first(-1000, TIME).unwrap();
        deltas.push(-1000);
        for n in 0..1000 {
            let delta = (n * 7919 % 65537) - 32000;
            summary = summary.observe(delta, TIME).unwrap();
            deltas.push(delta);
            let mut sorted = deltas.clone();
            sorted.sort_unstable();
            let exact = sorted[(9 * sorted.len()).div_ceil(10) - 1];
            assert!(summary.p90_upper_bytes() >= exact);
            assert!(summary.max_delta_bytes >= exact);
            assert!(serde_json::to_string(&summary).unwrap().len() < 8192);
        }
    }

    #[test]
    fn corrupt_or_overflowed_summary_is_refused_and_time_does_not_regress() {
        let mut summary = Summary::first(3, TIME).unwrap();
        summary = summary.observe(2, "2025-01-01T00:00:00Z").unwrap();
        assert_eq!(summary.last_seen, TIME);
        assert!(summary.observe(0, "not-a-time").is_err());
        summary.run_count += 1;
        assert!(summary.validate().is_err());
        summary.run_count = u64::MAX;
        summary.histogram = BTreeMap::from([(4, u64::MAX)]);
        assert!(summary.observe(3, TIME).is_err());
    }
}

mod io;
pub(crate) use io::{fetch_bound, record};
