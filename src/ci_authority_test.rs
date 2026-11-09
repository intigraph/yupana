use super::*;
use serde_json::{json, Value};

const REPO: &str = "github.com/example/project";
const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn manifest() -> Value {
    json!({"schema":1,"repository":REPO,"source_sha":SOURCE,"checks":[{
        "id":"syntax","argv":["scripts/check-static"],"scopes":["src/**"],
        "static_only":true,"network":false,"timeout_seconds":30,"output_bytes":65536
    }]})
}

fn projection() -> Value {
    json!({"truncated":false,"checks":[{"repository":REPO,"check_id":"syntax"}]})
}

fn evaluate(manifest: &Value, projection: &Value, paths: &[&str]) -> Result<Plan> {
    let bytes = serde_json::to_vec(manifest).unwrap();
    plan(
        REPO,
        &hex::encode(Sha256::digest(&bytes)),
        SOURCE,
        &bytes,
        &serde_json::to_vec(projection).unwrap(),
        &paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
    )
}

#[test]
fn matching_static_advice_is_bound_and_complete_nonmatch_is_quiet() {
    let Plan::Advise(advice) = evaluate(&manifest(), &projection(), &["src/lib.rs"]).unwrap()
    else {
        panic!("matching reviewed check must be advised")
    };
    assert_eq!(advice.len(), 1);
    assert_eq!(advice[0].repository, REPO);
    assert_eq!(advice[0].source_sha, SOURCE);
    assert_eq!(advice[0].argv, ["scripts/check-static"]);
    assert_eq!(
        evaluate(&manifest(), &projection(), &["docs/readme.md"]).unwrap(),
        Plan::Quiet
    );
    assert_eq!(
        evaluate(
            &manifest(),
            &json!({"truncated":false,"checks":[]}),
            &["src/lib.rs"]
        )
        .unwrap(),
        Plan::Unknown
    );
}

#[test]
fn same_path_in_another_repository_cannot_select_a_command() {
    let mut map = projection();
    map["checks"][0]["repository"] = json!("github.com/other/project");
    for path in ["src/lib.rs", "docs/readme.md"] {
        assert!(evaluate(&manifest(), &map, &[path]).is_err());
    }
    let mut owned = manifest();
    owned["repository"] = json!("github.com/other/project");
    assert!(evaluate(&owned, &projection(), &["src/lib.rs"]).is_err());
}

#[test]
fn graph_command_text_unknown_ids_and_partial_maps_are_refused() {
    for key in ["command", "commandText", "argv", "timeout_seconds"] {
        let mut map = projection();
        map["checks"][0][key] = json!("touch injected");
        assert!(evaluate(&manifest(), &map, &["src/lib.rs"]).is_err());
    }
    for marker in [json!(true), json!(null), json!("false")] {
        let mut map = projection();
        map["truncated"] = marker;
        assert!(evaluate(&manifest(), &map, &["docs/readme.md"]).is_err());
    }
    let mut map = projection();
    map.as_object_mut().unwrap().remove("truncated");
    assert!(evaluate(&manifest(), &map, &["src/lib.rs"]).is_err());
    let mut map = projection();
    map["checks"][0]["check_id"] = json!("unreviewed");
    assert!(evaluate(&manifest(), &map, &["docs/readme.md"]).is_err());
}

#[test]
fn source_manifest_tampering_and_replayed_review_digest_are_refused() {
    let bytes = serde_json::to_vec(&manifest()).unwrap();
    let digest = hex::encode(Sha256::digest(&bytes));
    let map = serde_json::to_vec(&projection()).unwrap();
    let mut changed = manifest();
    changed["checks"][0]["argv"] = json!(["scripts/unreviewed"]);
    assert!(plan(
        REPO,
        &digest,
        SOURCE,
        &serde_json::to_vec(&changed).unwrap(),
        &map,
        &[]
    )
    .is_err());
    assert!(plan(
        REPO,
        &digest,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        &bytes,
        &map,
        &[]
    )
    .is_err());
    for identity in [
        "",
        "project",
        "https://github.com/example/project",
        "github.com/../project",
    ] {
        assert!(plan(identity, &digest, SOURCE, &bytes, &map, &[]).is_err());
    }
}

#[test]
fn unsafe_or_unbounded_manifest_is_refused_even_for_nonmatching_paths() {
    for (field, value) in [
        ("static_only", json!(false)),
        ("network", json!(true)),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(31)),
        ("output_bytes", json!(65537)),
        ("output_bytes", json!(0)),
        ("argv", json!([])),
        ("argv", json!(["../escape"])),
        ("argv", json!(["/absolute"])),
        ("scopes", json!(["src/["])),
        ("scopes", json!(["../escape"])),
    ] {
        let mut owned = manifest();
        owned["checks"][0][field] = value;
        assert!(
            evaluate(&owned, &projection(), &["docs/readme.md"]).is_err(),
            "{field}"
        );
    }
    for path in ["", "../escape", "/absolute", "src/../escape"] {
        assert!(evaluate(&manifest(), &projection(), &[path]).is_err());
    }
}

#[test]
fn duplicates_and_total_budget_overflow_cannot_produce_partial_advice() {
    let mut owned = manifest();
    let first = owned["checks"][0].clone();
    owned["checks"].as_array_mut().unwrap().push(first);
    assert!(evaluate(&owned, &projection(), &["src/lib.rs"]).is_err());
    let mut map = projection();
    let first = map["checks"][0].clone();
    map["checks"].as_array_mut().unwrap().push(first);
    assert!(evaluate(&manifest(), &map, &["src/lib.rs"]).is_err());
    let mut owned = manifest();
    let mut map = projection();
    for id in ["second", "third"] {
        let mut next = owned["checks"][0].clone();
        next["id"] = json!(id);
        owned["checks"].as_array_mut().unwrap().push(next);
        map["checks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"repository":REPO,"check_id":id}));
    }
    assert!(evaluate(&owned, &map, &["src/lib.rs"]).is_err());
}
