//! Owner-file argument conversion is shared by feature and feature-stub builds.
use super::PromoteArgs;
use std::path::Path;

/// Explicit paths only; never holds bearer contents.
#[cfg_attr(not(feature = "quipu"), allow(dead_code))]
pub struct OwnerFiles<'a> {
    /// Enrollment pins file selected by the caller.
    pub pins: &'a Path,
    /// Scoped bearer file selected by the caller.
    pub credential: &'a Path,
    /// Existing private directory for durable original-attempt custody.
    pub custody: &'a Path,
}
impl<'a> OwnerFiles<'a> {
    /// Convert explicit owner paths after CLI admission, without reading them.
    pub fn from_args(args: &'a PromoteArgs) -> anyhow::Result<Option<Self>> {
        if !args.owner {
            return Ok(None);
        }
        Ok(Some(Self {
            pins: args
                .owner_pins
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --owner-pins"))?,
            credential: args
                .owner_credential_file
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --owner-credential-file"))?,
            custody: args
                .owner_custody
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("missing --owner-custody"))?,
        }))
    }
}
