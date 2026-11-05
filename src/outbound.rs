//! Exact-input outbound approval. Unknown actions never become permission.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// Operator-reviewed mapping for one connector's actual send schema.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SendAdapter {
    /// Exact harness tool name, not a suffix or substring match.
    pub tool_name: String,
    /// JSON pointer to the exact UTF-8 message text.
    pub text_pointer: String,
    /// JSON pointer to the recipient string or array of strings.
    pub recipient_pointer: String,
    /// JSON pointer to the thread; absence is bound explicitly for new messages.
    pub thread_pointer: String,
    /// Additional top-level literal string fields, such as subject. No references.
    pub literal_fields: Vec<String>,
    /// Replies and forwards require an independently observed thread.
    pub thread_required: bool,
}

/// Trusted policy: never read issuer keys or adapter mappings from a tool call.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundPolicy {
    /// SHA-256 of the reviewed producer/adapter contract.
    pub revision: String,
    /// Protected direct-human-chat approval producer's Ed25519 public key.
    pub approval_public_key: String,
    /// Protected domain content-gate producer's Ed25519 public key.
    pub gate_public_key: String,
    /// Both proofs must be no older than this, capped at one hour.
    pub max_age_secs: u64,
    /// Reviewed non-send tools, including draft creation. No substring matching.
    pub non_send_tools: Vec<String>,
    /// Reviewed send schemas. Every other action is unknown and refused.
    pub send_adapters: Vec<SendAdapter>,
}

/// Privacy-preserving identity computed from the actual attempted operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Binding {
    /// Exact text bytes; no trimming, newline or Unicode normalization.
    pub text_sha256: String,
    /// Exact recipient and thread, including an explicitly absent thread.
    pub destination_sha256: String,
    /// Entire tool name/input; this does not resolve externally referenced bytes.
    pub action_sha256: String,
}

/// Classifier output. Unknown must not be treated as outside policy scope.
#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    /// A reviewed action whose connector contract does not send.
    NonSend,
    /// A send whose actual inputs can be independently bound.
    Send(Binding),
    /// Invalid policy, schema or unclassified action.
    Unknown,
}

/// Detached signed records, transported separately from the tool input.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proofs {
    /// Signed direct-user approval, not an agent's summary of approval.
    pub approval: Value,
    /// Signed domain gate verdict for precisely the same operation.
    pub content_gate: Value,
}

/// Only static reason codes reach the hook response; input is never echoed.
#[derive(Debug, Serialize)]
pub struct Decision {
    /// Versioned adapter protocol, distinct from a generic JSON success.
    pub schema: &'static str,
    /// Either allow or deny.
    pub decision: &'static str,
    /// A fixed code, never message text, recipient or a parser diagnostic.
    pub reason: &'static str,
}

impl Decision {
    /// Refuse before dispatch.
    #[must_use]
    pub fn deny(reason: &'static str) -> Self {
        Self {
            schema: "yupana-outbound/v1",
            decision: "deny",
            reason,
        }
    }

    fn allow(reason: &'static str) -> Self {
        Self {
            schema: "yupana-outbound/v1",
            decision: "allow",
            reason,
        }
    }
}

