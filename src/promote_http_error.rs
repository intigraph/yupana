//! Bounded diagnostics for terminal HTTP refusals, without request credentials.

use std::io::Read;

const BODY_LIMIT: usize = 4096;

pub(super) fn describe(code: u16, mut body: impl Read, auth: &str) -> String {
    let mut bytes = Vec::new();
    if body
        .by_ref()
        .take((BODY_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return format!("status {code}; response body unreadable");
    }
    // Do not print a partial credential or partial JSON document at the boundary.
    if bytes.len() > BODY_LIMIT {
        return format!("status {code}; response body exceeds {BODY_LIMIT} bytes");
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if !auth.is_empty() {
        text = text.replace(auth, "[REDACTED]");
        // A JSON error can reflect an escaped credential rather than its raw bytes.
        if let Ok(encoded) = serde_json::to_string(auth) {
            text = text.replace(&encoded[1..encoded.len() - 1], "[REDACTED]");
        }
    }
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = text.trim();
    if text.is_empty() {
        format!("status {code}; empty response body")
    } else {
        format!("status {code}; response: {text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_refusal_preserves_json_and_plain_text() {
        for message in [
            r#"{"error":"write guard refused: owner grant missing"}"#,
            "write guard refused",
        ] {
            let error = describe(400, message.as_bytes(), "fixture-token");
            assert!(error.contains(message));
            assert!(error.starts_with("status 400; response:"));
        }
    }

    #[test]
    fn echoed_raw_and_json_credentials_are_redacted() {
        let token = "fixture-\"token";
        for message in [
            format!("Bearer {token}"),
            serde_json::json!({"error": token}).to_string(),
        ] {
            let error = describe(403, message.as_bytes(), token);
            assert!(!error.contains(token));
            assert!(!error.contains("fixture-"));
            assert!(error.contains("[REDACTED]"));
        }
    }

    #[test]
    fn control_characters_cannot_escape_into_the_terminal() {
        let error = describe(400, &b"guard\n\r\x1b[31m\0refused"[..], "token");
        assert!(!error.chars().any(char::is_control));
        assert!(error.contains("guard"));
        assert!(error.contains("refused"));
    }

    #[test]
    fn oversized_body_is_bounded_and_never_partially_printed() {
        let body = vec![b'x'; BODY_LIMIT * 2];
        let mut reader = std::io::Cursor::new(body);
        let error = describe(413, &mut reader, "token");
        assert_eq!(reader.position(), (BODY_LIMIT + 1) as u64);
        assert_eq!(error, "status 413; response body exceeds 4096 bytes");
    }

    #[test]
    fn malformed_empty_and_unreadable_bodies_remain_refusals() {
        assert!(describe(400, &b"not-json\xff"[..], "token").contains("not-json"));
        assert_eq!(
            describe(422, &b""[..], "token"),
            "status 422; empty response body"
        );
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("must not print reader internals"))
            }
        }
        assert_eq!(
            describe(400, Broken, "token"),
            "status 400; response body unreadable"
        );
    }
}
