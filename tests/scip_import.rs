//! Offline integration controls use a checked-in rust-analyzer-produced index.
use protobuf::Message;
use std::{collections::BTreeMap, path::Path, process::Command};
use yupana::scip::{import, sha256, Binding};

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup() -> (tempfile::TempDir, Vec<u8>, Binding) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let source = include_bytes!("fixtures/scip/src/lib.rs");
    std::fs::write(dir.path().join("src/lib.rs"), source).unwrap();
    git(dir.path(), &["init", "-q"]);
    git(
        dir.path(),
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/offline-fixture.git",
        ],
    );
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    let bytes = include_bytes!("fixtures/scip/index.scip").to_vec();
    let index = scip::types::Index::parse_from_bytes(&bytes).unwrap();
    let metadata = index.metadata.as_ref().unwrap();
    let producer = metadata.tool_info.as_ref().unwrap();
    let binding = Binding {
        version: 1,
        repository: "https://example.com/offline-fixture.git".into(),
        revision: yupana::git::head_commit(dir.path()).unwrap(),
        index_sha256: sha256(&bytes),
        project_root: metadata.project_root.clone(),
        producer: producer.name.clone(),
        producer_version: producer.version.clone(),
        sources: BTreeMap::from([("src/lib.rs".into(), sha256(source))]),
    };
    (dir, bytes, binding)
}

#[test]
fn compiler_index_disambiguates_duplicate_definitions_and_keeps_spans() {
    let (dir, bytes, binding) = setup();
    let imported = import(dir.path(), &bytes, binding).unwrap();
    assert!(imported.coverage.accepted);
    assert_eq!(imported.coverage.validated_documents, 1);
    let definitions: Vec<_> = imported
        .occurrences
        .iter()
        .filter(|o| o.definition && o.symbol.contains("duplicate"))
        .collect();
    assert_eq!(definitions.len(), 2);
    assert_ne!(definitions[0].symbol, definitions[1].symbol);
    for definition in definitions {
        let related = imported.lookup(&definition.symbol);
        assert_eq!(
            related.len(),
            2,
            "one definition and exactly its own reference"
        );
        assert_eq!(related.iter().filter(|o| o.definition).count(), 1);
        let body = definition.ast_body.as_ref().expect("separate AST body");
        assert!(body.end_line > definition.identifier.end_line + 1);
        assert_eq!(body.tier, yupana::types::Tier::TreeSitter);
    }
}

#[test]
fn stale_revision_foreign_repo_and_hashes_refuse_with_coverage() {
    let (dir, bytes, binding) = setup();
    for (mut bad, reason) in [
        (binding.clone(), "foreign repository"),
        (binding.clone(), "stale repository revision"),
        (binding.clone(), "index hash mismatch"),
        (binding.clone(), "producer mismatch"),
    ] {
        match reason {
            "foreign repository" => bad.repository.push_str("/foreign"),
            "stale repository revision" => bad.revision = "0".repeat(40),
            "index hash mismatch" => bad.index_sha256 = "0".repeat(64),
            _ => bad.producer.push_str("-other"),
        }
        let coverage = import(dir.path(), &bytes, bad).unwrap_err();
        assert!(!coverage.accepted);
        assert!(coverage.refusal.unwrap().contains(reason));
    }
    std::fs::write(dir.path().join("src/lib.rs"), "// changed\n").unwrap();
    let coverage = import(dir.path(), &bytes, binding.clone()).unwrap_err();
    assert!(coverage.refusal.unwrap().contains("stale source bytes"));
    // A forged current-byte attestation still cannot enrich another revision.
    let mut forged = binding;
    forged
        .sources
        .insert("src/lib.rs".into(), sha256(b"// changed\n"));
    assert!(import(dir.path(), &bytes, forged)
        .unwrap_err()
        .refusal
        .unwrap()
        .contains("source does not match revision"));
}

#[test]
fn malformed_index_language_paths_ranges_and_coverage_fail_closed() {
    let (dir, bytes, binding) = setup();
    let mut malformed = binding.clone();
    malformed.index_sha256 = sha256(b"\xff");
    assert!(import(dir.path(), b"\xff", malformed)
        .unwrap_err()
        .refusal
        .unwrap()
        .contains("malformed SCIP"));
    let original = scip::types::Index::parse_from_bytes(&bytes).unwrap();
    for kind in 0..8 {
        let mut index = original.clone();
        match kind {
            0 => index.documents[0].language = "unsupported".into(),
            1 => index.documents[0].relative_path = "../escape.rs".into(),
            2 => index.documents[0].occurrences[0].range = vec![-1, 0, 1],
            3 => index.documents.push(index.documents[0].clone()),
            4 => index.documents.clear(),
            5 => index.documents[0].occurrences[0].symbol = "invalid".into(),
            6 => index.documents[0].position_encoding = protobuf::EnumOrUnknown::from_i32(99),
            _ => index.metadata.as_mut().unwrap().version = protobuf::EnumOrUnknown::from_i32(99),
        }
        let bytes = index.write_to_bytes().unwrap();
        let mut binding = binding.clone();
        binding.index_sha256 = sha256(&bytes);
        let coverage = import(dir.path(), &bytes, binding).unwrap_err();
        assert!(!coverage.accepted, "case {kind}");
        assert!(coverage.refusal.is_some());
    }
}

