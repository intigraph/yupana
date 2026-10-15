//! The docs-history advisory: product docs describe the current product only.
//!
//! After an edit to an in-scope documentation file, the lines the edit ADDED
//! are put to an external judge as one yes/no question ("does this describe
//! the current product only?"). A probability below `threshold` produces an
//! advisory asking for the present-tense rewrite. It is **advise only**: it
//! runs after the write, never denies, and every failure (no command, spawn
//! error, deadline, unparseable answer) is silence.
//!
//! The judge is an operator-configured command, not a client built in here, so
//! the deployment keeps one owner of that service's credentials and request
//! discipline. The command is run as `<command...> --state <text> --ask
//! <question>` and must print JSON carrying a `noul` probability, either as
//! `{"answers": {"<id>": {"noul": p}}}` or `{"answer": {"noul": p}}`.
//!
//! It runs at post-edit, not pre-edit, because the judge's round trip is
//! hundreds of milliseconds and the pre-edit guard's deadline is far shorter:
//! there it would almost always fail open. Advice loses nothing by arriving
//! after the write; the agent rewrites the line it just wrote.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The question the threshold was calibrated against. Changing it voids the
/// calibration; change `threshold` only with new labelled measurements.
pub const QUESTION: &str = "Does this documentation hunk describe the current product only? \
Answer no if it says what the product or the document used to be, say or do, when or that \
something changed, was corrected, renamed, revoked or restored, or narrates a past incident \
or measurement. A date or ticket id used only as a reference does not make it history. If \
none of that is present, answer yes.";

/// Added text longer than this is not judged: long state degrades the judge,
/// and a whole new document is better reviewed than scored as one hunk.
pub const MAX_TEXT: usize = 2000;

/// `[yupana.docs_history]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DocsHistoryConfig {
    /// Off unless the operator turns it on and supplies `command`.
    pub enabled: bool,
    /// The judge's argv; `--state` and `--ask` are appended.
    pub command: Vec<String>,
    /// Wall-clock budget for the judge, after which the edit gets no advice.
    pub deadline_ms: u64,
    /// Advise when P(current state only) is below this.
    pub threshold: f64,
    /// Repo-relative globs of product docs.
    pub include: Vec<String>,
    /// Repo-relative globs excluded even when included (operational runbooks).
    pub exclude: Vec<String>,
}

impl Default for DocsHistoryConfig {
    fn default() -> Self {
        let globs = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect();
        Self {
            enabled: false,
            command: Vec::new(),
            deadline_ms: 1500,
            threshold: 0.19,
            include: globs(&[
                "README.md",
                "**/README.md",
                "CLAUDE.md",
                "**/CLAUDE.md",
                "AGENTS.md",
                "**/AGENTS.md",
                "**/SKILL.md",
                "docs/reference/**",
                "docs/services/**",
                "docs/book/src/reference/**",
            ]),
            exclude: globs(&["docs/runbooks/**", "**/runbooks/**"]),
        }
    }
}

/// Why an edit got no advisory, or the score that decided it. One metrics
/// line carries this per judged edit.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// The judge's probability was below the threshold.
    History(f64),
    /// The judge's probability was at or above the threshold.
    Current(f64),
    /// The judge did not answer usably; the reason is a short stable word.
    Unknown(&'static str),
}

/// Is `rel` (repo-relative, `/`-separated) a product doc under this config?
#[must_use]
pub fn in_scope(config: &DocsHistoryConfig, rel: &str) -> bool {
    let opts = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    };
    let hit = |globs: &[String]| {
        globs
            .iter()
            .any(|g| glob::Pattern::new(g).is_ok_and(|p| p.matches_with(rel, opts)))
    };
    hit(&config.include) && !hit(&config.exclude)
}

