//! Explicit commit boundary. Fail closed on missing policy, malformed data,
//! unreadable exception or Git failure. One deliberate exception: when quipu
//! cannot be reached for the rule catalogue at all, the outcome is
//! `unavailable` (exit 0, counted, loud), so an outage cannot block every
//! commit and is never mistaken for a pass. The normal pre-edit hook is unchanged.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;

use crate::commit_schedule::{self, Fact};
use crate::textrules::{self, TextRule, TextTier};

#[path = "commit_repository.rs"]
mod repository;
#[cfg(test)]
use repository::remote_urls;
use repository::repository_iri;

/// Native commit-msg/CI invocation. Neither mode changes the worktree or refs.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Repository to inspect. Reads Git objects, never unstaged file contents.
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    /// Commit-msg hook's message file. Required in staged mode.
    #[arg(long, conflicts_with = "to", required_unless_present = "to")]
    message_file: Option<PathBuf>,
    /// Exact CI range base. Must be supplied together with --to.
    #[arg(long, requires = "to")]
    base: Option<String>,
    /// Exact CI commit. Its actual commit message supplies the trailer.
    #[arg(long, requires = "base")]
    to: Option<String>,
    /// Existing Quipu policy endpoint. No caller-supplied severity override.
    #[arg(long, env = "QUIPU_SERVER")]
    endpoint: String,
}

/// One counted commit-boundary evaluation.
#[derive(Debug, Serialize)]
pub struct Report {
    /// allow, advise, refuse, unknown (refuse/unknown exit nonzero), or
    /// unavailable (quipu unreachable: exit 0, counted, never reported as allow).
    pub outcome: String,
    /// Explicit explanation; no stderr scraping required.
    pub reason: String,
    /// Governed rule ids actually evaluated.
    pub rules: Vec<String>,
    /// Added scheduler facts after multiset subtraction.
    pub additions: Vec<Fact>,
    /// Verified exception item, if one was required and admitted.
    pub exception: Option<String>,
}

impl Args {
    /// Execute, count and print the decision. Errors never become clean allows.
    pub fn run(&self) -> Result<()> {
        let report = match self.evaluate() {
            Ok(report) => report,
            Err(error) => Report {
                outcome: "unknown".into(),
                reason: error.to_string(),
                rules: Vec::new(),
                additions: Vec::new(),
                exception: None,
            },
        };
        crate::metrics::emit(
            "commit-schedule",
            &[
                ("result", serde_json::json!(report.outcome)),
                ("additions", serde_json::json!(report.additions.len())),
                ("rules", serde_json::json!(report.rules)),
            ],
        );
        println!("{}", serde_json::to_string_pretty(&report)?);
        if report.outcome == "unavailable" {
            eprintln!(
                "WARNING: commit scheduler check UNAVAILABLE: {}",
                report.reason
            );
        }
        if matches!(report.outcome.as_str(), "refuse" | "unknown") {
            bail!(
                "commit scheduler check {}: {}",
                report.outcome,
                report.reason
            );
        }
        Ok(())
    }

