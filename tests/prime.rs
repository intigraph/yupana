//! Read-only stack-prime conformance against the actual binary.
//! No unreachable-server row: prime never contacts a server.

use assert_cmd::Command;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn command(root: &Path) -> Command {
    let mut cmd = Command::cargo_bin("yupana").unwrap();
    cmd.current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("YUPANA_METRICS_PATH", root.join("metrics.jsonl"))
        .env("YUPANA_PROJECTION_CACHE_PATH", root.join("projection.json"))
        .env("RUST_LOG", "error")
        .timeout(Duration::from_secs(2));
    cmd
}

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("home")).unwrap();
    dir
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, String> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, String>) {
        let info = std::fs::symlink_metadata(path).unwrap();
        let bytes = if info.is_file() {
            std::fs::read(path).unwrap()
        } else {
            Vec::new()
        };
        out.insert(
            path.strip_prefix(root).unwrap().to_path_buf(),
            format!(
                "{:?}:{}:{:?}:{bytes:?}",
                info.permissions(),
                info.len(),
                info.modified().unwrap()
            ),
        );
        if info.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), out);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

#[test]
fn primer_and_generated_skill_docs_cannot_drift() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let primer = std::fs::read_to_string(root.join("src/primer.md")).unwrap();
    let book = std::fs::read_to_string(root.join("docs/book/src/getting-started/agent-primer.md"))
        .unwrap();
    let skill = std::fs::read_to_string(root.join("skills/yupana/SKILL.md")).unwrap();
    let body = skill
        .split_once("<!-- BEGIN yupana prime -->\n")
        .unwrap()
        .1
        .split_once("<!-- END yupana prime -->")
        .unwrap()
        .0;
    assert_eq!(
        book, primer,
        "stale docs: run python3 scripts/sync-primer.py"
    );
    assert_eq!(
        body, primer,
        "stale skill: run python3 scripts/sync-primer.py"
    );
    let headings = primer
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .collect::<Vec<_>>();
    assert_eq!(
        &headings[..4],
        &[
            "What yupana is",
            "Agent workflow",
            "Core commands",
            "MCP tools"
        ]
    );
    assert_eq!(headings.last(), Some(&"Next steps"));
}

#[test]
fn missing_state_static_brief_json_sections_are_read_only() {
    for args in [
        vec![],
        vec!["--json"],
        vec!["--brief"],
        vec!["--brief", "--static"],
        vec!["--brief", "--json"],
        vec!["--static", "--json"],
        vec!["--section", "CoRe", "--static"],
        vec!["--section", "unknown"],
        vec!["--section", "hook"],
        vec!["--unknown"],
        vec!["--section", ""],
    ] {
        let dir = fixture();
        let root = dir.path();
        let before = snapshot(root);
        let mut cmd = command(root);
        cmd.arg("prime").args(&args);
        let started = Instant::now();
        let out = cmd.output().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1), "{args:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        if args.contains(&"unknown")
            || args.contains(&"hook")
            || args.contains(&"--unknown")
            || args.contains(&"")
        {
            assert_eq!(out.status.code(), Some(2), "{args:?}");
            if args.contains(&"unknown") {
                assert!(String::from_utf8(out.stderr)
                    .unwrap()
                    .contains("What yupana is, Agent workflow, Core commands, MCP tools"));
            }
        } else {
            assert!(
                out.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            if args.contains(&"--json") {
                let obj: Value = serde_json::from_str(&text).unwrap();
                for key in ["schema", "tool", "version", "primer", "live", "status"] {
                    assert!(obj.get(key).is_some(), "{key}");
                }
                assert_eq!(obj["schema"], "stack-prime/1");
                assert_eq!(obj["tool"], "yupana");
                assert!(!obj["version"].as_str().unwrap().is_empty());
                assert!(obj["primer"]["sections"].is_array());
                assert_eq!(
                    obj["primer"].get("text").is_some(),
                    !args.contains(&"--brief")
                );
                assert!(obj["live"]["summary"].is_object());
                assert!(obj["live"]["elapsed_ms"].as_u64().unwrap() < 1000);
                assert_eq!(
                    obj["live"]["status"],
                    if args.contains(&"--static") {
                        "skipped"
                    } else {
                        "absent"
                    }
                );
                assert_eq!(
                    obj["status"],
                    if args.contains(&"--static") {
                        "ok"
                    } else {
                        "degraded"
                    }
                );
            } else if args.contains(&"--brief") {
                let lines = text.trim().lines().collect::<Vec<_>>();
                assert!((5..=10).contains(&lines.len()), "{text}");
                assert!(lines[0].starts_with("yupana "));
                assert_eq!(
                    lines.last(),
                    Some(&"Run `yupana prime` for the full primer.")
                );
            } else if args.contains(&"--section") {
                assert!(text.starts_with("## Core commands"));
                assert!(!text.contains("## MCP tools"));
                assert!(!text.contains("## Live state"));
            } else {
                assert!(text.contains("# Yupana agent primer"));
                assert!(text.contains("## Next steps"));
                assert!(text.contains("## Live state"));
            }
        }
        assert_eq!(
            before,
            snapshot(root),
            "prime changed HOME/cwd/cache or metrics: {args:?}"
        );
    }
}

