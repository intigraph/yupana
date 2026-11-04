//! Commit-boundary scheduler facts. Parsing is native; severity is a governed
//! `TextRule` over these facts, never an exposure-dependent pre-edit warning.
//! This is an accidental scheduling guard, not an interpreter for arbitrary
//! programs that compute commands or generate configuration at runtime.

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use yaml_rust2::{Yaml, YamlLoader};

/// Versioned projection class. Its suffix names the governed exception parent.
pub const RULE_CLASS: &str = "commit-scheduler/v1:";

/// One independently comparable scheduler definition.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct Fact {
    /// Git path, not a worktree path.
    pub path: String,
    /// Stable scheduler family.
    pub kind: String,
    /// Complete scheduling definition, including values.
    pub definition: String,
    /// Exact unit or line an exception description must name.
    pub exception_target: String,
}

impl Fact {
    fn new(path: &str, kind: &str, definition: String, unit: bool) -> Self {
        let exception_target = if unit {
            path.to_owned()
        } else {
            format!("{path} :: {definition}")
        };
        Self {
            path: path.into(),
            kind: kind.into(),
            definition,
            exception_target,
        }
    }

    /// Canonical text consumed by the existing governed `TextRule` evaluator.
    pub fn text(&self) -> String {
        format!("external-scheduler/v1:{}:{}", self.kind, self.definition)
    }
}

/// Multiset subtraction: duplicate new definitions count; unchanged debt and
/// removals do not. Renames are deliberately additions at their new path.
pub fn introduced(before: Vec<Fact>, after: Vec<Fact>) -> Vec<Fact> {
    let mut counts = BTreeMap::<Fact, usize>::new();
    for fact in before {
        *counts.entry(fact).or_default() += 1;
    }
    after
        .into_iter()
        .filter(|fact| {
            let n = counts.entry(fact.clone()).or_default();
            if *n == 0 {
                true
            } else {
                *n -= 1;
                false
            }
        })
        .collect()
}

/// Extract executable/configuration definitions. Documentation is data, not a
/// deployed scheduler. Unsupported executable source containing scheduler tokens
/// is UNKNOWN rather than silently called clean.
pub fn facts(path: &str, bytes: &[u8]) -> Result<Vec<Fact>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if matches!(extension, "md" | "rst" | "txt" | "log") {
        return Ok(Vec::new());
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_)
            if matches!(
                extension,
                "timer" | "plist" | "yaml" | "yml" | "sh" | "bash" | "zsh" | "cron"
            ) =>
        {
            bail!("non-UTF8 scheduler source is unmeasured")
        }
        Err(_) => return Ok(Vec::new()),
    };
    if [
        "cron.hourly/",
        "cron.daily/",
        "cron.weekly/",
        "cron.monthly/",
    ]
    .iter()
    .any(|part| path.contains(part))
    {
        return Ok(vec![Fact::new(
            path,
            "cron-entry",
            text.trim().into(),
            true,
        )]);
    }
    if extension == "timer" {
        return timer(path, text);
    }
    if extension == "plist" {
        return plist(path, text);
    }
    if matches!(extension, "yaml" | "yml") {
        return yaml(path, text);
    }
    if extension == "cron"
        || path.ends_with("/crontab")
        || path == "crontab"
        || path.contains("/cron.d/")
        || path.starts_with("cron.d/")
    {
        return Ok(text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                (!line.is_empty()
                    && !line.starts_with('#')
                    && !line.split_whitespace().next().unwrap_or("").contains('='))
                .then(|| Fact::new(path, "cron-entry", line.into(), false))
            })
            .collect());
    }
    if matches!(extension, "sh" | "bash" | "zsh")
        || text.starts_with("#!/bin/sh")
        || text.starts_with("#!/bin/bash")
        || text.starts_with("#!/usr/bin/env bash")
    {
        return shell(path, text);
    }
    if [
        "crontab",
        "/etc/cron.",
        "StartCalendarInterval",
        "StartInterval",
        "OnCalendar=",
        "OnUnitActiveSec=",
        "OnBootSec=",
    ]
    .iter()
    .any(|token| text.contains(token))
    {
        bail!("scheduler tokens in unsupported source format at {path}; explicit review required");
    }
    Ok(Vec::new())
}

fn timer(path: &str, text: &str) -> Result<Vec<Fact>> {
    let mut section = "";
    let mut definitions = BTreeMap::<String, Vec<String>>::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }
        if section != "Timer" {
            continue;
        }
        let (key, value) = line.split_once('=').context("malformed Timer directive")?;
        // Empty assignments reset systemd list directives. Compare effective
        // values, so removing one cadence passes and removing a reset cannot
        // reactivate a previous cadence unnoticed.
        let values = definitions.entry(key.trim().into()).or_default();
        if value.trim().is_empty() {
            values.clear();
        } else {
            values.push(value.trim().into());
        }
    }
    let context = definitions
        .iter()
        .filter(|(key, _)| !key.starts_with("On"))
        .map(|(key, values)| format!("{key}={}", values.join(";")))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(definitions
        .iter()
        .filter(|(key, _)| key.starts_with("On"))
        .flat_map(|(key, values)| {
            let context = &context;
            values.iter().map(move |value| {
                Fact::new(
                    path,
                    "systemd-timer",
                    format!("{key}={value}\n{context}"),
                    true,
                )
            })
        })
        .collect())
}

#[path = "commit_schedule_plist.rs"]
mod plist_parser;

fn plist(path: &str, text: &str) -> Result<Vec<Fact>> {
    plist_parser::facts(path, text)
}