    fn evaluate(&self) -> Result<Report> {
        // An unreachable quipu must not refuse every commit, and it must never
        // read as a clean pass either: report `unavailable` (exit 0, counted,
        // loud). Only the rule-catalogue fetch is treated this way. A reachable
        // quipu with a broken catalogue, and any exception that cannot be
        // verified, still fail closed below.
        let fetched = match crate::project::fetch_commit_rules(&self.endpoint) {
            Err(crate::errors::Error::Unavailable(why)) => {
                return Ok(Report {
                    outcome: "unavailable".into(),
                    reason: format!(
                        "quipu unreachable; commit allowed UNVERIFIED for scheduler additions: {why}"
                    ),
                    rules: Vec::new(),
                    additions: Vec::new(),
                    exception: None,
                });
            }
            other => other?,
        };
        let rules: Vec<_> = fetched
            .into_iter()
            .filter(|r| {
                r.class
                    .as_deref()
                    .is_some_and(|s| s.starts_with(commit_schedule::RULE_CLASS))
            })
            .collect();
        ensure!(
            !rules.is_empty(),
            "governed commit scheduler policy is missing"
        );
        ensure!(
            textrules::errors(&rules).is_empty(),
            "governed commit scheduler policy does not compile"
        );
        let (base, target, message) = if let (Some(base), Some(to)) = (&self.base, &self.to) {
            let base = git_text(
                &self.repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{base}^{{commit}}"),
                ],
            )?;
            let to = git_text(
                &self.repo,
                &[
                    "rev-parse",
                    "--verify",
                    "--end-of-options",
                    &format!("{to}^{{commit}}"),
                ],
            )?;
            let message = git_text(&self.repo, &["log", "-1", "--format=%B", &to])?;
            (base, to, message)
        } else {
            // write-tree snapshots the existing index, including partial staging.
            // It writes a normal immutable Git tree object, not files or refs.
            let tree = git_text(&self.repo, &["write-tree"])?;
            let base = git_text(&self.repo, &["rev-parse", "--verify", "HEAD^{commit}"])
                .context("staged mode requires an existing HEAD; bootstrap the repository before installing the hook")?;
            let message = std::fs::read_to_string(
                self.message_file
                    .as_ref()
                    .context("message file required")?,
            )?;
            (base, tree, message)
        };
        let additions = changed_facts(&self.repo, &base, &target)?;
        let mut report = judge(&rules, additions)?;
        let exception = trailer(&message)?;
        if !report.additions.is_empty() && exception.is_some() {
            let id = exception.context("missing exception id")?;
            let parents: std::collections::BTreeSet<_> = rules
                .iter()
                .filter_map(|r| r.class.as_deref())
                .filter_map(|s| s.strip_prefix(commit_schedule::RULE_CLASS))
                .collect();
            ensure!(parents.len() == 1, "ambiguous governed exception parent");
            let parent = parents
                .first()
                .context("missing governed exception parent")?;
            validate_exception(
                &read_exception(&id, &self.endpoint)?,
                &id,
                parent,
                &repository_iri(&self.repo, &self.endpoint)?,
                &report.additions,
            )?;
            report.outcome = "allow".into();
            report.reason =
                "open migration child binds the repository and every exact unit/line".into();
            report.exception = Some(id);
        }
        Ok(report)
    }
}

/// Apply the same governed regex engine as pre-edit, with no public/internal
/// downgrade. Every fact MUST match at least one non-exempt governed rule.
pub fn judge(rules: &[TextRule], additions: Vec<Fact>) -> Result<Report> {
    ensure!(!rules.is_empty(), "missing commit scheduler rules");
    ensure!(
        textrules::errors(rules).is_empty(),
        "invalid commit scheduler rule"
    );
    let mut blocking = false;
    for fact in &additions {
        let hits = textrules::evaluate_in(rules, &fact.text(), &fact.path, None);
        ensure!(
            !hits.is_empty(),
            "scheduler fact is outside governed coverage: {}",
            fact.path
        );
        blocking |= hits.iter().any(|v| v.tier == TextTier::Block);
    }
    Ok(Report {
        outcome: if additions.is_empty() { "allow" } else if blocking { "refuse" } else { "advise" }.into(),
        reason: if additions.is_empty() { "no added scheduler definitions" } else { "scheduled work must use the governed event/reaction engine or name an open migration exception" }.into(),
        rules: rules.iter().map(|r| r.name.clone()).collect(), additions, exception: None,
    })
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    ensure!(
        output.status.success(),
        "Git object read failed ({})",
        output.status
    );
    Ok(output.stdout)
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git(repo, args)?)?.trim().into())
}

/// Inspect mode and object identity from raw NUL-delimited Git data. No rename
/// heuristic, external diff, worktree file read or shell expansion participates.
pub fn changed_facts(repo: &Path, base: &str, target: &str) -> Result<Vec<Fact>> {
    let raw = git(
        repo,
        &[
            "diff-tree",
            "--no-commit-id",
            "--raw",
            "-r",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-abbrev",
            base,
            target,
            "--",
        ],
    )?;
    let mut fields = raw.split(|b| *b == 0).filter(|s| !s.is_empty());
    let mut out = Vec::new();
    while let Some(header) = fields.next() {
        let path = std::str::from_utf8(fields.next().context("missing Git path")?)?;
        let header: Vec<_> = std::str::from_utf8(header)?.split_whitespace().collect();
        ensure!(
            header.len() == 5 && header[0].starts_with(':'),
            "malformed Git raw record"
        );
        let before_mode = &header[0][1..];
        let after_mode = header[1];
        if after_mode == "000000" {
            continue;
        }
        // Deleted submodules/symlinks are harmless; introduced opaque scheduler
        // content is not called clean. Regular binary assets are handled below.
        ensure!(
            after_mode == "000000" || after_mode == "100644" || after_mode == "100755",
            "opaque added Git mode at {path}"
        );
        let read = |mode: &str, oid: &str| -> Result<Vec<Fact>> {
            if mode == "000000" {
                return Ok(Vec::new());
            }
            ensure!(
                matches!(mode, "100644" | "100755"),
                "unmeasured old Git mode at {path}"
            );
            let size: usize = git_text(repo, &["cat-file", "-s", oid])?.parse()?;
            ensure!(size <= 4 * 1024 * 1024, "oversize changed source at {path}");
            let bytes = git(repo, &["cat-file", "blob", oid])?;
            ensure!(
                bytes.len() <= 4 * 1024 * 1024,
                "oversize changed source at {path}"
            );
            commit_schedule::facts(path, &bytes)
        };
        out.extend(commit_schedule::introduced(
            read(before_mode, header[2])?,
            read(after_mode, header[3])?,
        ));
    }
    Ok(out)
}