#[test]
fn effective_config_and_compiled_tiers_remain_distinct() {
    let dir = fixture();
    let root = dir.path();
    let cfg = root.join("explicit.toml");
    std::fs::write(
        &cfg,
        "[yupana]\nbase_ref = 'reviewed-base'\nenable_lsp = false\nenable_cpg = true\n",
    )
    .unwrap();
    let before = snapshot(root);
    let out = command(root)
        .args(["prime", "--json", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    assert!(out.status.success());
    let obj: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(obj["live"]["status"], "ok");
    assert_eq!(obj["live"]["summary"]["base_ref"], "reviewed-base");
    assert_eq!(
        obj["live"]["summary"]["tiers"],
        json!(yupana::types::Tier::served())
    );
    assert!(obj["live"]["summary"].get("configured_tiers").is_none());
    assert!(obj["live"]["summary"]["governed_rules"]["count"].is_null());
    assert_eq!(before, snapshot(root));
    std::fs::write(&cfg, "[broken").unwrap();
    let out = command(root)
        .args(["prime", "--json", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    assert!(out.status.success());
    let obj: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(obj["live"]["status"], "unreadable");
    let out = command(root)
        .args(["prime", "--static", "--json", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    assert!(out.status.success());
    let obj: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(obj["live"]["status"], "skipped");
}

#[cfg(feature = "quipu")]
#[test]
fn cached_governed_counts_distinguish_present_empty_expired_and_unknown() {
    let dir = fixture();
    let root = dir.path();
    let cfg = root.join("explicit.toml");
    let endpoint = "http://127.0.0.1:1";
    std::fs::write(&cfg,format!("[yupana.quipu]\nenabled = true\nendpoint = '{endpoint}'\nprojection_cache_ttl_secs = 3600\n")).unwrap();
    let cache = root.join("projection.json");
    let invoke = || {
        let before = snapshot(root);
        let out = command(root)
            .args(["prime", "--json", "--config"])
            .arg(&cfg)
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(before, snapshot(root));
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    let out = invoke();
    assert_eq!(out["live"]["status"], "absent");
    assert!(out["live"]["summary"]["governed_rules"]["count"].is_null());
    let now = yupana::projection_cache::now_secs();
    let mut data = json!({"version":yupana::projection_cache::CACHE_VERSION,"written_at":now,"endpoint":endpoint,"policies":[],
        "text_rules":[{"name":"private-rule-fixture","pattern":"fixture","tier":yupana::textrules::TextTier::Warn}]});
    std::fs::write(&cache, data.to_string()).unwrap();
    let out = invoke();
    assert_eq!(out["live"]["status"], "ok");
    assert_eq!(out["live"]["summary"]["governed_rules"]["count"], 1);
    assert_eq!(
        out["live"]["summary"]["governed_rules"]["enforcement"],
        "not_measured"
    );
    data["text_rules"] = json!([]);
    std::fs::write(&cache, data.to_string()).unwrap();
    assert_eq!(invoke()["live"]["summary"]["governed_rules"]["count"], 0);
    data["written_at"] = json!(now.saturating_sub(3601));
    std::fs::write(&cache, data.to_string()).unwrap();
    let out = invoke();
    assert_eq!(out["live"]["status"], "partial");
    assert_eq!(
        out["live"]["summary"]["governed_rules"]["status"],
        "expired"
    );
    let brief = command(root)
        .args(["prime", "--brief", "--config"])
        .arg(&cfg)
        .output()
        .unwrap();
    assert!(brief.status.success());
    let text = String::from_utf8(brief.stdout).unwrap();
    assert!((5..=10).contains(&text.trim().lines().count()), "{text}");
    assert!(text.contains("expired"));

    data["endpoint"] = json!("http://127.0.0.1:2");
    std::fs::write(&cache, data.to_string()).unwrap();
    let out = invoke();
    assert_eq!(out["live"]["status"], "unreadable");
    assert!(out["live"]["summary"]["governed_rules"]["count"].is_null());
    std::fs::write(&cache, "not json").unwrap();
    assert_eq!(invoke()["live"]["status"], "unreadable");
    std::fs::write(&cache, vec![b' '; 1024 * 1024 + 1]).unwrap();
    assert_eq!(invoke()["live"]["status"], "unreadable");
}

#[test]
fn malformed_real_hook_remains_fail_open() {
    let dir = fixture();
    let out = command(dir.path())
        .args(["hook", "not-a-real-event"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("policy guard failed open"));
}

#[test]
fn hook_arguments_and_global_values_do_not_change_the_prime_exit_code() {
    let dir = fixture();
    let before = snapshot(dir.path());
    for args in [
        vec!["prime", "--section", "hook"],
        vec!["--json", "prime", "--section", "hook"],
        vec!["--config", "hook", "prime", "--section", "hook"],
        vec!["--tenant", "hook", "prime", "--section", "hook"],
        vec!["--config=hook", "prime", "--section", "hook"],
    ] {
        let out = command(dir.path()).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(!String::from_utf8_lossy(&out.stderr).contains("policy guard failed open"));
    }
    for args in [
        vec!["hook", "not-a-real-event"],
        vec!["--json", "hook", "not-a-real-event"],
        vec!["--config", "hook", "hook", "not-a-real-event"],
        vec!["--tenant", "hook", "hook", "not-a-real-event"],
    ] {
        let out = command(dir.path()).args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("policy guard failed open"));
    }
    assert_eq!(before, snapshot(dir.path()));
}
