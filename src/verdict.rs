//! ed25519 verdict signing + promotion (H-PROMOTE-VERDICT, `quipu` feature).
//!
//! A yupana edit-time verdict promotes into quipu as a signed `aegis:Verdict`.
//! Key custody, the canonical `v1|...` message, the evidence hash and the hex
//! encodings come from the shared `quipu-signing` crate (aegis-s24.3), the same
//! code quipu itself signs and verifies with. So a yupana-signed verdict
//! verifies under quipu's root of trust once a human registers yupana's public
//! key (`aegis:publicKey` on its `aegis:VerifierRegistration`). There is no local
//! copy of the scheme to drift.
//!
//! v1 custody is a host file (like quipu's), not an HSM. The verdict Turtle is
//! `VerdictShape`-conformant (every required field, including the signature), so
//! `POST /knot` accepts it and `quipu_verdict_verify` can check it.

use std::path::Path;

use ring::signature::Ed25519KeyPair;

use crate::errors::{Error, Result};

/// The verifier identity yupana attests as. Registered (human-authored) in quipu's
/// tree-sitter policy catalog.
pub const VERIFIER: &str = "yupana";
/// The tier of a structural verdict — always tree-sitter for these rules.
pub const TIER: &str = "tree-sitter";

/// Load the ed25519 signing keypair (PKCS#8) at `path`, generating and persisting
/// a fresh one (0600) if absent: `quipu_signing::load_or_generate`.
pub fn load_or_generate(path: &Path) -> Result<Ed25519KeyPair> {
    quipu_signing::load_or_generate(path).map_err(|e| Error::Promote(e.to_string()))
}

/// Hex-encoded public key — what a human pastes into yupana's
/// `aegis:VerifierRegistration aegis:publicKey` in quipu.
#[must_use]
pub fn public_key_hex(keypair: &Ed25519KeyPair) -> String {
    quipu_signing::public_key_hex(keypair)
}

/// The canonical byte string signed for a verdict: `quipu_signing::verdict_message`
/// with yupana's tier and verifier.
#[must_use]
pub fn verdict_message(
    predicate_id: &str,
    target_ref: &str,
    outcome: &str,
    evidence_hash: &str,
) -> Vec<u8> {
    quipu_signing::verdict_message(
        predicate_id,
        target_ref,
        outcome,
        evidence_hash,
        TIER,
        VERIFIER,
    )
}

/// The evidence hash bound into the verdict and its signature: `sha256:<hex>` of
/// the evidence the verdict was computed against (the introduced text).
#[must_use]
pub fn evidence_hash(evidence: &str) -> String {
    quipu_signing::evidence_hash(evidence)
}

/// A signed, `VerdictShape`-conformant `aegis:Verdict` in Turtle, ready for
/// `POST /knot`.
///
/// `satisfied` is the outcome: `true` when the policy holds, `false` when the
/// edit violated it. The signature is over [`verdict_message`], so quipu can
/// re-derive and check it against yupana's registered public key.
#[must_use]
pub fn verdict_turtle(
    keypair: &Ed25519KeyPair,
    policy_name: &str,
    target_ref: &str,
    satisfied: bool,
    evidence: &str,
    freshness: crate::types::Freshness,
) -> String {
    let outcome = if satisfied {
        "satisfied"
    } else {
        "unsatisfied"
    };
    let hash = evidence_hash(evidence);
    let signature = quipu_signing::sign_hex(
        keypair,
        &verdict_message(policy_name, target_ref, outcome, &hash),
    );
    // A stable IRI-safe id from the signature, so re-promoting the same verdict is
    // idempotent by content.
    let id = &signature[..signature.len().min(32)];
    let verification = format!(
        "aegis:verdict_{id} a aegis:Verdict, aegis:Verification ;\n\
         \x20   rdfs:label \"{label}\" ;\n\
         \x20   aegis:sourceKind \"observed\" ;\n\
         \x20   aegis:falsifier \"predicate `{predicate}` evaluates satisfied for target `{target}` against evidence {hash}\" ;\n\
         \x20   aegis:predicateId \"{predicate}\" ;\n\
         \x20   aegis:targetRef \"{target}\" ;\n\
         \x20   aegis:outcome \"{outcome}\" ;\n\
         \x20   aegis:evidenceHash \"{hash}\" ;\n\
         \x20   aegis:verifier \"{verifier}\" ;\n\
         \x20   aegis:signature \"{signature}\" ;\n\
         \x20   aegis:tier \"{tier}\" ;\n\
         \x20   aegis:freshness \"{freshness}\" .\n",
        label = escape(&format!("{policy_name} @ {target_ref}")),
        predicate = escape(policy_name),
        target = escape(target_ref),
        hash = escape(&hash),
        verifier = VERIFIER,
        tier = TIER,
        freshness = freshness_label(freshness),
    );
    let blocker = if satisfied {
        String::new()
    } else {
        format!(
            "aegis:blocker_{id} a aegis:Blocker ;\n\
             \x20   rdfs:label \"{label} is unsatisfied\" ;\n\
             \x20   aegis:sourceKind \"observed\" ;\n\
             \x20   aegis:blockerEvidence \"built\" ;\n\
             \x20   aegis:demonstratedBy aegis:verdict_{id} .\n",
            label = escape(&format!("{policy_name} @ {target_ref}")),
        )
    };
    format!(
        "@prefix aegis: <http://aegis.gastown.local/ontology/> .\n\
         @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
         {verification}{blocker}\n",
    )
}

