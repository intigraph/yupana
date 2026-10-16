//! Admission predicate on the existing pre-action landing circuit.
use super::pre_edit::Outcome;
use crate::config::YupanaConfig;
use crate::landing::{Landing, LandingVerb, RepoRef};
use crate::landing_admission::{AdmissionTarget, AdmissionVerdict};
use crate::project::ProjectionRegistry;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[path = "admission_record.rs"]
mod recording;

/// Producer-observed current target; independently bound to signed evidence
/// and the command digest, never populated from the proof by this consumer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    authority: String,
    repository: String,
    pr: u64,
    head: String,
    base: String,
    base_ref: String,
}

struct Observation<'a> {
    payload: &'a str,
    rule: &'a crate::project_admission::ProjectedAdmission,
    context: Option<&'a Context>,
    proof: Option<&'a crate::action_certification::SignedActionRecord>,
    authority: &'a str,
    repository: &'a str,
    command_digest: &'a str,
    verdict: AdmissionVerdict,
    blocks: bool,
    now: u64,
}

pub(super) fn check(
    config: &YupanaConfig,
    registry: &ProjectionRegistry,
    payload: &str,
    landing: &Landing,
    root: &Path,
) -> Outcome {
    if landing.verb != LandingVerb::Merge {
        return Outcome::Allow;
    }
    let Some(rules) = registry.admission_policies() else {
        return Outcome::Notify("yupana: landing admission NOT EVALUATED: projection has no admission catalogue (UNKNOWN, not an observed empty set)".into());
    };
    if rules.is_empty() {
        return Outcome::Allow;
    }
    let Some((authority, repository)) = identity(landing, root) else {
        return Outcome::Notify(
            "yupana: landing admission NOT EVALUATED: command repository authority is UNKNOWN"
                .into(),
        );
    };
    let body: serde_json::Value = match serde_json::from_str(payload) {
        Ok(body) => body,
        Err(_) => {
            return Outcome::Notify(
                "yupana: landing admission NOT EVALUATED: unreadable hook payload".into(),
            )
        }
    };
    let context = body
        .get("landing_admission_context")
        .cloned()
        .and_then(|value| serde_json::from_value::<Context>(value).ok());
    let proof = body.get("landing_admission").cloned().and_then(|value| {
        serde_json::from_value::<crate::action_certification::SignedActionRecord>(value).ok()
    });
    let command_digest = hex::encode(Sha256::digest(landing.evidence.as_bytes()));
    let now = crate::projection_cache::now_secs();
    let mut outcome = Outcome::Allow;
    for rule in rules {
        if rule.validate().is_err() {
            outcome = super::pre_bash::combine_advisories(
                outcome,
                Outcome::Notify(
                    "yupana: landing admission NOT EVALUATED: invalid projected policy".into(),
                ),
            );
            continue;
        }
        // An unstated base ref must not be supplied by an unsigned context to
        // escape scope. First establish the repository candidate, then verify
        // evidence before accepting any out-of-scope ref from the producer.
        if !rule
            .selector
            .repositories
            .iter()
            .any(|scope| scope.authority == authority && scope.repository == repository)
        {
            continue;
        }
        let verdict = match context.as_ref() {
            Some(context)
                if context.authority == authority
                    && context.repository == repository
                    && !landing.parse_incomplete =>
            {
                crate::landing_admission::evaluate(
                    &rule.predicate,
                    &AdmissionTarget {
                        authority: &authority,
                        repository: &repository,
                        pull_request: context.pr,
                        head: &context.head,
                        base: &context.base,
                        base_ref: &context.base_ref,
                        command_sha256: &command_digest,
                    },
                    proof.as_ref(),
                    now,
                )
            }
            _ if body.get("landing_admission").is_none() => AdmissionVerdict::Missing,
            _ => AdmissionVerdict::Unknown,
        };
        if verdict == AdmissionVerdict::Satisfied
            && context
                .as_ref()
                .is_some_and(|context| !rule.selects(&authority, &repository, &context.base_ref))
        {
            continue;
        }
        let blocks = rule.tier == "block" && config.policy.mode == crate::policy::Mode::Enforce;
        let recorded = recording::record(
            config,
            &Observation {
                payload,
                rule,
                context: context.as_ref(),
                proof: proof.as_ref(),
                authority: &authority,
                repository: &repository,
                command_digest: &command_digest,
                verdict,
                blocks,
                now,
            },
        );
        let response = match verdict {
            AdmissionVerdict::Satisfied if recorded => Outcome::Allow,
            AdmissionVerdict::Satisfied => Outcome::Notify("yupana: admission satisfied but signed verdict NOT RECORDED; soak coverage UNKNOWN".into()),
            _ => {
                let message = format!("landing admission {:?} for {authority}/{repository}; policy {}: {}{}",verdict,rule.id,rule.rationale,if recorded { "" } else { "; signed verdict NOT RECORDED" });
                if blocks { Outcome::Deny(message) } else { Outcome::Notify(format!("yupana (governed, not blocking): {message}")) }
            }
        };
        outcome = super::pre_bash::combine_advisories(outcome, response);
    }
    outcome
}

