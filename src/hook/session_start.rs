//! The `SessionStart` hook adapter — context BEFORE the first mistake.
//!
//! Reads the harness payload on stdin, resolves the tracked work item, and
//! prints the work-item briefing in the `SessionStart` envelope, so the graph's
//! knowledge of the item's ground reaches the agent at assignment time rather
//! than at the first denied edit. Silent (empty stdout, exit 0) whenever there
//! is nothing honest to say: no plate, no quipu seam, or an unparseable payload.
//!
//! It renders the **L0** briefing (`crate::brief_l0`), not the full one. Every
//! line here is spent in the agent's context whether or not it is read, and for
//! an item with a wide ground the full render is most of a page before the
//! agent has done anything. L0 keeps the item, its ground PATHS and the scope
//! posture, and replaces each remaining section with one line naming its size
//! and the `quipu_ask` rung that returns it.
//!
//! The full [`crate::brief::render`] stays — `yupana hook session-start` is not
//! its only caller shape, and an operator reading a briefing by hand wants all
//! of it. What is deliberately NOT done is dropping sections silently: a short
//! briefing with no counts is indistinguishable from a graph that knew nothing,
//! and that ambiguity is the thing the census exists to remove.

use std::io::Read;
use std::path::{Path, PathBuf};

use super::HookInput;
use crate::config::YupanaConfig;

/// Run the `session-start` hook: gather, render, wrap, print. Always `Ok` —
/// a briefing failure must never fail a session open.
pub fn run_session_start(config_override: Option<&Path>) -> anyhow::Result<()> {
    let mut buf = String::new();
    std::io::stdin().lock().read_to_string(&mut buf).ok();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let input = HookInput::parse(&buf);
    let parsed = input.is_some();
    let root = input.map_or_else(|| cwd.clone(), |input| input.root(&cwd));
    let brief = YupanaConfig::resolve(config_override, &root)
        .ok()
        .and_then(|config| crate::brief::gather(&config, &root));
    // Liveness, read by the hook bundle's evidence probe: a SessionStart hook
    // that prints nothing is otherwise indistinguishable from one that never
    // ran, because "no plate" is a silent success (aegis-c6cwdz).
    crate::metrics::emit(
        "session_start_invoked",
        &invocation_fields(&buf, parsed, brief.is_some()),
    );
    if let Some(brief) = brief {
        println!(
            "{}",
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": crate::brief_l0::render_l0(&brief),
                }
            })
        );
    }
    Ok(())
}

/// The fields of a `session_start_invoked` record. Pure, so the contract is
/// testable without the real spool; the same discriminators as
/// `pre_bash_invoked`, plus whether a briefing was printed.
#[must_use]
pub fn invocation_fields(
    payload: &str,
    parsed: bool,
    briefed: bool,
) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        ("parsed", parsed.into()),
        ("payload_bytes", payload.len().into()),
        ("briefed", briefed.into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::invocation_fields;

    #[test]
    fn the_record_separates_no_briefing_from_an_unread_payload() {
        let silent = invocation_fields("{}", true, false);
        assert_eq!(silent[0], ("parsed", true.into()));
        assert_eq!(silent[1], ("payload_bytes", 2.into()));
        assert_eq!(silent[2], ("briefed", false.into()));
        let garbled = invocation_fields("not json", false, false);
        assert_eq!(garbled[0], ("parsed", false.into()));
    }
}
