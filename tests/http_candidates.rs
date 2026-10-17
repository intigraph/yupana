//! Expected-set controls for the bounded static profile; no network or graph.
#![cfg(feature = "langs-extra")]
use yupana::http_candidates::{at, extract, verify_current};

#[test]
fn decorators_mounts_methods_and_nested_callers() {
    let src = r#"from fastapi import FastAPI, APIRouter
import requests as rq
app = FastAPI()
r = APIRouter(prefix="/api")
app.include_router(r, prefix="/v1")
app.include_router(r, prefix="/v2")
@r.get("/items")
def listing():
    def inner():
        return rq.get("https://example.com/api/items?secret=redacted")
    return inner()
def posting():
    pass
r.add_api_route("/items", posting, methods=["POST", "PUT"])
"#;
    let file = extract(src, "api.py").unwrap();
    assert!(file.diagnostics.is_empty());
    let rows: Vec<_> = file
        .candidates
        .iter()
        .filter(|c| c.role == "producer")
        .map(|c| {
            (
                c.method.as_deref().unwrap(),
                c.path.as_deref().unwrap(),
                c.resolution.as_str(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("GET", "/v1/api/items", "literal"),
            ("GET", "/v2/api/items", "literal"),
            ("POST", "/v1/api/items", "literal"),
            ("POST", "/v2/api/items", "literal"),
            ("PUT", "/v1/api/items", "literal"),
            ("PUT", "/v2/api/items", "literal")
        ]
    );
    let c = file
        .candidates
        .iter()
        .find(|c| c.role == "consumer")
        .unwrap();
    assert_eq!(c.caller.as_ref().unwrap().name, "inner");
    assert_eq!(c.caller.as_ref().unwrap().scope, vec!["listing"]);
    assert_eq!(c.path.as_deref(), Some("/api/items"));
    assert_eq!(c.query_present, Some(true));
    assert!(!serde_json::to_string(&file).unwrap().contains("secret="));
    assert!(file
        .candidates
        .iter()
        .all(|c| c.occurrence_span.end_byte > c.occurrence_span.start_byte));
    let ids: std::collections::BTreeSet<_> = file.candidates.iter().map(|c| &c.id).collect();
    assert_eq!(ids.len(), file.candidates.len());
}
#[test]
fn dynamic_unknown_bindings_comments_and_credentials_are_not_exact() {
    let src = r#"from fastapi import FastAPI, APIRouter
import requests
app = FastAPI()
r = APIRouter(prefix=prefix)
app.include_router(r)
@r.get("/items")
def route(): pass
@app.post(dynamic)
def dynamic_route(): pass
# requests.get("https://ignored.example/a")
example = 'requests.get("https://ignored.example/b")'
def caller(requests):
    requests.get("https://example.com/a")
    requests.get("/relative")
    requests.get("https://name:password@example.com/private")
    arbitrary.get("https://ignored.example/c")
"#;
    let file = extract(src, "dynamic.py").unwrap();
    assert_eq!(file.candidates.len(), 5);
    assert!(file.candidates.iter().all(|c| c.resolution == "unresolved"));
    assert!(!serde_json::to_string(&file).unwrap().contains("password"));
    assert!(file.candidates.iter().any(|c| c
        .unresolved_reasons
        .iter()
        .any(|r| r == "unknown_router_prefix")));
}
#[test]
fn malformed_python_is_explicit_and_paths_preserve_literal_bytes() {
    let malformed = extract("def broken(:", "bad.py").unwrap();
    assert!(malformed.candidates.is_empty());
    assert!(!malformed.diagnostics.is_empty());
    let good = extract(
        "import httpx\nhttpx.get('https://example.com/a/../B%2fc//?x=y')\n",
        "a.py",
    )
    .unwrap();
    assert_eq!(good.candidates[0].path.as_deref(), Some("/a/../B%2fc//"));
}
#[test]
fn committed_snapshot_excludes_working_edits_and_changes_identity_on_revision() {
    let d = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .arg("-C")
            .arg(d.path())
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.com"]);
    let p = d.path().join("a.py");
    std::fs::write(
        &p,
        "import requests\nrequests.get('https://example.com/a')\n",
    )
    .unwrap();
    git(&["add", "a.py"]);
    git(&["commit", "-qm", "one"]);
    let first = at(d.path(), "fixture", "HEAD").unwrap();
    assert_eq!(first.coverage.literal_records, 1);
    verify_current(d.path(), "fixture", &first).unwrap();
    assert!(verify_current(d.path(), "foreign", &first).is_err());
    assert_eq!(first.files[0].snapshot_state, "verified_commit");
    std::fs::write(
        &p,
        "import requests\nrequests.post('https://example.com/b')\n",
    )
    .unwrap();
    assert_eq!(at(d.path(), "fixture", "HEAD").unwrap().files, first.files);
    assert!(verify_current(d.path(), "fixture", &first).is_err());
    let mut forged = first.clone();
    use sha2::{Digest, Sha256};
    forged.files[0].source_sha256 = hex::encode(Sha256::digest(std::fs::read(&p).unwrap()));
    assert!(verify_current(d.path(), "fixture", &forged).is_err());
    git(&["add", "a.py"]);
    git(&["commit", "-qm", "two"]);
    let second = at(d.path(), "fixture", "HEAD").unwrap();
    assert_ne!(first.revision, second.revision);
    assert_ne!(
        first.files[0].candidates[0].id,
        second.files[0].candidates[0].id
    );
    assert_ne!(first.files[0].source_sha256, second.files[0].source_sha256);
}

#[test]
fn ambiguous_rebindings_unmounted_generated_and_stale_admission() {
    for binding in [
        "def requests(): pass",
        "requests, other = pair",
        "import arbitrary as requests",
        "from arbitrary import requests",
        "del requests",
    ] {
        let src = format!("import requests\n{binding}\nrequests.get('https://example.com/a')\n");
        let f = extract(&src, "a.py").unwrap();
        assert_eq!(f.candidates.len(), 1);
        assert_eq!(f.candidates[0].resolution, "unresolved", "{binding}");
    }
    let generated = extract(
        "# @generated\nimport requests\nrequests.get('https://example.com:443/a')\n",
        "g.py",
    )
    .unwrap();
    assert_eq!(generated.candidates[0].resolution, "unresolved");
    assert_eq!(
        generated.candidates[0].authority.as_ref().unwrap().port,
        Some(443)
    );
    let unmounted = extract(
        "from fastapi import APIRouter\nr=APIRouter()\n@r.get('/a')\ndef f(): pass\n",
        "r.py",
    )
    .unwrap();
    assert_eq!(unmounted.candidates[0].resolution, "unresolved");
    assert!(extract("", "../unsafe.py").is_err());
}

#[test]
fn dynamic_registration_and_constructor_shadowing_stay_unresolved() {
    for tail in [
        "app.add_api_route('/a', f)",
        "app.add_api_route('/a', f, methods=dynamic)",
        "app.add_api_route('/a', f, methods=['GET', dynamic])",
    ] {
        let source = format!("from fastapi import FastAPI\napp=FastAPI()\ndef f(): pass\n{tail}\n");
        let f = extract(&source, "a.py").unwrap();
        assert!(f
            .candidates
            .iter()
            .any(|c| c.method.is_none() && c.resolution == "unresolved"));
    }
    let source="from fastapi import FastAPI\nFastAPI = other\napp = FastAPI()\n@app.get('/a')\ndef f(): pass\n";
    let f = extract(source, "a.py").unwrap();
    assert_eq!(f.candidates[0].resolution, "unresolved");
    let duplicate="from fastapi import FastAPI\napp=FastAPI()\ndef f(): pass\napp.add_api_route('/a', f, methods=['GET','GET'])\n";
    assert_eq!(extract(duplicate, "a.py").unwrap().candidates.len(), 1);
}

#[test]
fn nonabsolute_urls_and_unsafe_revision_inputs_fail_explicitly() {
    for url in [
        "https:example.com/a",
        "https:///example.com/a",
        "/relative",
        "ftp://example.com/a",
    ] {
        let source = format!("import requests\nrequests.get('{url}')\n");
        let f = extract(&source, "a.py").unwrap();
        assert_eq!(f.candidates[0].resolution, "unresolved", "{url}");
    }
    let root = tempfile::tempdir().unwrap();
    assert!(at(root.path(), "fixture", "--help").is_err());
}