fn remote_identity(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim().trim_end_matches('/').trim_end_matches(".git");
    let (authority, path) = if let Some((_, rest)) = raw.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        (authority.rsplit('@').next()?, path)
    } else {
        let (authority, path) = raw.split_once(':')?;
        (authority.rsplit('@').next()?, path)
    };
    if authority.is_empty()
        || path.split('/').count() != 2
        || path.split('/').any(str::is_empty)
        || raw.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some((authority.to_string(), path.to_string()))
}

fn identity(landing: &Landing, root: &Path) -> Option<(String, String)> {
    let root = landing
        .cwd_hint
        .as_ref()
        .map_or_else(|| root.to_path_buf(), |hint| root.join(hint));
    match &landing.repo {
        RepoRef::Url(raw) => remote_identity(raw),
        RepoRef::Remote(name) => {
            remote_identity(&crate::git::run(&root, &["remote", "get-url", name])?)
        }
        RepoRef::Cwd => remote_identity(&crate::git::run(&root, &["remote", "get-url", "origin"])?),
        RepoRef::Slug(slug) => {
            // A REST URL states its authority, even though the older landing
            // selector retains only the slug. Never collapse it to github.com.
            let parsed = crate::shell_command::parse(&landing.evidence);
            for command in parsed.commands {
                for word in command.words {
                    if let Some((_, rest)) = word.split_once("://") {
                        if let Some((host, path)) = rest.split_once('/') {
                            if path.contains(&format!("repos/{slug}/pulls/")) {
                                return Some((host.rsplit('@').next()?.into(), slug.clone()));
                            }
                        }
                    }
                }
            }
            let parts: Vec<_> = slug.split('/').collect();
            match parts.as_slice() {
                [owner, repo] if !owner.is_empty() && !repo.is_empty() => {
                    Some((gh_authority(&landing.evidence)?, slug.clone()))
                }
                [host, owner, repo]
                    if !host.is_empty() && !owner.is_empty() && !repo.is_empty() =>
                {
                    Some(((*host).into(), format!("{owner}/{repo}")))
                }
                _ => None,
            }
        }
    }
}

fn gh_authority(evidence: &str) -> Option<String> {
    let words = shell_words::split(evidence).ok()?;
    let program = words
        .iter()
        .position(|word| word.rsplit('/').next() == Some("gh"))?;
    let mut host = std::env::var("GH_HOST").unwrap_or_else(|_| "github.com".into());
    for prefix in &words[..program] {
        if let Some(value) = prefix.strip_prefix("GH_HOST=") {
            host = value.into();
        }
    }
    let mut specified = None;
    for (at, word) in words.iter().enumerate().skip(program + 1) {
        let value = if word == "--hostname" {
            Some(words.get(at + 1)?.as_str())
        } else {
            word.strip_prefix("--hostname=")
        };
        if let Some(value) = value {
            if specified.is_some_and(|prior| prior != value) {
                return None;
            }
            specified = Some(value);
        }
    }
    if let Some(value) = specified {
        host = value.into();
    }
    (!host.is_empty()
        && host
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c)))
    .then_some(host)
}
