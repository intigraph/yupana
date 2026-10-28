//! Counts from a bounded local cache, with explicit historical provenance.
use crate::config::YupanaConfig;
use crate::projection_cache::{cache_path, now_secs, CachedProjection, CACHE_VERSION};
use serde_json::{json, Value};

pub(super) fn inspect(config: &YupanaConfig, summary: &mut Value) -> &'static str {
    summary["governed_rules"] = json!({"status":"disabled","count":null});
    if !config.quipu.enabled {
        return "ok";
    }
    if config.quipu.endpoint.is_empty() {
        summary["governed_rules"]["status"] = json!("endpoint_unconfigured");
        summary["reason"] = json!("projection_endpoint_unconfigured");
        return "unreadable";
    }
    let Some(path) = cache_path() else {
        summary["governed_rules"]["status"] = json!("cache_path_unknown");
        return "absent";
    };
    summary["cache_path"] = json!(path);
    let bytes = match super::state::read_bounded(&path) {
        Ok(bytes) => bytes,
        Err(status) => {
            summary["governed_rules"]["status"] = json!(status);
            return status;
        }
    };
    let Ok(cached) = serde_json::from_slice::<CachedProjection>(&bytes) else {
        summary["governed_rules"]["status"] = json!("cache_unreadable");
        return "unreadable";
    };
    let now = now_secs();
    if !(1..=CACHE_VERSION).contains(&cached.version)
        || cached.endpoint.trim_end_matches('/') != config.quipu.endpoint.trim_end_matches('/')
        || cached.written_at > now
    {
        summary["governed_rules"]["status"] = json!("cache_identity_unreadable");
        return "unreadable";
    }
    let age = now - cached.written_at;
    let expired = age > config.quipu.projection_cache_ttl_secs;
    summary["governed_rules"] = json!({"status":if expired {"expired"} else {"cached"},
        "count":cached.policies.len()+cached.text_rules.len(),"structural":cached.policies.len(),
        "text":cached.text_rules.len(),"age_secs":age,"source":"local_projection_cache",
        "count_scope":"cached_structural_and_text","enforcement":"not_measured"});
    if expired {
        summary["next"] = json!("yupana refresh-projection");
        "partial"
    } else {
        "ok"
    }
}
