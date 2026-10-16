//! Exercise the existing command hook, not a parallel admission command.
#![cfg(feature = "quipu")]
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    process::{Command, Stdio},
};
use yupana::action_certification::{sign, verify, ActionInput, SignedActionRecord};

struct Fixture {
    dir: tempfile::TempDir,
    issuer: Ed25519KeyPair,
    verifier_public: Vec<u8>,
    cache: serde_json::Value,
}

fn key() -> (Vec<u8>, Ed25519KeyPair) {
    let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .unwrap()
        .as_ref()
        .to_vec();
    let pair = Ed25519KeyPair::from_pkcs8(&bytes).unwrap();
    (bytes, pair)
}

impl Fixture {
    fn new(mode: &str, tier: &str, authority: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".bobbin")).unwrap();
        std::fs::write(root.join(".bobbin/config.toml"),format!("[yupana.policy]\nmode='{mode}'\n[yupana.quipu]\nenabled=true\nendpoint='http://127.0.0.1:1'\nprojection_cache_ttl_secs=3600\n[yupana.serve]\nuse_daemon=false\n")).unwrap();
        let (_, issuer) = key();
        let (bytes, verifier) = key();
        std::fs::write(root.join("verifier.pk8"), bytes).unwrap();
        let cache = serde_json::json!({
            "version":2,"written_at":yupana::projection_cache::now_secs(),"endpoint":"http://127.0.0.1:1",
            "policies":[],"text_rules":[],"memory_policies":[],"trajectory_policies":[],"landing_policies":[],
            "admission_policies":[{
                "id":"https://example.org/policy/admission","rationale":"canonical admission required",
                "tier":tier,"effect":if tier=="block" {"deny"} else {"warn"},"verification_point":"PAG","match_type":"must-match",
                "selector":{"repositories":[{"authority":authority,"repository":"example/project","protected_refs":["main"]}]},
                "predicate":{"revision":"c".repeat(64),"issuer_public_key":hex::encode(issuer.public_key().as_ref()),"max_age_secs":120,"required_checks":["current-admission"]}
            }]
        });
        Self {
            dir,
            issuer,
            verifier_public: verifier.public_key().as_ref().to_vec(),
            cache,
        }
    }

    fn hook(
        &self,
        command: &str,
        admission: Option<(serde_json::Value, SignedActionRecord)>,
    ) -> String {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = self.dir.path();
        std::fs::create_dir_all(root.join("tmp")).unwrap();
        std::fs::write(root.join("projection.json"), self.cache.to_string()).unwrap();
        let mut payload = serde_json::json!({"session_id":format!("admission-test-{}",SEQUENCE.fetch_add(1,std::sync::atomic::Ordering::Relaxed)),"cwd":root,"tool_name":"Bash","tool_input":{"command":command}});
        if let Some((context, proof)) = admission {
            payload["landing_admission_context"] = context;
            payload["landing_admission"] = serde_json::to_value(proof).unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_yupana"))
            .args(["hook", "pre-bash"])
            .current_dir(root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root)
            .env("TMPDIR", root.join("tmp"))
            .env("YUPANA_FAILOPEN_MARKER_DIR", root.join("markers"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("YUPANA_PROJECTION_CACHE_PATH", root.join("projection.json"))
            .env("YUPANA_ACTION_KEY", root.join("verifier.pk8"))
            .env("YUPANA_ACTION_SPOOL", root.join("admission.jsonl"))
            .env("SHANTY_AGENT", "tester")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(payload.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn proof(
        &self,
        command: &str,
        authority: &str,
        review_disposition: &str,
    ) -> (serde_json::Value, SignedActionRecord) {
        let context = serde_json::json!({"authority":authority,"repository":"example/project","pr":42,"head":"a".repeat(40),"base":"b".repeat(40),"base_ref":"main"});
        let input: ActionInput = serde_json::from_value(serde_json::json!({
            "record_id":"admission-42","correlation_id":"helper-operation-42","session":"producer-session",
            "ts":yupana::projection_cache::now_secs(),"agent":"producer","item":"task-1","verb":"merge",
            "target":"repository-1","target_class":"repo","tenant":"worker","result":"admitted",
            "repo":"example/project","sha":"a".repeat(40),"ref":"main","remote_authority":authority,
            "scope_provenance":{"pr":42,"base":"b".repeat(40),"policy_revision":"c".repeat(64),
                "command_sha256":hex::encode(Sha256::digest(command.as_bytes())),"transport":"canonical-helper-api", "review_disposition":review_disposition},
            "checks":[{"id":"current-admission","expected":true,"observed":true,"evidence_ref":"immutable-preflight-42"}]
        })).unwrap();
        (context, sign(input, &self.issuer).unwrap())
    }

    fn records(&self) -> Vec<SignedActionRecord> {
        std::fs::read_to_string(self.dir.path().join("admission.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<SignedActionRecord>(line).unwrap())
            .filter(|record| {
                record
                    .checks
                    .iter()
                    .any(|check| check.id == "landing-admission")
            })
            .collect()
    }
}

fn denied(output: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .is_some_and(|value| value["hookSpecificOutput"]["permissionDecision"] == "deny")
}

#[test]
fn advice_never_promotes_itself_and_raw_merge_is_recorded() {
    for (mode, tier) in [("advise", "warn"), ("enforce", "warn"), ("advise", "block")] {
        let fixture = Fixture::new(mode, tier, "github.com");
        let output = fixture.hook("gh pr merge 42 --repo example/project", None);
        assert!(!denied(&output), "{mode}/{tier}: {output}");
        assert!(output.contains("landing admission Missing"), "{output}");
        let records = fixture.records();
        assert_eq!(records.len(), 1);
        assert!(verify(&records[0], &fixture.verifier_public));
        assert_eq!(records[0].scope_provenance["transport"], "UNKNOWN");
        assert_eq!(records[0].certification_status, "uncertified");
    }
}

#[test]
fn controlled_block_refuses_raw_and_admits_current_helper_evidence() {
    let fixture = Fixture::new("enforce", "block", "github.com");
    let raw = fixture.hook("gh pr merge 42 --repo example/project", None);
    assert!(denied(&raw));
    println!(
        "DEMO {}",
        serde_json::json!({"case":"raw merge without admission","denied":denied(&raw),"hook_response":serde_json::from_str::<serde_json::Value>(&raw).ok()})
    );
    let command = format!(
        "gh pr merge 42 --repo example/project --match-head-commit {}",
        "a".repeat(40)
    );
    let proof = fixture.proof(&command, "github.com", "independent-review-recorded");
    let admitted_output = fixture.hook(&command, Some(proof));
    assert!(!denied(&admitted_output));
    println!(
        "DEMO {}",
        serde_json::json!({"case":"same PR with current signed admission","denied":denied(&admitted_output),"hook_response":serde_json::from_str::<serde_json::Value>(&admitted_output).ok()})
    );
    let records = fixture.records();
    assert_eq!(records.len(), 2);
    let admitted = &records[1];
    assert!(verify(admitted, &fixture.verifier_public));
    assert_eq!(admitted.sha, "a".repeat(40));
    assert_eq!(admitted.scope_provenance["pr"], 42);
    assert_eq!(
        admitted.scope_provenance["transport"],
        "canonical-helper-api"
    );
    assert_eq!(admitted.certification_status, "certified");
}

#[test]
fn proof_cannot_be_replayed_for_a_changed_command_or_forged_target() {
    let fixture = Fixture::new("enforce", "block", "github.com");
    let command = format!(
        "gh pr merge 42 --repo example/project --match-head-commit {}",
        "a".repeat(40)
    );
    let proof = fixture.proof(&command, "github.com", "independent-review-recorded");
    let changed = command.replace("merge 42", "merge 43");
    assert!(denied(&fixture.hook(&changed, Some(proof))));
    let (mut context, proof) = fixture.proof(&command, "github.com", "independent-review-recorded");
    context["base_ref"] = "topic".into();
    assert!(denied(&fixture.hook(&command, Some((context, proof)))));
}

#[test]
fn release_and_internal_review_exemptions_are_producer_decisions() {
    for (authority, command, disposition) in [
        (
            "github.com",
            "gh pr merge 42 --repo example/project",
            "release-policy-exemption",
        ),
        (
            "forge.example",
            "curl -X POST https://forge.example/api/v1/repos/example/project/pulls/42/merge",
            "internal-policy-review-exempt",
        ),
    ] {
        let fixture = Fixture::new("enforce", "block", authority);
        let proof = fixture.proof(command, authority, disposition);
        assert!(!denied(&fixture.hook(command, Some(proof))));
        let records = fixture.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].certification_status, "certified");
    }
}

#[test]
fn harmless_prose_topic_push_and_other_repositories_are_not_selected() {
    let fixture = Fixture::new("enforce", "block", "github.com");
    for command in [
        "echo 'gh pr merge 42 --repo example/project'",
        "cat <<'DOC'\ngh pr merge 42 --repo example/project\nDOC",
        "git push https://github.com/example/project.git topic",
        "gh pr merge 42 --repo other/project",
    ] {
        let output = fixture.hook(command, None);
        assert!(!denied(&output), "{output}");
        assert!(!output.contains("landing admission Missing"), "{output}");
    }
}

#[test]
fn old_cache_does_not_claim_observed_empty_or_activate_global_block() {
    let mut fixture = Fixture::new("enforce", "block", "github.com");
    fixture
        .cache
        .as_object_mut()
        .unwrap()
        .remove("admission_policies");
    let output = fixture.hook("gh pr merge 42 --repo example/project", None);
    assert!(
        output.contains("UNKNOWN, not an observed empty set"),
        "{output}"
    );
    assert!(!denied(&output));
    fixture.cache["admission_policies"] = serde_json::json!([]);
    let output = fixture.hook("gh pr merge 42 --repo example/project", None);
    assert!(!output.contains("admission"), "{output}");
    assert!(!denied(&output));
}

#[test]
fn explicit_cli_host_is_not_collapsed_to_default_github_authority() {
    let fixture = Fixture::new("enforce", "block", "forge.example");
    for command in [
        "GH_HOST=forge.example gh pr merge 42 --repo example/project",
        "gh api --hostname forge.example repos/example/project/pulls/42/merge -X POST",
        "gh pr merge 42 --repo forge.example/example/project",
    ] {
        let output = fixture.hook(command, None);
        assert!(denied(&output), "{command}: {output}");
    }
    let github = Fixture::new("enforce", "block", "github.com");
    let output = github.hook(
        "GH_HOST=forge.example gh pr merge 42 --repo example/project",
        None,
    );
    assert!(
        !denied(&output),
        "different authority is outside this explicit scope: {output}"
    );
}
