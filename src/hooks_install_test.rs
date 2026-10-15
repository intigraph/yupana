use super::*;

#[test]
fn the_bundle_is_valid_owned_by_yupana_and_versioned_by_the_build() {
    let b = bundle();
    assert_eq!(b["schema"], "st.hook-bundle/1");
    assert_eq!(b["name"], "yupana");
    assert_eq!(b["owner"], "yupana");
    assert_eq!(b["version"], env!("CARGO_PKG_VERSION"));
    assert!(!hooks_for(&b, Harness::Claude).is_empty());
    assert!(!hooks_for(&b, Harness::Codex).is_empty());
}

#[test]
fn every_codex_hook_declares_evidence_so_st_can_grade_firing() {
    // aegis-5s32or.6: a codex hook without evidence can be live but never
    // graded firing; the probe found yupana's pre-edit in exactly that state.
    for h in bundle()["hooks"].as_array().unwrap() {
        if h["harnesses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "codex")
        {
            assert!(
                h["evidence"]["command"].is_string(),
                "{} has no evidence",
                h["command"]
            );
        }
    }
}

#[test]
fn merge_is_idempotent_and_keeps_foreign_hooks() {
    let b = bundle();
    let mut cfg = json!({"hooks": {"PostToolUse": [
        {"matcher": "Bash", "hooks": [{"type": "command", "command": "other-tool hook"}]}
    ]}, "model": "x"});
    let want = hooks_for(&b, Harness::Claude).len();
    assert_eq!(merge(&mut cfg, &b, Harness::Claude), want);
    assert_eq!(
        merge(&mut cfg, &b, Harness::Claude),
        0,
        "second install adds nothing"
    );
    assert_eq!(present(&cfg, &b, Harness::Claude), (want, want));
    assert_eq!(cfg["model"], "x");
    let bash = cfg["hooks"]["PostToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["matcher"] == "Bash")
        .unwrap();
    assert!(bash["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["command"] == "other-tool hook"));
}

#[test]
fn remove_takes_only_yupanas_hooks_and_drops_empty_groups() {
    let b = bundle();
    let mut cfg = json!({"hooks": {"PostToolUse": [
        {"matcher": "Bash", "hooks": [{"type": "command", "command": "other-tool hook"}]}
    ]}});
    merge(&mut cfg, &b, Harness::Claude);
    let n = hooks_for(&b, Harness::Claude).len();
    assert_eq!(remove(&mut cfg, &b, Harness::Claude), n);
    assert_eq!(present(&cfg, &b, Harness::Claude).0, 0);
    assert_eq!(
        cfg["hooks"]["PostToolUse"].as_array().unwrap().len(),
        1,
        "foreign group survives"
    );
    assert!(
        cfg["hooks"].get("PostToolUseFailure").is_none(),
        "emptied event is dropped"
    );
}

#[test]
fn codex_config_round_trips_through_toml() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "model = \"gpt\"\n[mcp_servers.x]\ncommand = \"x\"\n").unwrap();
    let b = bundle();
    let mut cfg = read_config(&path, Harness::Codex).unwrap();
    assert!(merge(&mut cfg, &b, Harness::Codex) > 0);
    write_config(&path, Harness::Codex, &cfg).unwrap();
    let back = read_config(&path, Harness::Codex).unwrap();
    let n = hooks_for(&b, Harness::Codex).len();
    assert_eq!(present(&back, &b, Harness::Codex), (n, n));
    assert_eq!(back["model"], "gpt");
    assert_eq!(back["mcp_servers"]["x"]["command"], "x");
    assert!(
        dir.path().join("config.toml.bak-yupana").exists(),
        "previous file kept"
    );
}

#[test]
fn an_absent_config_reads_as_empty() {
    let dir = tempfile::tempdir().unwrap();
    let v = read_config(&dir.path().join("nope.json"), Harness::Claude).unwrap();
    assert_eq!(v, json!({}));
}

/// `YUPANA_PROJECTION_<name>=<n>` in a command, if present.
fn budget(command: &str, name: &str) -> Option<u64> {
    let key = format!("YUPANA_PROJECTION_{name}=");
    let rest = &command[command.find(&key)? + key.len()..];
    rest.split_whitespace().next()?.parse().ok()
}

#[test]
fn every_budgeted_entry_keeps_its_ladder_per_call_below_total_below_parent() {
    // aegis-c6cwdz: yupana now authors both halves of the ladder, the budget
    // env in the command and the parent timeout st renders, so the ordering
    // assertion lives here, beside the numbers.
    let mut checked = 0;
    for h in bundle()["hooks"].as_array().unwrap() {
        let cmd = h["command"].as_str().unwrap();
        let (Some(per), Some(total)) = (
            budget(cmd, "HTTP_TIMEOUT_SECS"),
            budget(cmd, "TOTAL_BUDGET_SECS"),
        ) else {
            continue;
        };
        let parent = h["timeout"]
            .as_u64()
            .unwrap_or_else(|| panic!("{cmd} sets a budget but no parent timeout"));
        assert!(
            per < total && total < parent,
            "{cmd}: {per} < {total} < {parent}"
        );
        checked += 1;
    }
    assert_eq!(checked, 2, "brief and pre-edit both carry a ladder");
}

#[test]
fn every_entry_fails_open_and_prints_only_on_success() {
    for h in bundle()["hooks"].as_array().unwrap() {
        let cmd = h["command"].as_str().unwrap();
        let laundered = (cmd.starts_with("out=$(")
            && cmd.ends_with(") || exit 0; printf %s \"$out\""))
            || (!cmd.contains("out=$(") && cmd.ends_with(" || exit 0"));
        assert!(laundered, "not laundered: {cmd}");
    }
}

#[test]
fn the_moved_entries_are_byte_identical_to_what_st_emits() {
    // st de-duplicates a bundle hook against its own by exact (matcher,
    // command), so during the overlap each renders ONCE and removing st's copy
    // changes nothing rendered. Any byte drift here renders it twice.
    let b = bundle();
    let find = |event: &str, matcher: Option<&str>| {
        b["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|h| {
                h["event"] == event
                    && h["matcher"].as_str() == matcher
                    && h["command"].as_str().unwrap().contains("YUPANA_PROJECTION")
            })
            .cloned()
            .unwrap()
    };
    let brief = find("SessionStart", None);
    assert_eq!(
        brief["command"],
        "out=$(YUPANA_PROJECTION_HTTP_TIMEOUT_SECS=5 YUPANA_PROJECTION_TOTAL_BUDGET_SECS=8 yupana hook session-start) || exit 0; printf %s \"$out\""
    );
    assert_eq!(brief["timeout"], 10);
    assert_eq!(brief["harnesses"], json!(["claude", "codex", "opencode"]));
    let edit = find("PreToolUse", Some("Edit|Write|MultiEdit"));
    assert_eq!(
        edit["command"],
        "out=$(YUPANA_PROJECTION_HTTP_TIMEOUT_SECS=5 YUPANA_PROJECTION_TOTAL_BUDGET_SECS=20 yupana hook pre-edit) || exit 0; printf %s \"$out\""
    );
    assert_eq!(edit["timeout"], 30);
    assert_eq!(edit["harnesses"], json!(["claude", "opencode"]));
}

#[test]
fn pre_bash_stays_out_of_the_bundle() {
    // Where the deployment's Bash guard already runs pre-bash, st suppresses
    // its own copy; a bundle copy would not be suppressed and would race the
    // guard's landing record (aegis-xit7y6). It moves only once st can apply
    // that delegation to bundle items.
    for h in bundle()["hooks"].as_array().unwrap() {
        assert!(!h["command"].as_str().unwrap().contains("hook pre-bash"));
    }
}

#[cfg(unix)]
fn run_against_stub(cmd: &str, stub_body: &str) -> std::process::Output {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("yupana");
    std::fs::write(&stub, stub_body).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::process::Command::new("sh")
        .args(["-c", cmd])
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn a_stale_binary_that_rejects_the_subcommand_never_blocks() {
    // clap exits 2 on an unknown subcommand, with its error on stderr, and
    // exit 2 is Claude Code's blocking code. Every entry must turn that into
    // exit 0 with empty stdout.
    let clap = "#!/bin/sh\necho \"error: unrecognized subcommand '$2'\" >&2\nexit 2\n";
    for h in bundle()["hooks"].as_array().unwrap() {
        let cmd = h["command"].as_str().unwrap();
        let out = run_against_stub(cmd, clap);
        assert_eq!(out.status.code(), Some(0), "{cmd}");
        assert!(out.stdout.is_empty(), "{cmd} printed {:?}", out.stdout);
    }
}

#[cfg(unix)]
#[test]
fn an_entry_whose_output_reaches_the_agent_drops_a_failed_runs_stdout() {
    // The printing entries (brief, pre-edit) feed stdout to the agent, so a
    // failed run must print NOTHING, even if it wrote to stdout before failing.
    let noisy = "#!/bin/sh\necho partial\nexit 2\n";
    for h in bundle()["hooks"].as_array().unwrap() {
        let cmd = h["command"].as_str().unwrap();
        if !cmd.starts_with("out=$(") {
            continue;
        }
        let out = run_against_stub(cmd, noisy);
        assert_eq!(out.status.code(), Some(0), "{cmd}");
        assert!(out.stdout.is_empty(), "{cmd} printed {:?}", out.stdout);
    }
}

#[test]
fn a_standalone_install_keeps_the_parent_timeouts() {
    let b = bundle();
    let mut cfg = json!({});
    merge(&mut cfg, &b, Harness::Claude);
    let start = &cfg["hooks"]["SessionStart"][0]["hooks"][0];
    assert_eq!(start["timeout"], 10);
}
