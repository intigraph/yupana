//! Dedicated fail-closed hook protocol, outside the fail-open singular hook command.
use crate::outbound::{self, Decision, OutboundPolicy, Proofs, Selection};
use clap::Args;
use std::io::Read;
use std::path::{Path, PathBuf};

const LIMIT: u64 = 1024 * 1024;

/// Protected deployment paths. These are invocation configuration, never tool input.
#[derive(Debug, Args)]
pub struct GuardArgs {
    /// Operator-projected policy with pinned protected producer keys.
    #[arg(long)]
    policy: PathBuf,
    /// Protected producer's detached receipt store, keyed by action SHA-256.
    #[arg(long)]
    receipts: PathBuf,
}

fn bounded(path: &Path) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(LIMIT + 1)
        .read_to_end(&mut body)
        .ok()?;
    (body.len() as u64 <= LIMIT).then_some(body)
}

impl GuardArgs {
    /// Return only fixed metadata; malformed input/config never prints its contents.
    pub fn run(&self) -> anyhow::Result<i32> {
        let mut raw = Vec::new();
        let decision = if std::io::stdin()
            .take(LIMIT + 1)
            .read_to_end(&mut raw)
            .is_err()
            || raw.len() as u64 > LIMIT
        {
            Decision::deny("unreadable-input")
        } else {
            self.evaluate(&raw)
        };
        println!("{}", serde_json::to_string(&decision)?);
        Ok(0)
    }

    fn evaluate(&self, raw: &[u8]) -> Decision {
        let Some(policy) =
            bounded(&self.policy).and_then(|b| serde_json::from_slice::<OutboundPolicy>(&b).ok())
        else {
            return Decision::deny("unreadable-policy");
        };
        let Ok(payload) = serde_json::from_slice(raw) else {
            return Decision::deny("unreadable-input");
        };
        let proofs = match outbound::select(&policy, &payload) {
            Selection::Send(binding) => bounded(
                &self
                    .receipts
                    .join(format!("{}.json", binding.action_sha256)),
            )
            .and_then(|body| serde_json::from_slice::<Proofs>(&body).ok()),
            _ => None,
        };
        let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
            return Decision::deny("clock-unknown");
        };
        outbound::decide(&policy, &payload, proofs.as_ref(), now.as_secs())
    }
}
