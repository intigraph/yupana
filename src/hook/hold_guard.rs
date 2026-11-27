//! Quiet-time build-hold arm of the command guard.
//!
//! Policy is projected from Quipu (`crate::project_hold`); evidence is the
//! fleet's quiet-time gauge, read from the Prometheus textfile the policy names.
//! While that series reads 1, a matching heavy build is refused with the hold
//! named and the governed retry advice. Launches and light commands are never
//! touched: this arm runs only on commands a hold policy selects.
//!
//! FAIL CLOSED ON THE SIGNAL, OPEN ON THE POLICY. An unreadable, stale or
//! series-less textfile means "the hold may be on" and the build is refused —
//! a build that slips through during playback is the outage this exists to
//! prevent. A missing or failed policy projection is loud fail-open, exactly
//! like the memory arm: no governed rule means no rule. The deployment mode
//! stays the ceiling, so a hard policy denies only under `enforce`.

use crate::hook::pre_edit::Outcome;
#[cfg(feature = "quipu")]
use crate::policy::Mode;

/// What the hold signal says right now.
#[cfg(feature = "quipu")]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Signal {
    /// The series reads 1: heavy builds are held.
    Held,
    /// The series reads 0.
    Clear,
    /// No trustworthy reading; the reason is reported.
    Unknown(String),
}

/// Read `series` out of Prometheus textfile `text`, written `age_secs` ago.
#[cfg(feature = "quipu")]
pub(crate) fn read_signal(text: &str, series: &str, age_secs: u64, max_age_secs: u64) -> Signal {
    if age_secs > max_age_secs {
        return Signal::Unknown(format!(
            "the hold signal is {age_secs}s old (governed maximum {max_age_secs}s)"
        ));
    }
    let value = text.lines().filter(|l| !l.starts_with('#')).find_map(|l| {
        let (name, value) = l.trim().rsplit_once(char::is_whitespace)?;
        (name.trim() == series).then(|| value.trim().to_string())
    });
    match value.as_deref().map(str::parse::<f64>) {
        None => Signal::Unknown(format!("`{series}` is absent from the hold signal")),
        Some(Ok(1.0)) => Signal::Held,
        Some(Ok(0.0)) => Signal::Clear,
        Some(_) => Signal::Unknown(format!("`{series}` does not read 0 or 1")),
    }
}

#[cfg(feature = "quipu")]
fn signal_of(policy: &crate::project_hold::HoldPolicy) -> Signal {
    let path = std::path::Path::new(&policy.signal_path);
    let age = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().map_or(0, |d| d.as_secs()));
    match (std::fs::read_to_string(path), age) {
        (Ok(text), Ok(age)) => read_signal(&text, &policy.series, age, policy.max_age_secs),
        (Err(e), _) | (_, Err(e)) => {
            Signal::Unknown(format!("cannot read {}: {e}", policy.signal_path))
        }
    }
}

#[cfg(feature = "quipu")]
pub(super) fn check(payload: &str, command: &str) -> Outcome {
    // A cheap routing superset, never the decision: the graph regex decides,
    // but an unrelated shell command must not pay for a remote projection.
    if !might_be_heavy_build(command) {
        return Outcome::Allow;
    }
    let Some(input) = super::HookInput::parse(payload) else {
        return Outcome::Allow;
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let config = match crate::config::YupanaConfig::resolve(None, &input.root(&cwd)) {
        Ok(config) => config,
        Err(e) => {
            return Outcome::Notify(format!(
                "{} quiet-time build hold was NOT EVALUATED: unreadable config ({e})",
                super::CONFIG_ERROR_PREFIX
            ))
        }
    };
    if config.policy.mode == Mode::Off || !config.quipu.enabled || config.quipu.endpoint.is_empty()
    {
        return Outcome::Allow;
    }
    let mut registry = crate::project::ProjectionRegistry::new(&config.quipu.endpoint);
    if let Err(e) = super::daemon_projection::projected(&config, &mut registry) {
        let message = format!(
            "yupana: quiet-time build hold was NOT EVALUATED: governed policy projection \
             failed ({e}). The command is allowed, loudly fail-open."
        );
        eprintln!("{message}");
        return Outcome::Notify(message);
    }
    evaluate(
        registry.hold_policies(),
        command,
        signal_of,
        config.policy.mode,
    )
}

#[cfg(not(feature = "quipu"))]
pub(super) fn check(_payload: &str, _command: &str) -> Outcome {
    Outcome::Allow
}

/// Commands worth projecting policy for. A hold policy whose regex selects a
/// command outside this set is never consulted — widen it here with the data.
#[cfg(feature = "quipu")]
fn might_be_heavy_build(command: &str) -> bool {
    const WORDS: &[&str] = &[
        "cargo", "rustc", "just", "make", "cmake", "ninja", "bazel", "go", "npm", "pnpm", "yarn",
        "mvn", "gradle", "docker", "podman", "tambo", "caboodle",
    ];
    command
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-')
        .any(|part| WORDS.contains(&part))
}

/// The pure decision: every matching policy whose signal is held or unknown.
#[cfg(feature = "quipu")]
pub(crate) fn evaluate(
    policies: &[crate::project_hold::HoldPolicy],
    command: &str,
    signal_of: impl Fn(&crate::project_hold::HoldPolicy) -> Signal,
    mode: crate::policy::Mode,
) -> Outcome {
    let mut refusals = Vec::new();
    let mut blocks = false;
    for policy in policies.iter().filter(|p| p.matches(command)) {
        let signal = signal_of(policy);
        let why = match &signal {
            Signal::Clear => continue,
            Signal::Held => format!("`{}` reads 1", policy.series),
            Signal::Unknown(reason) => {
                format!("the hold state is UNKNOWN ({reason}), so it fails closed")
            }
        };
        let retry = policy
            .retry
            .clone()
            .unwrap_or_else(|| format!("retry once `{}` reads 0", policy.series));
        let policy_blocks = policy.blocks(mode);
        blocks |= policy_blocks;
        refusals.push(format!(
            "quiet-time hold `{}`: heavy build refused because {why}. Launches and light \
             work continue; {retry}.",
            policy.label
        ));
        crate::metrics::emit(
            "hold_policy",
            &[
                ("policy", policy.id.clone().into()),
                (
                    "signal",
                    if signal == Signal::Held {
                        "held"
                    } else {
                        "unknown"
                    }
                    .into(),
                ),
                (
                    "result",
                    if policy_blocks { "deny" } else { "advise" }.into(),
                ),
            ],
        );
    }
    if refusals.is_empty() {
        Outcome::Allow
    } else if blocks {
        Outcome::Deny(refusals.join("\n"))
    } else {
        Outcome::Notify(format!(
            "yupana (governed, not blocking): {}",
            refusals.join("\n")
        ))
    }
}

#[cfg(all(test, feature = "quipu"))]
#[path = "hold_guard_test.rs"]
mod hold_guard_test;
