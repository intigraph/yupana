//! Off-path writer for the disk guard's one-command-late delta (aegis-u4f).
//!
//! ## Why the write left the `pre-bash` hook
//!
//! `pre-bash` posted the previous command's disk delta to quipu's `/update`
//! INLINE, under a 3 s client timeout, inside a hook the harness kills at 5 s.
//! Measured on 2026-11-27 the store's `/update` p99 was 4.8 s, so the client
//! routinely gave up mid-write: `yupana-hook` was the main cancelled class
//! (8.3 cancelled `/update` per 30 min, HTTP 499) and tripped
//! `QuipuRepeatedTimeouts` twice. There was no timeout that worked inline: one
//! above p99 cannot fit under the harness's 5 s kill.
//!
//! The delta is a measurement of the PREVIOUS command, so nothing in this hook
//! invocation waits for it. It is handed to a detached `yupana hook
//! disk-record` child in its own process group, which survives the hook's exit
//! and the harness's kill and writes under [`WRITE_TIMEOUT`]. That bound sits
//! above the measured p99 on purpose; the write was already idempotent (keyed
//! by sample id and read back on an uncertain result), so a slow write now
//! completes instead of being abandoned.
//!
//! The child keeps the `yupana-hook` caller label and the hook's provenance
//! environment, so the store-side after-measure is read from the same series:
//! `increase(quipu_http_requests_cancelled_total{client="yupana-hook"}[30m])`.

use std::io::Write;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// The client bound on the off-path `/update`. Above the measured 4.8 s p99
/// with headroom; the process is detached, so no edit waits on it.
pub(crate) const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// Test seam: when set, `hand_off` waits for the writer, so a fixture can
/// count writes after the hook exits instead of racing the detached child.
/// Production never sets it.
const WAIT_ENV: &str = "YUPANA_DISK_RECORD_WAIT";

/// Metrics-spool kind for the background writer's outcome.
const KIND: &str = "disk_delta_write";

/// One delta to record, exactly the arguments of `aggregate::record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Job {
    pub endpoint: String,
    pub signature: String,
    pub filesystem: String,
    pub observed_at: String,
    pub delta: i64,
    pub session: Option<String>,
}

/// Start the detached writer and return without waiting for it. An error means
/// the sample was NOT handed off (and so is not recorded); it never blocks.
pub(super) fn hand_off(job: &Job) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("no current exe: {e}"))?;
    let body = serde_json::to_vec(job).map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .args(["hook", "disk-record"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut command);
    let mut child = command.spawn().map_err(|e| format!("spawn failed: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("child has no stdin")?;
    stdin.write_all(&body).map_err(|e| e.to_string())?;
    // Dropping stdin closes it (EOF for the child); dropping `child` does not
    // wait for or kill it — that is the point.
    drop(stdin);
    if std::env::var_os(WAIT_ENV).is_some() {
        child.wait().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Own process group, so a harness that kills the hook's group on timeout does
/// not take the in-flight write with it.
#[cfg(unix)]
fn detach(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn detach(_command: &mut Command) {}

/// `yupana hook disk-record`: read one [`Job`] on stdin and write it. Always
/// exits 0 — the outcome goes to the metrics spool, never to a pane.
pub fn run_disk_record() -> anyhow::Result<()> {
    let mut buf = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin().lock(), &mut buf);
    let (outcome, reason) = match serde_json::from_str::<Job>(&buf) {
        Ok(job) => match record(&job) {
            Ok(()) => ("recorded", String::new()),
            Err(e) => ("not_recorded", e),
        },
        Err(e) => ("unreadable_job", e.to_string()),
    };
    crate::metrics::emit(
        KIND,
        &[("outcome", outcome.into()), ("reason", reason.into())],
    );
    Ok(())
}

fn record(job: &Job) -> Result<(), String> {
    crate::project_disk::aggregate::record(
        &job.endpoint,
        &job.signature,
        &job.filesystem,
        &job.observed_at,
        job.delta,
        job.session.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_write_timeout_clears_the_measured_update_p99() {
        // 4.8 s was the store's /update p99 when the 3 s inline bound was
        // cancelling 8.3 writes per 30 min (aegis-u4f).
        assert!(WRITE_TIMEOUT > std::time::Duration::from_millis(4_800));
    }

    #[test]
    fn a_job_round_trips_through_the_stdin_contract() {
        let job = Job {
            endpoint: "http://quipu.test".into(),
            signature: "cargo:build".into(),
            filesystem: "root:test".into(),
            observed_at: "2026-11-27T06:00:00+00:00".into(),
            delta: -4096,
            session: None,
        };
        let back: Job = serde_json::from_slice(&serde_json::to_vec(&job).unwrap()).unwrap();
        assert_eq!(back, job);
    }
}