/// The `aegis:freshness` lexical value for a yupana freshness.
///
/// The shape admits only `fresh` and `stale`. `Recomputing` is a yupana-internal
/// state with no governed counterpart, and it maps to STALE rather than fresh:
/// a verdict computed while the registry was mid-refresh was computed against
/// something that may already have moved, and the conservative reading is the
/// only one that cannot overstate.
///
/// This function exists because the field was a hardcoded `"fresh"` — every
/// verdict yupana could have promoted would have claimed currency it had not
/// checked, which is the precise thing the tier/freshness discipline exists to
/// stop.
#[must_use]
fn freshness_label(freshness: crate::types::Freshness) -> &'static str {
    match freshness {
        crate::types::Freshness::Fresh => "fresh",
        crate::types::Freshness::Stale | crate::types::Freshness::Recomputing => "stale",
    }
}

/// Sign and promote a verdict to quipu via `/knot`. Returns the quipu response.
pub fn promote_verdict(
    endpoint: &str,
    keypair: &Ed25519KeyPair,
    policy_name: &str,
    target_ref: &str,
    satisfied: bool,
    evidence: &str,
    freshness: crate::types::Freshness,
) -> Result<crate::promote::KnotResult> {
    let turtle = verdict_turtle(
        keypair,
        policy_name,
        target_ref,
        satisfied,
        evidence,
        freshness,
    );
    crate::promote::write_knot(
        endpoint,
        &turtle,
        &format!("yupana verdict: {policy_name} on {target_ref}"),
    )
}

