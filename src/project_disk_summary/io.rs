//! Existing subject-bound conditional update protocol; no new server endpoint.
use super::{command_class, subject, Summary};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::Path,
    time::{Duration, Instant},
};

const RAW_BYTES: usize = 65_536;
const BUDGET: Duration = Duration::from_secs(3);

fn remaining(start: Instant) -> Result<Duration, String> {
    let local = BUDGET
        .checked_sub(start.elapsed())
        .filter(|d| !d.is_zero())
        .ok_or("disk summary operation deadline reached")?;
    let shared = crate::projection_budget::call_timeout()
        .ok_or("hook projection budget spent; disk request not started")?;
    Ok(local.min(shared))
}

fn read(endpoint: &str, iri: &str, start: Instant) -> Result<Option<(String, Summary)>, String> {
    let sparql = format!(
        "PREFIX a: <{}> SELECT ?state WHERE {{ <{iri}> a:diskImpactState ?state }} LIMIT 2",
        crate::export::ONTO
    );
    let response = crate::quipu_label::json_post(
        &format!("{}/query", endpoint.trim_end_matches('/')),
        crate::quipu_label::HOOK,
    )
    .set("Accept", "application/sparql-results+json")
    .timeout(remaining(start)?)
    .send_string(&serde_json::json!({"query": sparql}).to_string())
    .map_err(|e| e.to_string())?;
    if response
        .header("X-Quipu-Truncated")
        .is_some_and(|s| !s.trim().eq_ignore_ascii_case("false"))
    {
        return Err("truncated disk summary response".into());
    }
    let mut text = String::new();
    response
        .into_reader()
        .take(32_769)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > 32_768 {
        return Err("oversized disk summary response".into());
    }
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if value.get("truncated").and_then(serde_json::Value::as_bool) == Some(true) {
        return Err("truncated disk summary read".into());
    }
    let rows = crate::project_decode::rows_of(&value).map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() != 1 {
        return Err("multiple disk summary states".into());
    }
    let raw = crate::project_decode::binding_value(&rows[0], "state")
        .ok_or("disk summary has no state value")?;
    if raw.len() > 8192 {
        return Err("oversized disk summary state".into());
    }
    let summary: Summary = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    summary.validate()?;
    Ok(Some((raw, summary)))
}

/// The lifetime maximum conservatively bounds every old per-signature p90,
/// even after several repositories are pooled into a command class.
pub(crate) fn fetch_bound(
    endpoint: &str,
    signature: &str,
    filesystem: &str,
) -> Result<Option<i64>, String> {
    read(
        endpoint,
        &subject(filesystem, command_class(signature)),
        Instant::now(),
    )
    .map(|row| row.map(|(_, state)| state.max_delta_bytes))
}

fn literal(text: &str) -> String {
    // JSON string escaping is also valid for a SPARQL double-quoted string.
    serde_json::to_string(text).expect("serializing a string cannot fail")
}

fn fields(iri: &str, raw: &str, value: &Summary) -> String {
    format!("<{iri}> a:diskImpactState {} ; a:runCount {} ; a:maxDiskDeltaBytes {} ; a:p90DiskDeltaBytes {} ; a:observedAt {}^^xsd:dateTime .",
        literal(raw), value.run_count, value.max_delta_bytes,
        value.p90_upper_bytes(), literal(&value.last_seen))
}

fn update(
    iri: &str,
    class: &str,
    filesystem: &str,
    old: Option<&(String, Summary)>,
    next: &Summary,
) -> Result<String, String> {
    let raw = serde_json::to_string(next).map_err(|e| e.to_string())?;
    if raw.len() > 8192 {
        return Err("oversized disk summary state".into());
    }
    let insert = format!("<{iri}> a a:CommandDiskImpactSummary ; a:commandClass {} ; a:filesystemIdentity {} ; rdfs:label {} . {}",
        literal(class), literal(filesystem), literal(&format!("disk summary {class}")), fields(iri, &raw, next));
    let prefix = format!("PREFIX a: <{}> PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#> PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>", crate::export::ONTO);
    Ok(match old {
        Some((raw, previous)) => format!("{prefix} DELETE {{ {} }} INSERT {{ {insert} }} WHERE {{ <{iri}> a:diskImpactState {} }}",
            fields(iri, raw, previous), literal(raw)),
        None => format!("{prefix} INSERT {{ {insert} }} WHERE {{ FILTER NOT EXISTS {{ <{iri}> ?p ?o }} }}"),
    })
}

