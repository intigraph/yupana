//! Shared scheduler path and token boundary; parsers cannot bypass this check.
use super::Fact;
use anyhow::{bail, Result};
use std::sync::OnceLock;

pub(super) enum CronPath {
    Unit,
    Lines,
    Unsupported,
}

pub(super) fn cron_path(path: &str) -> Option<CronPath> {
    let parts: Vec<_> = path.split('/').collect();
    if parts.iter().any(|p| {
        matches!(
            *p,
            "cron.hourly" | "cron.daily" | "cron.weekly" | "cron.monthly"
        )
    }) {
        return Some(CronPath::Unit);
    }
    if path.ends_with(".cron") || parts.contains(&"cron.d") || parts.last() == Some(&"crontab") {
        return Some(CronPath::Lines);
    }
    parts
        .iter()
        .any(|p| p.starts_with("cron"))
        .then_some(CronPath::Unsupported)
}

pub(super) fn scheduler_path(path: &str) -> bool {
    cron_path(path).is_some()
        || path.contains(".timer")
        || path.contains(".plist")
        || path.split('/').any(|p| {
            p == "systemd"
                || p.eq_ignore_ascii_case("LaunchAgents")
                || p.eq_ignore_ascii_case("LaunchDaemons")
        })
}

pub(super) fn tokens(text: &str) -> bool {
    static DIRECTIVE: OnceLock<regex::Regex> = OnceLock::new();
    let directive = DIRECTIVE.get_or_init(|| {
        regex::Regex::new(r"\bOn(?:Calendar|[A-Za-z0-9_]*Sec)[ \t]*=[ \t]*[^ \t\r\n]")
            .expect("constant scheduler token regex")
    });
    text.lines().any(|line| {
        !line.trim_start().starts_with(['#', ';'])
            && ([
                "crontab",
                "/etc/cron.",
                "StartInterval",
                "StartCalendarInterval",
                "systemd-run",
                ".timer",
            ]
            .iter()
            .any(|token| line.contains(token))
                || directive.is_match(line))
    })
}

pub(super) fn checked(path: &str, text: &str, found: Vec<Fact>) -> Result<Vec<Fact>> {
    if found.is_empty() && tokens(text) {
        bail!("unclassified scheduler tokens at {path}; explicit review required");
    }
    Ok(found)
}
