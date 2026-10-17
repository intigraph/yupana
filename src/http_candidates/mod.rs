//! Opt-in, offline static HTTP candidates. No graph writes or runtime inference.
mod model;
#[cfg(feature = "langs-extra")]
mod python;
use crate::errors::{Error, Result};
pub use model::*;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Read Python sources from an immutable committed tree, using the exporter's
/// existing source selection. Uncommitted bytes can never enter this snapshot.
/// Callers must revalidate repository, revision and source hashes at consumption.
pub fn at(root: &Path, repository: &str, reference: &str) -> Result<Snapshot> {
    if reference.starts_with('-') || repository.is_empty() {
        return Err(Error::Parse(
            "invalid revision or repository identity".into(),
        ));
    }
    if !crate::extract::languages().contains(&"python") {
        return Err(Error::UnsupportedLanguage(
            "python (requires langs-extra)".into(),
        ));
    }
    let revision = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--verify", &format!("{reference}^{{commit}}")])
        .output()?;
    if !revision.status.success() {
        return Err(Error::Parse("unverifiable committed revision".into()));
    }
    let revision = String::from_utf8_lossy(&revision.stdout).trim().to_string();
    if revision.len() != 40 && revision.len() != 64 {
        return Err(Error::Parse("unverifiable committed revision".into()));
    }
    let mut result = Snapshot {
        contract: "yupana-http-candidates/v1".into(),
        profile: "python-fastapi-requests/v1".into(),
        extractor: Extractor {
            name: "yupana".into(),
            version: env!("YUPANA_BUILD_VERSION").into(),
            profile_version: 1,
        },
        repository: repository.into(),
        revision: revision.clone(),
        observed_at: chrono::Utc::now().to_rfc3339(),
        files: Vec::new(),
        coverage: Coverage { limitations: vec![
            "Python only; module-level from-fastapi constructors and requests/httpx module imports".into(),
            "Same-file literal prefixes/mounts only; no cross-file binding or runtime dispatch".into(),
            "SDK/client instances and generic request() calls are outside profile coverage".into(),
            "Generated status is unknown unless an explicit # @generated header is present; never infer nongenerated".into(),
        ], ..Coverage::default() },
    };
    let inventory = crate::git::run(root, &["ls-tree", "-r", "-z", "--full-tree", &revision])
        .ok_or_else(|| Error::Parse("committed source inventory unavailable".into()))?;
    for entry in inventory.split('\0').filter(|p| !p.is_empty()) {
        let (metadata, name) = entry
            .split_once('\t')
            .ok_or_else(|| Error::Parse("malformed Git source inventory".into()))?;
        let path = std::path::PathBuf::from(name);
        let Some(language) = crate::extract::selectable_language(&path) else {
            continue;
        };
        if language != "python" {
            result.coverage.unsupported_files += 1;
            continue;
        }
        validate_module(name)?;
        if !metadata.starts_with("100644 blob ") && !metadata.starts_with("100755 blob ") {
            return Err(Error::Parse(
                "non-regular Python source in committed inventory".into(),
            ));
        }
        let source = crate::git::read_blob_at(root, &revision, &path).ok_or_else(|| {
            Error::Parse(format!("unreadable committed source: {}", path.display()))
        })?;
        let mut file = extract(&source, &path.to_string_lossy())?;
        file.snapshot_state = "verified_commit".into();
        for candidate in &mut file.candidates {
            candidate.id = candidate_id(
                repository,
                Some(&revision),
                &file.module,
                &file.source_sha256,
                candidate,
            )?;
            if candidate.resolution == "literal" {
                result.coverage.literal_records += 1;
            } else {
                result.coverage.unresolved_records += 1;
            }
        }
        result.coverage.profiled_files += 1;
        result.coverage.parse_errors += usize::from(!file.diagnostics.is_empty());
        result.files.push(file);
    }
    result.files.sort_by(|a, b| a.module.cmp(&b.module));
    Ok(result)
}