fn yaml(path: &str, text: &str) -> Result<Vec<Fact>> {
    let docs = YamlLoader::load_from_str(text).context("malformed scheduler YAML")?;
    let workflow =
        path.starts_with(".github/workflows/") || path.starts_with(".forgejo/workflows/");
    let mut out = Vec::new();
    for doc in docs {
        yaml_walk(path, &doc, &mut Vec::new(), workflow, &mut out)?;
    }
    Ok(out)
}

fn yaml_walk(
    path: &str,
    node: &Yaml,
    trail: &mut Vec<String>,
    workflow: bool,
    out: &mut Vec<Fact>,
) -> Result<()> {
    match node {
        Yaml::Hash(map) => {
            for (key, value) in map {
                let key = key.as_str().context("non-string YAML mapping key")?;
                let cron = matches!(
                    key,
                    "cron" | "ansible.builtin.cron" | "community.general.cron"
                );
                // An action's run:/shell:/command: content is NOT a workflow
                // trigger. Only the direct on.schedule subtree is exempt.
                let ci_trigger = workflow && trail.as_slice() == ["on"] && key == "schedule";
                if ci_trigger {
                    continue;
                }
                if cron || key == "schedule" {
                    let absent = cron && value["state"].as_str() == Some("absent");
                    if !absent && !matches!(value, Yaml::Null) {
                        let entries = if let Yaml::Array(entries) = value {
                            entries.iter().collect::<Vec<_>>()
                        } else {
                            vec![value]
                        };
                        for entry in entries {
                            out.push(Fact::new(
                                path,
                                if cron {
                                    "ansible-cron"
                                } else {
                                    "schedule-block"
                                },
                                format!("{key}: {}", yaml_value(entry)?),
                                false,
                            ));
                        }
                    }
                } else {
                    trail.push(key.into());
                    yaml_walk(path, value, trail, workflow, out)?;
                    trail.pop();
                }
            }
        }
        Yaml::Array(values) => {
            for value in values {
                yaml_walk(path, value, trail, workflow, out)?;
            }
        }
        Yaml::String(value) => out.extend(shell(path, value)?),
        Yaml::Alias(_) | Yaml::BadValue => bail!("unresolved YAML alias/value"),
        _ => {}
    }
    Ok(())
}

fn yaml_value(value: &Yaml) -> Result<String> {
    Ok(match value {
        Yaml::Hash(fields) => {
            let mut fields = fields
                .iter()
                .map(|(key, value)| Ok(format!("{}:{}", yaml_value(key)?, yaml_value(value)?)))
                .collect::<Result<Vec<_>>>()?;
            fields.sort();
            format!("{{{}}}", fields.join(","))
        }
        Yaml::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(yaml_value)
                .collect::<Result<Vec<_>>>()?
                .join(",")
        ),
        Yaml::Alias(_) | Yaml::BadValue => bail!("unresolved YAML definition"),
        value => format!("{value:?}"),
    })
}

fn shell(path: &str, text: &str) -> Result<Vec<Fact>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_bash::LANGUAGE.into())?;
    let tree = parser
        .parse(text, None)
        .context("shell parser did not return a tree")?;
    // YAML prose/string scalars need not be shell. Only fail on invalid shell
    // when a scheduling token is present, never claim it was parsed cleanly.
    let candidate = text.contains("crontab") || text.contains("/etc/cron.");
    if tree.root_node().has_error() && candidate {
        bail!("unparsed scheduler shell at {path}");
    }
    let mut out = Vec::new();
    shell_walk(path, text.as_bytes(), tree.root_node(), &mut out)?;
    Ok(out)
}

fn shell_walk(
    path: &str,
    bytes: &[u8],
    node: tree_sitter::Node<'_>,
    out: &mut Vec<Fact>,
) -> Result<()> {
    if node.kind() == "command" {
        let source = node.utf8_text(bytes)?;
        let name = node
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(bytes).ok())
            .unwrap_or("");
        let words = shell_words::split(source).unwrap_or_default();
        let name = name.trim_matches(['\'', '"']);
        let direct = name.rsplit('/').next() == Some("crontab");
        let wrapped = matches!(name, "sudo" | "env" | "command" | "exec" | "timeout")
            && words
                .iter()
                .any(|s| s.rsplit('/').next() == Some("crontab"));
        if direct || wrapped {
            let pos = words
                .iter()
                .position(|s| s.rsplit('/').next() == Some("crontab"));
            let listing = pos.is_some_and(|p| {
                let args = &words[p + 1..];
                args.len() == 1
                    && matches!(
                        args[0].as_str(),
                        "-l" | "-h" | "-V" | "--help" | "--version"
                    )
            });
            if !listing {
                out.push(Fact::new(
                    path,
                    "crontab-write",
                    source.trim().into(),
                    false,
                ));
            }
        }
        if matches!(
            name.rsplit('/').next(),
            Some("tee" | "cp" | "mv" | "install")
        ) && words.iter().any(|word| word.starts_with("/etc/cron."))
        {
            out.push(Fact::new(
                path,
                "cron-file-write",
                source.trim().into(),
                false,
            ));
        }
        // Explicit nested shell strings, which the outer bash AST correctly
        // treats as data, become code when supplied to sh/bash -c.
        if matches!(name.rsplit('/').next(), Some("sh" | "bash" | "zsh")) {
            if let Some(p) = words.iter().position(|s| s == "-c") {
                if let Some(body) = words.get(p + 1) {
                    out.extend(shell(path, body)?);
                }
            }
        }
    }
    if node.kind() == "file_redirect" {
        let source = node.utf8_text(bytes)?;
        if source.contains('>') && source.contains("/etc/cron.") {
            out.push(Fact::new(
                path,
                "cron-file-write",
                source.trim().into(),
                false,
            ));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        shell_walk(path, bytes, child, out)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "commit_schedule_test.rs"]
mod tests;
