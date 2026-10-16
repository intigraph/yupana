//! Signed, path-attributed admission observations, with no raw command storage.
use super::Observation;
use crate::action_certification::{ActionInput, CheckInput};
use sha2::{Digest, Sha256};

pub(super) fn record(config: &crate::config::YupanaConfig, observed: &Observation<'_>) -> bool {
    let home = std::env::var("HOME").ok();
    let Some(path) = crate::hook::landing_guard::resolve_key_path(
        std::env::var("YUPANA_ACTION_KEY").ok().as_deref(),
        &config.quipu.signing_key_path,
        home.as_deref(),
    ) else {
        return false;
    };
    let Some(key) = crate::verdict_spool::existing_key(&path) else {
        return false;
    };
    let Some(spool) = crate::hook::landing_guard::resolve_spool(
        std::env::var("YUPANA_ACTION_SPOOL").ok().as_deref(),
        std::env::var("XDG_STATE_HOME").ok().as_deref(),
        home.as_deref(),
    ) else {
        return false;
    };
    let hook = crate::hook::HookInput::parse(observed.payload);
    let session = hook
        .as_ref()
        .and_then(|input| input.session_id.as_deref())
        .unwrap_or("UNKNOWN");
    let agent = ["SHANTY_AGENT", "GT_CREW", "YUPANA_AGENT"]
        .iter()
        .find_map(|name| std::env::var(name).ok())
        .unwrap_or_else(|| "UNKNOWN".into());
    let success = observed.verdict == crate::landing_admission::AdmissionVerdict::Satisfied;
    let context = observed.context;
    let proof_digest = observed
        .proof
        .and_then(|proof| serde_json::to_vec(proof).ok())
        .map(|bytes| hex::encode(Sha256::digest(bytes)));
    let transport = if success {
        observed
            .proof
            .and_then(|proof| proof.scope_provenance.get("transport"))
            .cloned()
            .unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::json!("UNKNOWN")
    };
    // Without a tool-use ID, timestamp alone would conflate two attempts in
    // one second. The nonce is attribution only; it is not an issuer credential.
    let nonce = hook
        .as_ref()
        .and_then(|input| input.tool_use_id.clone())
        .unwrap_or_else(|| {
            format!(
                "{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
    let record_id = format!(
        "admission-{}",
        hex::encode(Sha256::digest(format!(
            "{session}:{nonce}:{}:{}",
            observed.rule.id, observed.command_digest
        )))
    );
    let result = if success {
        "allowed"
    } else if observed.blocks {
        "refused"
    } else {
        "advised"
    };
    let input = ActionInput {
        record_id,
        correlation_id: session.into(),
        session: session.into(),
        ts: observed.now,
        agent: agent.clone(),
        item: crate::plate::observation(Some(session)).flatten(),
        verb: "merge".into(),
        target: format!("{}/{}", observed.authority, observed.repository),
        target_class: "repo".into(),
        tenant: agent,
        result: result.into(),
        repo: observed.repository.into(),
        sha: context.map(|c| c.head.clone()).unwrap_or_default(),
        git_ref: context.map(|c| c.base_ref.clone()).unwrap_or_default(),
        remote_authority: observed.authority.into(),
        scope_provenance: serde_json::json!({
            "policy":observed.rule.id,"policy_revision":observed.rule.predicate.revision,
            "pr":context.map(|c|c.pr),"base":context.map(|c|&c.base),
            "command_sha256":observed.command_digest,"proof_sha256":proof_digest,
            "transport":transport,"ref_stated_by_command":false,
            "target_provenance":if success {"verified-issuer-context"} else {"unverified-or-missing-context"},
            "predicate_verdict":format!("{:?}",observed.verdict),"enforcement_tier":observed.rule.tier,
        }),
        checks: vec![CheckInput {
            id: "landing-admission".into(),
            expected: true.into(),
            observed: success.into(),
            evidence_ref: format!("policy:{}", observed.rule.id),
        }],
    };
    crate::action_certification::sign(input, &key)
        .and_then(|record| crate::action_certification::append(&spool, &record))
        .is_ok()
}