/// Escape a value for a Turtle double-quoted string literal.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{UnparsedPublicKey, ED25519};

    fn keypair() -> Ed25519KeyPair {
        let dir = tempfile::tempdir().unwrap();
        load_or_generate(&dir.path().join("k.pk8")).unwrap()
    }

    #[test]
    fn a_signed_verdict_verifies_under_the_shared_scheme() {
        // The interop guarantee: yupana signs with the exact message + encoding quipu
        // verifies with (quipu::signing::verify_hex re-derives this same message).
        let kp = keypair();
        let outcome = "unsatisfied";
        let hash = evidence_hash("// see ABC-123");
        let message = verdict_message("no-ticket-in-comment", "src/a.rs", outcome, &hash);
        let signature = hex::encode(kp.sign(&message).as_ref());

        let pk = hex::decode(public_key_hex(&kp)).unwrap();
        let sig = hex::decode(&signature).unwrap();
        assert!(
            UnparsedPublicKey::new(&ED25519, pk)
                .verify(&message, &sig)
                .is_ok(),
            "a yupana-signed verdict must verify under ed25519 with the registered key"
        );
    }

    #[test]
    fn yupana_and_quipu_sign_one_envelope() {
        // aegis-s24.3: the message is quipu-signing's own, byte for byte, and a
        // yupana verdict signature verifies through quipu-signing's verify_hex.
        let kp = keypair();
        let hash = evidence_hash("// see ABC-123");
        let ours = verdict_message("no-ticket-in-comment", "src/a.rs", "unsatisfied", &hash);
        assert_eq!(
            ours,
            b"v1|no-ticket-in-comment|src/a.rs|unsatisfied|sha256:"
                .iter()
                .copied()
                .chain(hash["sha256:".len()..].bytes())
                .chain(b"|tree-sitter|yupana".iter().copied())
                .collect::<Vec<u8>>()
        );
        let ttl = verdict_turtle(
            &kp,
            "no-ticket-in-comment",
            "src/a.rs",
            false,
            "// see ABC-123",
            crate::types::Freshness::Fresh,
        );
        let signature = ttl
            .split("aegis:signature \"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(quipu_signing::verify_hex(
            &public_key_hex(&kp),
            &ours,
            signature
        ));
        let mut tampered = ours.clone();
        tampered[3] ^= 1;
        assert!(!quipu_signing::verify_hex(
            &public_key_hex(&kp),
            &tampered,
            signature
        ));
    }

    #[test]
    fn verdict_turtle_is_shape_conformant_and_signed() {
        let kp = keypair();
        let ttl = verdict_turtle(
            &kp,
            "no-ticket-in-comment",
            "src/a.rs",
            false,
            "// ABC-123",
            crate::types::Freshness::Fresh,
        );
        // Every VerdictShape-required field is present.
        for field in [
            "a aegis:Verdict, aegis:Verification",
            "aegis:sourceKind \"observed\"",
            "aegis:falsifier \"predicate `no-ticket-in-comment` evaluates satisfied",
            "aegis:predicateId \"no-ticket-in-comment\"",
            "aegis:targetRef \"src/a.rs\"",
            "aegis:outcome \"unsatisfied\"",
            "aegis:evidenceHash \"sha256:",
            "aegis:verifier \"yupana\"",
            "aegis:signature \"",
            "aegis:tier \"tree-sitter\"",
            "aegis:blockerEvidence \"built\"",
            "aegis:demonstratedBy aegis:verdict_",
        ] {
            assert!(
                ttl.contains(field),
                "verdict Turtle missing `{field}`:\n{ttl}"
            );
        }
    }

    #[test]
    fn satisfied_verdict_is_a_verification_but_never_a_blocker() {
        let kp = keypair();
        let ttl = verdict_turtle(
            &kp,
            "no-ticket-in-comment",
            "src/a.rs",
            true,
            "// no ticket",
            crate::types::Freshness::Fresh,
        );
        assert!(ttl.contains("a aegis:Verdict, aegis:Verification"));
        assert!(
            ttl.contains("aegis:falsifier \"predicate `no-ticket-in-comment` evaluates satisfied")
        );
        assert!(
            !ttl.contains("a aegis:Blocker"),
            "a passing Yupana check is verification evidence, not a blocker: {ttl}"
        );
    }

    #[test]
    fn the_signature_binds_the_evidence_hash() {
        // Changing the evidence changes the hash, hence the signed message, hence
        // the signature — so a verdict cannot be replayed onto different evidence.
        let kp = keypair();
        let a = verdict_turtle(
            &kp,
            "p",
            "src/a.rs",
            false,
            "evidence one",
            crate::types::Freshness::Fresh,
        );
        let b = verdict_turtle(
            &kp,
            "p",
            "src/a.rs",
            false,
            "evidence two",
            crate::types::Freshness::Fresh,
        );
        let sig = |ttl: &str| {
            ttl.split("aegis:signature \"")
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
                .to_string()
        };
        assert_ne!(sig(&a), sig(&b));
    }

    #[test]
    fn generate_is_idempotent_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k.pk8");
        let a = public_key_hex(&load_or_generate(&path).unwrap());
        // Second load reads the SAME persisted key, not a fresh one.
        let b = public_key_hex(&load_or_generate(&path).unwrap());
        assert_eq!(a, b);
    }
}