#[test]
fn cli_refusal_emits_no_facts_and_success_supports_identity_lookup() {
    let (dir, bytes, binding) = setup();
    let index_file = dir.path().join("index.scip");
    let binding_file = dir.path().join("binding.json");
    std::fs::write(&index_file, &bytes).unwrap();
    std::fs::write(&binding_file, serde_json::to_vec(&binding).unwrap()).unwrap();
    let invoke = || {
        Command::new(assert_cmd::cargo::cargo_bin!("yupana"))
            .args(["import-scip", "--root"])
            .arg(dir.path())
            .arg("--index")
            .arg(&index_file)
            .arg("--binding")
            .arg(&binding_file)
            .output()
            .unwrap()
    };
    let success = invoke();
    assert!(success.status.success());
    let report: serde_json::Value = serde_json::from_slice(&success.stdout).unwrap();
    assert_eq!(report["tier"], "scip");
    assert_eq!(report["coverage"]["accepted"], true);
    let symbol = report["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["definition"] == true && o["symbol"].as_str().unwrap().contains("duplicate"))
        .unwrap()["symbol"]
        .as_str()
        .unwrap();
    let lookup = Command::new(assert_cmd::cargo::cargo_bin!("yupana"))
        .args(["import-scip", "--root"])
        .arg(dir.path())
        .arg("--index")
        .arg(&index_file)
        .arg("--binding")
        .arg(&binding_file)
        .args(["--symbol", symbol])
        .output()
        .unwrap();
    assert!(lookup.status.success());
    let lookup: serde_json::Value = serde_json::from_slice(&lookup.stdout).unwrap();
    assert_eq!(lookup["occurrences"].as_array().unwrap().len(), 2);
    std::fs::write(&index_file, b"malformed").unwrap();
    let refused = invoke();
    assert!(!refused.status.success());
    let report: serde_json::Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(report["coverage"]["accepted"], false);
    assert_eq!(report["occurrences"].as_array().unwrap().len(), 0);
}

#[cfg(unix)]
#[test]
fn symlinked_source_is_refused_even_when_bytes_match() {
    let (dir, bytes, binding) = setup();
    let source = dir.path().join("src/lib.rs");
    let elsewhere = dir.path().join("same-bytes.rs");
    std::fs::rename(&source, &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &source).unwrap();
    assert!(import(dir.path(), &bytes, binding)
        .unwrap_err()
        .refusal
        .unwrap()
        .contains("symlink"));
}

#[test]
fn local_symbol_ids_are_scoped_per_document() {
    let (dir, bytes, mut binding) = setup();
    std::fs::copy(
        dir.path().join("src/lib.rs"),
        dir.path().join("src/other.rs"),
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(
        dir.path(),
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "second document",
        ],
    );
    binding.revision = yupana::git::head_commit(dir.path()).unwrap();
    binding
        .sources
        .insert("src/other.rs".into(), binding.sources["src/lib.rs"].clone());
    let mut index = scip::types::Index::parse_from_bytes(&bytes).unwrap();
    index.documents[0]
        .occurrences
        .retain(|o| o.symbol.contains("duplicate"));
    for occurrence in &mut index.documents[0].occurrences {
        occurrence.symbol = "local same".into();
    }
    let mut second = index.documents[0].clone();
    second.relative_path = "src/other.rs".into();
    index.documents.push(second);
    let bytes = index.write_to_bytes().unwrap();
    binding.index_sha256 = sha256(&bytes);
    let imported = import(dir.path(), &bytes, binding).unwrap();
    assert_eq!(imported.lookup("src/lib.rs::local same").len(), 4);
    assert_eq!(imported.lookup("src/other.rs::local same").len(), 4);
    assert!(imported.lookup("local same").is_empty());
}

/// Explicit fixture maintenance only; the normal suite never executes an indexer.
#[test]
#[ignore]
fn normalize_compiler_fixture() {
    let raw = std::fs::read(
        std::env::var("SCIP_FIXTURE_RAW").expect("explicit synthetic raw index path"),
    )
    .unwrap();
    let mut index = scip::types::Index::parse_from_bytes(&raw).unwrap();
    assert_eq!(index.documents.len(), 1);
    index.metadata.as_mut().unwrap().project_root = "file:///offline-scip-fixture".into();
    std::fs::write(
        "tests/fixtures/scip/index.scip",
        index.write_to_bytes().unwrap(),
    )
    .unwrap();
}
