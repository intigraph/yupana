//! Read-only stack orientation, separate from graph-building and projection refresh.

#[cfg(feature = "quipu")]
mod cache;
mod state;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args as ClapArgs;
use serde_json::{json, Value};

const PRIMER: &str = include_str!("../primer.md");

#[derive(Debug, ClapArgs)]
pub(super) struct Args {
    /// Analysis root whose project configuration is inspected.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Five to ten lines for stack aggregation.
    #[arg(long, conflicts_with = "section")]
    brief: bool,
    /// Skip configuration and cache reads.
    #[arg(long = "static")]
    static_only: bool,
    /// One section by case-insensitive heading prefix.
    #[arg(long, value_parser = validate_section)]
    section: Option<String>,
}

fn sections() -> Vec<&'static str> {
    PRIMER
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .collect()
}

fn select(wanted: Option<&str>) -> Result<&'static str, String> {
    let Some(wanted) = wanted else {
        return Ok(PRIMER);
    };
    let needle = wanted.trim().to_lowercase();
    let mut offset = 0;
    for block in PRIMER.split_inclusive('\n') {
        if let Some(heading) = block.strip_prefix("## ") {
            if !needle.is_empty() && heading.trim_end().to_lowercase().starts_with(&needle) {
                let rest = &PRIMER[offset..];
                let end = rest.find("\n## ").map_or(rest.len(), |end| end + 1);
                return Ok(&rest[..end]);
            }
        }
        offset += block.len();
    }
    Err(format!(
        "unknown primer section {wanted:?}; sections: {}",
        sections().join(", ")
    ))
}

fn validate_section(value: &str) -> Result<String, String> {
    select(Some(value)).map(|_| value.to_owned())
}

pub(super) fn run(args: &Args, json_output: bool, config: Option<&Path>) -> anyhow::Result<()> {
    let text = select(args.section.as_deref()).map_err(anyhow::Error::msg)?;
    let live = if args.static_only {
        json!({"status":"skipped","elapsed_ms":0,"summary":{}})
    } else {
        state::summarize(&args.path, config)
    };
    let status = if matches!(live["status"].as_str(), Some("ok" | "skipped")) {
        "ok"
    } else {
        "degraded"
    };
    let version = env!("YUPANA_BUILD_VERSION");
    let mut output = json!({"schema":"stack-prime/1","tool":"yupana","version":version,
        "primer":{"sections":sections()},"live":live,"status":status});
    if !args.brief {
        output["primer"]["text"] = json!(text);
    }
    let mut stdout = io::stdout().lock();
    if json_output {
        writeln!(stdout, "{output}")?;
    } else if args.brief {
        let mut lines = vec![format!(
            "yupana {version}: explains code structure and edit impact"
        )];
        if args.static_only {
            lines.extend(
                select(Some("What yupana is"))
                    .map_err(anyhow::Error::msg)?
                    .lines()
                    .skip(1)
                    .filter(|line| !line.trim().is_empty())
                    .map(str::to_owned),
            );
        } else {
            lines.extend(live_lines(&output["live"]));
        }
        lines.push("Run `yupana prime` for the full primer.".to_owned());
        writeln!(stdout, "{}", lines.join("\n"))?;
    } else {
        write!(stdout, "{text}")?;
        if !args.static_only {
            writeln!(
                stdout,
                "\n## Live state\n\n{}",
                live_lines(&output["live"]).join("\n")
            )?;
        }
    }
    Ok(())
}

fn live_lines(live: &Value) -> Vec<String> {
    let s = &live["summary"];
    let mut lines = vec![
        format!("Root: {}", s["root"].as_str().unwrap_or("unknown")),
        format!("State: {}", live["status"].as_str().unwrap_or("unreadable")),
    ];
    if let Some(path) = s["path"].as_str() {
        lines.push(format!("Path: {path}"));
    } else if s["configs"].is_array() {
        lines.push(format!("Configuration: {}", s["configs"]));
    }
    if let Some(base) = s["base_ref"].as_str() {
        lines.push(format!(
            "Configured base: {base}; compiled tiers: {}",
            s["tiers"]
        ));
    }
    if let Some(count) = s["governed_rules"]["count"].as_u64() {
        lines.push(format!(
            "Cached governed rules: {count} ({}); age: {} s",
            s["governed_rules"]["status"].as_str().unwrap_or("unknown"),
            s["governed_rules"]["age_secs"]
        ));
    } else {
        lines.push(format!(
            "Governed rules: {}",
            s["governed_rules"]["status"].as_str().unwrap_or("unknown")
        ));
    }
    if let Some(path) = s["cache_path"].as_str() {
        lines.push(format!("Cache: {path}"));
    }
    if let Some(reason) = s["reason"].as_str() {
        lines[1].push_str(&format!(" ({reason})"));
    }
    if let Some(next) = s["next"].as_str() {
        lines.push(format!("Next: {next}"));
    }
    lines.push(format!("Read: {} ms", live["elapsed_ms"]));
    lines
        .iter_mut()
        .for_each(|line| *line = line.replace('\n', "\\n").replace('\r', "\\r"));
    lines
}