pub(crate) fn digest(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

pub(crate) fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn canonical(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort_unstable();
            let mut sorted = serde_json::Map::new();
            for key in keys {
                sorted.insert(key.clone(), canonical(&object[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

fn json_digest(value: &Value) -> String {
    // JSON values always serialize; an impossible failure is a denial upstream.
    digest(canonical(value).to_string().as_bytes())
}

fn destination(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.trim().is_empty(),
        Value::Array(values) => {
            !values.is_empty()
                && values
                    .iter()
                    .all(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()))
        }
        _ => false,
    }
}

impl OutboundPolicy {
    /// Reject ambiguous mappings and contradictory send/non-send declarations.
    #[must_use]
    pub fn valid(&self) -> bool {
        let mut names = BTreeSet::new();
        hex_digest(&self.revision)
            && hex_digest(&self.approval_public_key)
            && hex_digest(&self.gate_public_key)
            && self.approval_public_key != self.gate_public_key
            && (1..=3600).contains(&self.max_age_secs)
            && self
                .non_send_tools
                .iter()
                .all(|n| !n.is_empty() && names.insert(n.as_str()))
            && self.send_adapters.iter().all(|a| {
                let pointers = [&a.text_pointer, &a.recipient_pointer, &a.thread_pointer];
                !a.tool_name.is_empty()
                    && names.insert(&a.tool_name)
                    && pointers
                        .iter()
                        .all(|p| p.starts_with('/') && p.len() > 1 && !p[1..].contains(['/', '~']))
                    && pointers.into_iter().collect::<BTreeSet<_>>().len() == 3
                    && a.literal_fields.iter().all(|field| {
                        matches!(field.as_str(), "subject" | "cc" | "bcc")
                            && !pointers.iter().any(|p| &p[1..] == field)
                    })
                    && a.literal_fields.iter().collect::<BTreeSet<_>>().len()
                        == a.literal_fields.len()
            })
    }
}

/// Resolve actual connector fields. Raw browser coordinates cannot supply text.
#[must_use]
pub fn select(policy: &OutboundPolicy, payload: &Value) -> Selection {
    if !policy.valid() {
        return Selection::Unknown;
    }
    let Some(name) = payload.get("tool_name").and_then(Value::as_str) else {
        return Selection::Unknown;
    };
    let Some(input) = payload.get("tool_input").filter(|v| v.is_object()) else {
        return Selection::Unknown;
    };
    if policy.non_send_tools.iter().any(|n| n == name) {
        return Selection::NonSend;
    }
    let Some(adapter) = policy.send_adapters.iter().find(|a| a.tool_name == name) else {
        return Selection::Unknown;
    };
    // Closed-world literal schemas only. External content cannot be frozen by
    // hashing its path or draft ID; never admit such fields, even with receipts.
    let pointers = [
        &adapter.text_pointer,
        &adapter.recipient_pointer,
        &adapter.thread_pointer,
    ];
    if !input.as_object().is_some_and(|object| {
        object.iter().all(|(key, value)| {
            if pointers.iter().any(|p| &p[1..] == key) {
                true
            } else if adapter.literal_fields.contains(key) {
                if matches!(key.as_str(), "cc" | "bcc") {
                    destination(value)
                } else {
                    value.is_string()
                }
            } else {
                false
            }
        })
    }) {
        return Selection::Unknown;
    }
    let Some(text) = input
        .pointer(&adapter.text_pointer)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    else {
        return Selection::Unknown;
    };
    let Some(recipient) = input
        .pointer(&adapter.recipient_pointer)
        .filter(|v| destination(v))
    else {
        return Selection::Unknown;
    };
    let thread = input
        .pointer(&adapter.thread_pointer)
        .unwrap_or(&Value::Null);
    if !thread.is_null() && !thread.as_str().is_some_and(|s| !s.trim().is_empty())
        || adapter.thread_required && thread.is_null()
    {
        return Selection::Unknown;
    }
    Selection::Send(Binding {
        text_sha256: digest(text.as_bytes()),
        destination_sha256: json_digest(
            &serde_json::json!({"recipient":recipient,"thread":thread}),
        ),
        action_sha256: json_digest(&serde_json::json!({"tool_name":name,"tool_input":input})),
    })
}

/// Check both protected issuer roles against independently computed inputs.
#[must_use]
pub fn decide(
    policy: &OutboundPolicy,
    payload: &Value,
    proofs: Option<&Proofs>,
    now: u64,
) -> Decision {
    let binding = match select(policy, payload) {
        Selection::NonSend => return Decision::allow("non-send"),
        Selection::Unknown => return Decision::deny("unclassified-action"),
        Selection::Send(binding) => binding,
    };
    let Some(proofs) = proofs else {
        return Decision::deny("missing-receipts");
    };
    #[cfg(feature = "quipu")]
    {
        if !proof::valid(policy, &binding, &proofs.approval, true, now) {
            return Decision::deny("approval-unsatisfied");
        }
        if !proof::valid(policy, &binding, &proofs.content_gate, false, now) {
            return Decision::deny("content-gate-unsatisfied");
        }
        Decision::allow("approved")
    }
    #[cfg(not(feature = "quipu"))]
    {
        let _ = (binding, proofs, now);
        Decision::deny("verification-unavailable")
    }
}

#[cfg(feature = "quipu")]
#[path = "outbound_proof.rs"]
mod proof;

#[cfg(test)]
#[path = "outbound_test.rs"]
mod tests;
