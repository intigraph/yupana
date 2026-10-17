//! Explicit one-shot import; always prints machine-readable coverage.
use super::{Binding, Coverage};
use std::path::PathBuf;

/// Arguments for the opt-in offline importer.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Repository checkout; must be its top-level directory.
    #[arg(long)]
    root: PathBuf,
    /// Local protobuf SCIP index. Never downloaded or regenerated implicitly.
    #[arg(long)]
    index: PathBuf,
    /// JSON source/revision/index binding recorded when the index was produced.
    #[arg(long)]
    binding: PathBuf,
    /// Exact SCIP symbol identity; omit to return all validated occurrences.
    #[arg(long)]
    symbol: Option<String>,
}

impl Args {
    pub(crate) fn run(&self) -> anyhow::Result<()> {
        let result = (|| -> anyhow::Result<_> {
            let binding: Binding = serde_json::from_slice(&std::fs::read(&self.binding)?)?;
            let bytes = std::fs::read(&self.index)?;
            Ok(super::import(&self.root, &bytes, binding))
        })();
        let imported = match result {
            Ok(Ok(imported)) => imported,
            error => {
                let coverage = match error {
                    Ok(Err(coverage)) => coverage,
                    Err(error) => Coverage {
                        refusal: Some(format!("{error:#}")),
                        ..Coverage::default()
                    },
                    Ok(Ok(_)) => unreachable!(),
                };
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"tier":"scip", "coverage":coverage, "occurrences":[]})
                    )?
                );
                anyhow::bail!("SCIP import refused; see coverage.refusal");
            }
        };
        if let Some(symbol) = &self.symbol {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "tier": imported.tier, "binding": imported.binding,
                    "protocol_version": imported.protocol_version,
                    "coverage": imported.coverage, "symbol": symbol,
                    "occurrences": imported.lookup(symbol)
                }))?
            );
        } else {
            println!("{}", serde_json::to_string_pretty(&imported)?);
        }
        Ok(())
    }
}