/// Only terminal Git trailers count, not a quoted line in the message body.
/// Duplicate/ambiguous exception trailers are refused even in advise mode.
pub fn trailer(message: &str) -> Result<Option<String>> {
    let mut child = Command::new("git")
        .args(["interpret-trailers", "--parse"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .context("trailer stdin missing")?
        .write_all(message.as_bytes())?;
    let output = child.wait_with_output()?;
    ensure!(output.status.success(), "Git trailer parser failed");
    let body = String::from_utf8(output.stdout)?;
    let mut ids = Vec::new();
    for line in body.lines() {
        if let Some((key, value)) = line.split_once(':') {
            // The later explicit ruling uses Schedule-Exception. The older
            // Scheduler spelling is an error, never a silently ignored escape.
            ensure!(
                !key.eq_ignore_ascii_case("Scheduler-Exception"),
                "use Schedule-Exception, not Scheduler-Exception"
            );
            if key.eq_ignore_ascii_case("Schedule-Exception") {
                ids.push(value.trim().to_owned());
            }
        }
    }
    ensure!(ids.len() <= 1, "multiple schedule exception trailers");
    if let Some(id) = ids.first() {
        ensure!(
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')),
            "invalid schedule exception item id"
        );
    }
    Ok(ids.pop())
}

fn read_exception(id: &str, endpoint: &str) -> Result<serde_json::Value> {
    // Existing deployment-selected Seeds reader owns credentials and graph
    // configuration. No new store, signing identity, HTTP credential or retry.
    let stdout = tempfile::tempfile()?;
    let mut child = Command::new("sd")
        .args(["--quipu", endpoint, "show", id, "--json"])
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("exception reader timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    ensure!(status.success(), "exception reader failed");
    ensure!(
        stdout.metadata()?.len() <= 1024 * 1024,
        "exception response exceeds bound"
    );
    use std::io::{Read, Seek, SeekFrom};
    let mut stdout = stdout;
    stdout.seek(SeekFrom::Start(0))?;
    let mut body = String::new();
    stdout.read_to_string(&mut body)?;
    Ok(serde_json::from_str(&body)?)
}

/// An exception is live data: exactly one OPEN direct child of the governed
/// parent, with a description naming every exact added unit/line. No signature
/// or self-declared permission in the commit message can substitute for it.
pub fn validate_exception(
    value: &serde_json::Value,
    id: &str,
    parent: &str,
    repository: &str,
    facts: &[Fact],
) -> Result<()> {
    let rows = value
        .as_array()
        .context("exception reader did not return item rows")?;
    ensure!(
        rows.len() == 1,
        "exception must resolve to exactly one item"
    );
    let item = &rows[0];
    ensure!(
        item["id"].as_str() == Some(id),
        "exception reader returned a different item"
    );
    ensure!(
        item["status"].as_str() == Some("open"),
        "exception item is not OPEN"
    );
    ensure!(
        item["parent"].as_str() == Some(parent),
        "exception is not a direct child of the governed migration parent"
    );
    let description = item["description"]
        .as_str()
        .context("exception has no readable description")?;
    let repositories: Vec<_> = description
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Repository:"))
        .map(str::trim)
        .collect();
    ensure!(
        repositories.len() == 1,
        "exception requires exactly one Repository: IRI field"
    );
    ensure!(
        repositories[0] == repository,
        "exception repository does not match the checked repository"
    );
    for fact in facts {
        ensure!(
            description
                .lines()
                .any(|line| line.trim().trim_matches('`') == fact.exception_target),
            "exception does not name exact unit/line {}",
            fact.exception_target
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "commit_check_test.rs"]
mod tests;
