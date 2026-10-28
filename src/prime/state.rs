//! Bounded local configuration and cache inspection; never initialize state.
use crate::config::YupanaConfig;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const LIMIT: u64 = 1024 * 1024;
const DEADLINE: Duration = Duration::from_millis(650);

pub(super) fn summarize(path: &Path, config: Option<&Path>) -> Value {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let config = config.map(|p| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        }
    });
    let fallback = root.clone();
    bounded(&fallback, move || read(&root, config.as_deref()))
}

fn bounded<F>(fallback: &Path, read: F) -> Value
where
    F: FnOnce() -> Value + Send + 'static,
{
    let started = Instant::now();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(read());
    });
    let mut out = match rx.recv_timeout(DEADLINE) {
        Ok(out) => out,
        Err(mpsc::RecvTimeoutError::Timeout) => json!({"status":"partial",
            "summary":{"root":fallback,"reason":"deadline","next":"yupana prime --static"}}),
        Err(mpsc::RecvTimeoutError::Disconnected) => json!({"status":"unreadable",
            "summary":{"root":fallback,"reason":"reader_failed","next":"yupana prime --static"}}),
    };
    out["elapsed_ms"] = json!(started.elapsed().as_millis());
    out
}

pub(super) fn read_bounded(path: &Path) -> Result<Vec<u8>, &'static str> {
    if !std::fs::metadata(path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "absent"
            } else {
                "unreadable"
            }
        })?
        .is_file()
    {
        return Err("unreadable");
    }
    let file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            "absent"
        } else {
            "unreadable"
        }
    })?;
    if !file.metadata().map_err(|_| "unreadable")?.is_file() {
        return Err("unreadable");
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "unreadable")?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > LIMIT {
        return Err("unreadable");
    }
    Ok(bytes)
}

fn read(root: &Path, override_path: Option<&Path>) -> Value {
    let paths = if let Some(path) = override_path {
        vec![path.to_path_buf()]
    } else {
        let mut paths = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".config/bobbin/config.toml"));
        }
        paths.push(root.join(".bobbin/config.toml"));
        paths
    };
    let mut found = false;
    for path in &paths {
        match read_bounded(path) {
            Ok(_) => found = true,
            Err("absent") => {}
            Err(_) => {
                return json!({"status":"unreadable","summary":{"root":root,"path":path,"reason":"configuration","next":"yupana prime --static"}})
            }
        }
    }
    if override_path.is_some() && !found {
        return json!({"status":"absent","summary":{"root":root,"path":override_path,"reason":"configuration_absent","next":"yupana prime --static"}});
    }
    let Ok(config) = YupanaConfig::resolve(override_path, root) else {
        return json!({"status":"unreadable","summary":{"root":root,"configs":paths,"reason":"configuration","next":"yupana prime --static"}});
    };
    let mut summary = json!({"root":root,"configs":paths,"configuration_source":if found {"files"} else {"compiled_defaults"},
        "base_ref":config.base_ref,"base_ref_source":"configuration","tiers":crate::types::Tier::served(),
        "local_rules":config.policy.rules.len()});
    let mut status = if found { "ok" } else { "absent" };
    #[cfg(feature = "quipu")]
    {
        let cache_status = super::cache::inspect(&config, &mut summary);
        if cache_status != "ok" {
            status = cache_status;
        }
    }
    #[cfg(not(feature = "quipu"))]
    {
        summary["governed_rules"] = json!({"status":"feature_unavailable","count":null});
        if config.quipu.enabled {
            status = "partial";
            summary["reason"] = json!("quipu_feature_unavailable");
        }
    }
    if status != "ok" && summary["next"].is_null() {
        summary["next"] = json!("yupana prime --static");
    }
    json!({"status":status,"summary":summary})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_local_reader_reports_partial_with_elapsed_under_one_second() {
        let (release, wait) = mpsc::channel();
        let (done, finish) = mpsc::channel();
        let started = Instant::now();
        let state = bounded(Path::new("private-fixture"), move || {
            let _ = wait.recv_timeout(Duration::from_secs(2));
            let _ = done.send(());
            json!({"status":"ok","summary":{}})
        });
        assert_eq!(state["status"], "partial");
        assert_eq!(state["summary"]["reason"], "deadline");
        assert!(state["elapsed_ms"].as_u64().unwrap() < 1000);
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = release.send(());
        finish.recv_timeout(Duration::from_secs(1)).unwrap();
    }
}
