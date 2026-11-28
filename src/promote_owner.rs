//! Explicit owner transport. No ambient credentials, write retries or legacy fallback.
use crate::{
    errors::{Error, Result},
    promote::KnotResult,
};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

fn refused(message: &str) -> Error {
    Error::Promote(format!("owner promotion: {message}"))
}

/// Enrollment-produced pins, consumed whole; callers never synthesize defaults.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerPins {
    /// Pin format version.
    pub version: u32,
    /// Enrolled server origin.
    pub endpoint: String,
    /// Exact enrolled graph IRI.
    pub graph: String,
    /// Published catalog revision.
    pub catalog_revision: u64,
    /// Numeric route generation, distinct from the instance digest.
    pub owner_generation: u64,
    /// Installed owner policy.
    pub policy: String,
}

/// One explicit target. Deliberately has no Debug implementation: it holds a bearer.
pub struct OwnerTarget {
    pins: OwnerPins,
    bearer: String,
    custody: PathBuf,
    _lock: CustodyLock,
}
struct CustodyLock(PathBuf);
impl Drop for CustodyLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl OwnerTarget {
    /// Read the named credential only from its file, with no ambient fallback.
    pub fn load(
        endpoint: &str,
        pins_file: &Path,
        credential_file: &Path,
        custody: &Path,
    ) -> Result<Self> {
        let pins: OwnerPins = serde_json::from_reader(
            File::open(pins_file).map_err(|_| refused("pin file unreadable"))?,
        )
        .map_err(|_| refused("pin file invalid"))?;
        crate::promote::validate_target_graph(Some(&pins.graph))?;
        if pins.version != 1
            || pins.policy.trim().is_empty()
            || pins.policy.contains(['\r', '\n'])
            || pins.endpoint.trim_end_matches('/') != endpoint.trim_end_matches('/')
        {
            return Err(refused("enrollment pins do not match target"));
        }
        let url = url::Url::parse(endpoint).map_err(|_| refused("invalid endpoint"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(refused("invalid endpoint authority"));
        }
        let token = std::fs::read_to_string(credential_file)
            .map_err(|_| refused("credential file unreadable"))?;
        let bearer = token.trim().to_owned();
        if bearer.is_empty() || bearer.contains(['\r', '\n']) {
            return Err(refused("credential file invalid"));
        }
        if !custody.is_dir() {
            return Err(refused("explicit custody directory must exist"));
        }
        let lock_path = custody.join(".owner-active");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|_| refused("custody is active or needs crash inspection"))?;
        let lock = CustodyLock(lock_path);
        for entry in std::fs::read_dir(custody).map_err(|_| refused("custody unreadable"))? {
            let entry = entry.map_err(|_| refused("custody unreadable"))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".json")
                && !name.ends_with(".finished.json")
                && !name.ends_with(".sealed.json")
            {
                return Err(refused("unresolved original attempt in custody; resolve its status before another promotion"));
            }
        }
        Ok(Self {
            _lock: lock,
            pins,
            bearer,
            custody: custody.to_owned(),
        })
    }
    /// Exact graph selected by the enrollment pins.
    pub fn graph(&self) -> &str {
        &self.pins.graph
    }

    fn post(&self, route: &str, id: &str, body: &Value) -> Result<Value> {
        let url = format!(
            "{}/owner/knot{route}",
            self.pins.endpoint.trim_end_matches('/')
        );
        // Disable redirects: an enrollment-bound bearer must never cross authorities.
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(std::time::Duration::from_secs(30))
            .build();
        let response =
            crate::quipu_provenance::apply(agent.post(&url), crate::quipu_label::PROMOTE)
                .set("content-type", "application/json")
                .set("authorization", &format!("Bearer {}", self.bearer))
                .set("x-quipu-client", crate::quipu_label::PROMOTE)
                .set("x-quipu-completion-contract", "quipu.knot-completion/v2")
                .set("x-quipu-write-id", id)
                .set(
                    "x-quipu-catalog-revision",
                    &self.pins.catalog_revision.to_string(),
                )
                .set(
                    "x-quipu-owner-generation",
                    &self.pins.owner_generation.to_string(),
                )
                .set("x-quipu-owner-policy", &self.pins.policy)
                .send_string(&body.to_string())
                .map_err(|_| {
                    refused(
                        "request refused or response UNKNOWN; retained attempt must be inspected",
                    )
                })?;
        serde_json::from_reader(response.into_reader().take(1024 * 1024))
            .map_err(|_| refused("response malformed; retained attempt must be inspected"))
    }

    /// Freeze the exact request before its sole send. The retained ID is never
    /// reused or retried automatically, even after an ambiguous transport error.
    pub fn write(
        &self,
        turtle: &str,
        source: &str,
        snapshot: Option<&str>,
        valid_from: Option<&str>,
    ) -> Result<KnotResult> {
        let mut body =
            json!({"turtle":turtle,"actor":"yupana","source":source,"graph":self.pins.graph});
        if let Some(snapshot) = snapshot {
            body["snapshot"] = json!(snapshot);
            body["replace_snapshot"] = json!(true);
        }
        if let Some(time) = valid_from {
            body["valid_from"] = json!(time);
        }
        let mut random = [0u8; 32];
        SystemRandom::new()
            .fill(&mut random)
            .map_err(|_| refused("write ID entropy unavailable"))?;
        let id = hex::encode(random);
        let binding = json!({"write_id":id,"request":body});
        let path = self.custody.join(format!("{id}.json"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| refused("could not create attempt custody"))?;
        // Enrollment pins and request, never the bearer. fsync before any send.
        let frozen = json!({"pins":self.pins,"binding":binding});
        file.write_all(frozen.to_string().as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|_| refused("could not persist attempt custody"))?;
        File::open(&self.custody)
            .and_then(|f| f.sync_all())
            .map_err(|_| refused("could not persist custody directory"))?;
        let sent = self.post("", &id, &body);
        let proof = self.post("/status", &id, &binding)?;
        if proof["contract"] != "quipu.knot-completion/v2" || proof["write_id"] != id {
            return Err(refused("status binding or contract mismatch"));
        }
        match proof["state"].as_str() {
            Some("finished") => {
                let receipt = &proof["receipt"];
                if receipt["conforms"] != true {
                    return Err(refused("finished attempt did not conform"));
                }
                let mut result: KnotResult = serde_json::from_value(receipt.clone())
                    .map_err(|_| refused("finished receipt invalid"))?;
                if result.tx_id.is_none_or(|id| id == 0 && result.count != 0) {
                    return Err(refused("finished receipt lacks transaction"));
                }
                if proof["commit"]["tx_id"] != receipt["tx_id"] {
                    return Err(refused("status commit witness and receipt disagree"));
                }
                if let Ok(sent) = sent {
                    if sent["tx_id"] != receipt["tx_id"] || sent["count"] != receipt["count"] {
                        return Err(refused("write and status receipts disagree"));
                    }
                    result.valid_from = sent
                        .get("valid_from")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                if valid_from.is_some() && result.valid_from.as_deref().is_none_or(str::is_empty) {
                    return Err(refused("commit confirmed but valid-time receipt unavailable; retained attempt needs inspection"));
                }
                std::fs::rename(&path, self.custody.join(format!("{id}.finished.json")))
                    .and_then(|()| File::open(&self.custody).and_then(|f| f.sync_all()))
                    .map_err(|_| refused("commit confirmed but custody finalization failed"))?;
                Ok(result)
            }
            Some("admitted") => {
                let sealed = self.post("/seal", &id, &binding)?;
                if sealed["state"] != "sealed"
                    || sealed["write_id"] != id
                    || sealed["contract"] != "quipu.knot-completion/v2"
                {
                    return Err(refused(
                        "seal unconfirmed; retained attempt needs inspection",
                    ));
                }
                std::fs::rename(&path, self.custody.join(format!("{id}.sealed.json")))
                    .and_then(|()| File::open(&self.custody).and_then(|f| f.sync_all()))
                    .map_err(|_| refused("seal confirmed but custody finalization failed"))?;
                Err(refused("admitted attempt sealed; promotion not completed"))
            }
            _ => Err(refused(
                "completion UNKNOWN; inspect retained attempt, never resend automatically",
            )),
        }
    }
}

#[cfg(test)]
#[path = "promote_owner_tests.rs"]
mod tests;