/// The lines an edit adds: every line of `new` that is not a line of `old`.
/// Unchanged context around an edit is not the author's new claim, so it is
/// not judged. Blank lines are dropped.
#[must_use]
pub fn added_lines(old: &str, new: &str) -> String {
    let before: std::collections::HashSet<&str> = old.lines().map(str::trim_end).collect();
    new.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !before.contains(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Read the judge's `noul` probability out of its JSON output.
#[must_use]
pub fn parse_noul(stdout: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
    let p = v["answers"]
        .as_object()
        .and_then(|m| m.values().next())
        .map_or(&v["answer"], |a| a)["noul"]
        .as_f64()?;
    (0.0..=1.0).contains(&p).then_some(p)
}

/// Ask the judge, within the deadline. Never panics, never blocks past it.
#[must_use]
pub fn judge(config: &DocsHistoryConfig, text: &str) -> Verdict {
    let Some((program, args)) = config.command.split_first() else {
        return Verdict::Unknown("no_command");
    };
    let child = Command::new(program)
        .args(args)
        .arg("--state")
        .arg(text)
        .arg("--ask")
        .arg(QUESTION)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Verdict::Unknown("spawn");
    };
    let deadline = Instant::now() + Duration::from_millis(config.deadline_ms);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Verdict::Unknown("exit"),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Verdict::Unknown("deadline");
            }
        }
    }
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    match parse_noul(&out) {
        Some(p) if p < config.threshold => Verdict::History(p),
        Some(p) => Verdict::Current(p),
        None => Verdict::Unknown("unparseable"),
    }
}

/// The nearest ancestor of `file` holding `.git`, else `fallback`.
fn repo_root(file: &Path, fallback: &Path) -> PathBuf {
    file.ancestors()
        .skip(1)
        .find(|dir| dir.join(".git").exists())
        .map_or_else(|| fallback.to_path_buf(), Path::to_path_buf)
}

/// The advisory for one edited file, or `None`. `old`/`new` are the replaced
/// and replacing text (`old` empty for a `Write`). Records one metrics line
/// for every edit it judges; the path rides in it only as `[yupana.metrics]
/// record_paths` allows, like a guard record.
#[must_use]
pub fn advisory(
    config: &DocsHistoryConfig,
    recording: crate::audit::PathRecording,
    file: &Path,
    cwd: &Path,
    old: &str,
    new: &str,
) -> Option<String> {
    if !config.enabled || config.command.is_empty() {
        return None;
    }
    let root = repo_root(file, cwd);
    let rel = file
        .strip_prefix(&root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    if !in_scope(config, &rel) {
        return None;
    }
    let text = added_lines(old, new);
    if !text.chars().any(char::is_alphabetic) {
        return None;
    }
    let started = Instant::now();
    let verdict = if text.len() > MAX_TEXT {
        Verdict::Unknown("too_long")
    } else {
        judge(config, &text)
    };
    let (result, score) = match &verdict {
        Verdict::History(p) => ("history", Some(*p)),
        Verdict::Current(p) => ("current", Some(*p)),
        Verdict::Unknown(why) => (*why, None),
    };
    let mut fields: Vec<(&str, serde_json::Value)> = Vec::new();
    if let Some(path) = crate::audit::record_path(recording, &rel, file) {
        fields.push(("path", path.into()));
    }
    fields.extend([
        ("result", result.into()),
        ("score", score.into()),
        (
            "ms",
            u64::try_from(started.elapsed().as_millis())
                .unwrap_or(u64::MAX)
                .into(),
        ),
    ]);
    crate::metrics::emit("docs_history", &fields);
    let Verdict::History(p) = verdict else {
        return None;
    };
    let excerpt: String = text
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(160)
        .collect();
    Some(format!(
        "docs-history (advisory): the text you just added to {rel} reads as history \
         (P(current state only) = {p:.2}, below {threshold:.2}). Product docs describe the \
         current product. Rewrite it as the present-tense fact (\"X is not supported; use \
         Y\") instead of a date or a story of what changed; keep the guidance. Starts: \
         \"{excerpt}\"",
        threshold = config.threshold
    ))
}

#[cfg(test)]
#[path = "docs_history_test.rs"]
mod docs_history_test;