/// Parse a source buffer as unverified working-tree evidence. Only `at` binds
/// records to committed bytes. No graph/profile activation occurs here.
pub fn extract(source: &str, module: &str) -> Result<FileCandidates> {
    validate_module(module)?;
    #[cfg(feature = "langs-extra")]
    {
        python::extract(source, module)
    }
    #[cfg(not(feature = "langs-extra"))]
    {
        let _ = (source, module);
        Err(Error::UnsupportedLanguage(
            "python (requires langs-extra)".into(),
        ))
    }
}

fn validate_module(module: &str) -> Result<()> {
    if module
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
        || module.is_empty()
        || module.contains('\\')
        || Path::new(module)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Parse(
            "module must be a canonical root-relative path".into(),
        ));
    }
    Ok(())
}
fn candidate_id(
    repository: &str,
    revision: Option<&str>,
    module: &str,
    hash: &str,
    c: &Candidate,
) -> Result<String> {
    let key = serde_json::to_vec(&(
        "yupana-http-candidates/v1",
        "python-fastapi-requests/v1",
        env!("YUPANA_BUILD_VERSION"),
        repository,
        revision,
        module,
        hash,
        &c.role,
        (&c.occurrence_span.start_byte, &c.occurrence_span.end_byte),
        c.prefix_spans
            .iter()
            .map(|s| (s.start_byte, s.end_byte))
            .collect::<Vec<_>>(),
        &c.method,
        &c.path,
        &c.authority,
    ))
    .map_err(|e| Error::Parse(e.to_string()))?;
    Ok(hex::encode(Sha256::digest(key)))
}
/// Refuse stale/foreign snapshots before any downstream admission. Verification
/// includes current HEAD and working source bytes; it never silently substitutes
/// newer source or turns an unavailable read into a successful zero.
pub fn verify_current(root: &Path, repository: &str, snapshot: &Snapshot) -> Result<()> {
    let head = crate::git::run(root, &["rev-parse", "HEAD"])
        .ok_or_else(|| Error::Parse("current HEAD unavailable".into()))?;
    if repository != snapshot.repository || head.trim() != snapshot.revision {
        return Err(Error::Parse("stale or foreign HTTP snapshot".into()));
    }
    for file in &snapshot.files {
        validate_module(&file.module)?;
        let path = root.join(&file.module);
        if std::fs::symlink_metadata(&path)?.file_type().is_symlink()
            || !path.canonicalize()?.starts_with(root.canonicalize()?)
        {
            return Err(Error::Parse("unsafe HTTP source path".into()));
        }
        let mut component_path = root.to_path_buf();
        for component in Path::new(&file.module).components() {
            component_path.push(component.as_os_str());
            if std::fs::symlink_metadata(&component_path)?
                .file_type()
                .is_symlink()
            {
                return Err(Error::Parse("symlink in HTTP source path".into()));
            }
        }
        let committed = crate::git::read_blob_at(root, &snapshot.revision, Path::new(&file.module))
            .ok_or_else(|| Error::Parse("committed HTTP source unavailable".into()))?;
        if hex::encode(Sha256::digest(committed.as_bytes())) != file.source_sha256 {
            return Err(Error::Parse(
                "HTTP snapshot does not bind committed bytes".into(),
            ));
        }
        let bytes = std::fs::read(&path)?;
        if file.snapshot_state != "verified_commit"
            || hex::encode(Sha256::digest(&bytes)) != file.source_sha256
        {
            return Err(Error::Parse("stale or unverified HTTP source bytes".into()));
        }
    }
    Ok(())
}

#[cfg(all(test, not(feature = "langs-extra")))]
mod tests {
    #[test]
    fn unsupported_profile_never_returns_a_successful_zero() {
        assert!(matches!(
            super::extract("", "a.py"),
            Err(crate::errors::Error::UnsupportedLanguage(_))
        ));
    }
}
