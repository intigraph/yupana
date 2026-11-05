//! The native reconstruction boundary must block through the real hook protocol.
use assert_cmd::Command;

#[test]
fn unreconstructed_patch_denies_on_every_invocation_in_the_same_session() {
    let dir = tempfile::tempdir().unwrap();
    let before = "repeated\nrepeated\n";
    std::fs::write(dir.path().join("target.txt"), before).unwrap();
    let input = serde_json::json!({
        "tool_name":"apply_patch", "cwd":dir.path(),
        "session_id":format!("native-reconstruction-{}", std::process::id()),
        "tool_input":{"command":"*** Begin Patch\n*** Add File: new.txt\n+new\n*** Update File: target.txt\n@@\n repeated\n+changed\n*** End Patch"},
    }).to_string();
    for _ in 0..2 {
        let result = Command::cargo_bin("yupana")
            .unwrap()
            .current_dir(dir.path())
            .args(["hook", "pre-edit"])
            .write_stdin(input.clone())
            .assert()
            .success();
        let output: serde_json::Value =
            serde_json::from_slice(&result.get_output().stdout).unwrap();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny");
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PreToolUse");
        assert!(output["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("hunk anchor missing or ambiguous"));
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("target.txt")).unwrap(),
        before
    );
    assert!(!dir.path().join("new.txt").exists());
}
