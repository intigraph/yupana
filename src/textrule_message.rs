//! The hook-facing message for one governed text-rule match, capped
//! (aegis-9z52).
//!
//! The pre-edit and pre-bash hooks deliver this text through `systemMessage`,
//! so the OPERATOR reads it on every fire. It used to carry the rule's whole
//! `rdfs:comment`: measured on the live projection, rationales ran to 1,302 B
//! (`pattern_internal-home-path-macos`), 821 B, 770 B and 768 B, which is a
//! screenful of prose per refusal. The rationale is the author's argument and
//! belongs in Quipu, where it is governed and versioned; the hook needs only
//! enough to say WHAT fired and WHY in one line, and a pointer to the rest.
//!
//! So the message is: the rule id, the matched token, its class, the label,
//! and either the whole rationale when it fits
//! or its first sentence and the pointer — bounded by
//! [`MESSAGE_CAP`] characters. The rule id and token are never cut: they are
//! the facts a reviewer adjudicates on. Only the prose shrinks.

/// The message budget, in characters. The rationale's first sentence gets
/// whatever the fixed parts leave; a fixed part that is itself longer (a huge
/// matched token) is kept whole and the sentence is dropped, never the facts.
pub const MESSAGE_CAP: usize = 300;

/// Build the capped message for rule `name` matching `token`.
#[must_use]
pub fn render(
    name: &str,
    label: &str,
    class: Option<&str>,
    rationale: Option<&str>,
    token: &str,
) -> String {
    let class = class.map_or_default(|c| format!(" ({c})"));
    let head =
        format!("governed text rule `{name}`: the edit introduces `{token}`{class} — {label}.");
    let Some(rationale) = rationale.map(str::trim).filter(|r| !r.is_empty()) else {
        return head;
    };
    let whole = format!("{head} {rationale}");
    if whole.chars().count() <= MESSAGE_CAP {
        return whole;
    }
    let pointer = format!(" Full rationale: Quipu `{name}` (rdfs:comment).");
    let fixed = head.chars().count() + 1 + pointer.chars().count();
    let why = clip(first_sentence(rationale), MESSAGE_CAP.saturating_sub(fixed));
    if why.is_empty() {
        // Not even a word fits: say where the reason lives rather than
        // printing a fragment of it.
        return format!("{head} Rationale: Quipu `{name}` (rdfs:comment).");
    }
    format!("{head} {why}{pointer}")
}

/// The rationale up to and including its first sentence terminator (`.`, `!`
/// or `?` followed by whitespace), or the whole text when it has none.
fn first_sentence(text: &str) -> &str {
    let bytes = text.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if matches!(b, b'.' | b'!' | b'?') && bytes.get(i + 1).is_some_and(u8::is_ascii_whitespace)
        {
            return &text[..=i];
        }
    }
    text
}

/// `text` cut to at most `room` characters on a word boundary, with an
/// ellipsis when cut. Empty when not even one word fits.
fn clip(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let budget = room.saturating_sub(1); // the ellipsis
    let cut: String = text.chars().take(budget).collect();
    match cut.rfind(char::is_whitespace) {
        Some(at) if at > 0 => format!("{}…", cut[..at].trim_end()),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "Absolute macOS home paths leak the operator's account name. \
        They also break on every other host, and the incident that motivated this \
        rule ran for three days before anyone noticed the path in a shared doc, \
        by which time it had been copied into four runbooks and a release note.";

    #[test]
    fn a_long_rationale_is_cut_to_its_first_sentence_with_a_pointer() {
        let m = render(
            "pattern_mac",
            "a macOS home path",
            Some("path"),
            Some(LONG),
            "/Users/x",
        );
        assert!(m.contains("leak the operator's account name."), "{m}");
        assert!(!m.contains("three days"), "{m}");
        assert!(
            m.ends_with("Full rationale: Quipu `pattern_mac` (rdfs:comment)."),
            "{m}"
        );
        assert!(
            m.chars().count() <= MESSAGE_CAP,
            "{} chars: {m}",
            m.chars().count()
        );
    }

    #[test]
    fn a_rationale_that_fits_is_printed_whole_without_a_pointer() {
        let m = render(
            "r",
            "a label",
            None,
            Some("Maps the private estate."),
            "tok",
        );
        assert_eq!(
            m,
            "governed text rule `r`: the edit introduces `tok` — a label. Maps the private estate."
        );
    }

    #[test]
    fn no_rationale_prints_the_head_only() {
        let m = render("r", "a label", Some("hostname"), None, "tok");
        assert_eq!(
            m,
            "governed text rule `r`: the edit introduces `tok` (hostname) — a label."
        );
    }

    #[test]
    fn an_overlong_first_sentence_is_clipped_on_a_word_and_still_points_home() {
        let rationale = "word ".repeat(200);
        let m = render("r", "a label", None, Some(&rationale), "tok");
        assert!(m.chars().count() <= MESSAGE_CAP, "{m}");
        assert!(m.contains("word…"), "{m}");
        assert!(
            m.ends_with("Full rationale: Quipu `r` (rdfs:comment)."),
            "{m}"
        );
    }

    #[test]
    fn a_huge_token_is_kept_whole_and_the_prose_gives_way() {
        let token = "x".repeat(400);
        let m = render("r", "a label", None, Some(LONG), &token);
        assert!(
            m.contains(&token),
            "the matched token is the adjudicated fact"
        );
        assert!(m.ends_with("Rationale: Quipu `r` (rdfs:comment)."), "{m}");
    }

    #[test]
    fn first_sentence_needs_whitespace_after_the_terminator() {
        assert_eq!(
            first_sentence("Use v1.2 here. Then more."),
            "Use v1.2 here."
        );
        assert_eq!(first_sentence("no terminator"), "no terminator");
    }
}