fn valid_filesystem(value: &str) -> bool {
    value.split_once(':').is_some_and(|(kind, hash)| {
        matches!(kind, "root" | "mount" | "tmpfs")
            && hash.len() == 24
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// One byte-bounded private ring, shared across sessions on this host. A busy
/// or stale exclusive lock is UNKNOWN, never permission to drop the lock.
fn retain_raw(directory: &Path, record: &serde_json::Value) -> Result<(), String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let lock = directory.join("samples.lock");
    let _file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map_err(|e| format!("raw disk sample custody unavailable: {e}"))?;
    struct Lock<'a>(&'a Path);
    impl Drop for Lock<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }
    let _lock = Lock(&lock);
    let path = directory.join("samples.jsonl");
    let mut bytes = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_file() && meta.len() <= RAW_BYTES as u64 => {
            std::fs::read(&path).map_err(|e| e.to_string())?
        }
        Ok(_) => return Err("raw disk ring is not a bounded regular file".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    let mut line = serde_json::to_vec(record).map_err(|e| e.to_string())?;
    line.push(b'\n');
    if line.len() > 1024 {
        return Err("oversized raw disk sample".into());
    }
    while bytes.len() + line.len() > RAW_BYTES {
        let end = bytes
            .iter()
            .position(|b| *b == b'\n')
            .ok_or("invalid raw disk ring")?;
        bytes.drain(..=end);
    }
    bytes.extend(line);
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    temporary.write_all(&bytes).map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    temporary.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn record(
    endpoint: &str,
    signature: &str,
    filesystem: &str,
    delta: i64,
    observed_at: &str,
    session: Option<&str>,
    directory: &Path,
) -> Result<(), String> {
    if !valid_filesystem(filesystem) {
        return Err("invalid opaque filesystem identity".into());
    }
    let first = Summary::first(delta, observed_at)?;
    let class = command_class(signature);
    let iri = subject(filesystem, class);
    retain_raw(
        directory,
        &serde_json::json!({"sample_id": hex::encode(Sha256::digest(
        format!("{signature}\0{filesystem}\0{observed_at}"))), "class": class, "filesystem": filesystem,
        "delta_bytes": delta, "observed_at": observed_at}),
    )?;
    let start = Instant::now();
    let token = crate::quipu_auth::require_for_hook(endpoint, session)?;
    for _ in 0..3 {
        let old = read(endpoint, &iri, start)?;
        let next = match &old {
            Some((_, value)) => value.observe(delta, observed_at)?,
            None => first.clone(),
        };
        let body = update(&iri, class, filesystem, old.as_ref(), &next)?;
        let url = format!("{}/update", endpoint.trim_end_matches('/'));
        let request = crate::quipu_provenance::with_hook_session(
            crate::quipu_label::json_post(&url, crate::quipu_label::HOOK),
            session,
        )
        .query("actor", "yupana")
        .query("source", "command-disk-impact-summary")
        .set("Content-Type", "application/sparql-update")
        .set("Authorization", &format!("Bearer {token}"))
        .timeout(remaining(start)?);
        // Retry ONLY a known zero-change conditional miss. Transport errors or
        // ambiguous write receipts are never retried; the raw ring is retained.
        let response = request.send_string(&body).map_err(|error| match error {
            ureq::Error::Status(401, _) => crate::quipu_auth::rejected_for_hook(endpoint, session),
            _ => error.to_string(),
        })?;
        let mut text = String::new();
        response
            .into_reader()
            .take(8193)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        if text.len() > 8192 {
            return Err("oversized disk summary receipt; no retry".into());
        }
        let receipt: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let asserted = receipt
            .get("asserted")
            .and_then(serde_json::Value::as_u64)
            .ok_or("unknown disk summary write receipt")?;
        let retracted = receipt
            .get("retracted")
            .and_then(serde_json::Value::as_u64)
            .ok_or("unknown disk summary write receipt")?;
        if asserted > 0
            && receipt
                .get("tx")
                .and_then(serde_json::Value::as_u64)
                .is_some_and(|tx| tx > 0)
        {
            return Ok(());
        }
        if asserted != 0 || retracted != 0 || receipt.get("tx") != Some(&serde_json::Value::Null) {
            return Err("ambiguous disk summary write; no retry".into());
        }
    }
    Err("disk summary contention: three known conditional misses; raw sample retained".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updates_are_subject_bound_and_compare_the_exact_old_state() {
        let first = Summary::first(-3, "2026-01-01T00:00:00Z").unwrap();
        let next = first.observe(99, "2026-01-02T00:00:00Z").unwrap();
        let old = (serde_json::to_string(&first).unwrap(), first);
        let query = update("urn:summary", "cargo:build", "root:a", Some(&old), &next).unwrap();
        assert!(query.contains("WHERE { <urn:summary> a:diskImpactState "));
        assert!(query.contains("DELETE { <urn:summary>"));
        assert!(query.contains("a:runCount 2"));
        assert!(!query.contains("CommandDiskImpactObservation"));
        assert!(update("urn:summary", "cargo:build", "root:a", None, &next)
            .unwrap()
            .contains("FILTER NOT EXISTS { <urn:summary> ?p ?o }"));
    }
    #[test]
    fn raw_ring_is_byte_bounded_and_never_steals_existing_custody() {
        let dir = tempfile::tempdir().unwrap();
        let record =
            serde_json::json!({"delta": 1, "class": "cargo:build", "padding": "x".repeat(600)});
        for _ in 0..200 {
            retain_raw(dir.path(), &record).unwrap();
        }
        let path = dir.path().join("samples.jsonl");
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() <= RAW_BYTES);
        for line in bytes.split(|b| *b == b'\n').filter(|s| !s.is_empty()) {
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(line).unwrap(),
                record
            );
        }
        std::fs::write(dir.path().join("samples.lock"), b"owner").unwrap();
        assert!(retain_raw(dir.path(), &record).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        assert_eq!(
            std::fs::read(dir.path().join("samples.lock")).unwrap(),
            b"owner"
        );
    }
}
