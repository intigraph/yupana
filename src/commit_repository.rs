//! Origin-derived, governed repository identity for scheduler exceptions.

use anyhow::{ensure, Context, Result};
use std::path::Path;

/// Derive identity from Git's actual origin, never a commit trailer or a
/// directory basename. Only a unique URI-typed `GitRepo` registration can bind it.
pub(super) fn repository_iri(repo: &Path, endpoint: &str) -> Result<String> {
    let remote = super::git_text(repo, &["remote", "get-url", "origin"])?;
    let urls = remote_urls(&remote)?;
    let values = urls
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join(" ");
    let query = format!("PREFIX a: <http://aegis.gastown.local/ontology/> SELECT DISTINCT ?repo WHERE {{ VALUES ?url {{ {values} }} ?repo a a:GitRepo ; a:externalUrl ?registered . FILTER(STR(?registered) = ?url) }} LIMIT 2");
    let body = crate::project::query(endpoint, &query)?;
    decode_identity(&body)
}

fn decode_identity(body: &str) -> Result<String> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    ensure!(
        value.get("truncated").is_none_or(|v| v == false),
        "repository identity response truncated"
    );
    let rows = value["results"]["bindings"]
        .as_array()
        .context("repository identity response is not SPARQL results")?;
    ensure!(rows.len() == 1, "repository identity missing or ambiguous");
    ensure!(
        rows[0]["repo"]["type"] == "uri",
        "repository identity is not an IRI"
    );
    let iri = rows[0]["repo"]["value"]
        .as_str()
        .context("repository IRI missing")?;
    ensure!(
        iri.starts_with("https://") || iri.starts_with("http://") || iri.starts_with("urn:"),
        "repository IRI is not absolute"
    );
    Ok(iri.into())
}

/// Deliberately narrow forge remotes. Retain host AND owner/repo, reject local
/// paths, credentials/passwords, URL queries and opaque path spellings.
pub(super) fn remote_urls(raw: &str) -> Result<Vec<String>> {
    let raw = raw.trim().trim_end_matches('/');
    let (host, path) = if let Some((scheme, rest)) = raw.split_once("://") {
        ensure!(
            ["http", "https", "ssh"].contains(&scheme),
            "unsupported origin scheme"
        );
        let (authority, path) = rest
            .split_once('/')
            .context("origin has no repository path")?;
        (authority.rsplit('@').next().unwrap_or(""), path)
    } else {
        let (authority, path) = raw
            .split_once(':')
            .context("origin is not a forge remote")?;
        ensure!(
            authority.contains('@'),
            "local origin cannot establish repository identity"
        );
        (authority.rsplit('@').next().unwrap_or(""), path)
    };
    let path = path.strip_suffix(".git").unwrap_or(path);
    ensure!(
        !host.is_empty()
            && host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c)),
        "invalid origin host"
    );
    let parts: Vec<_> = path.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|part| !part.is_empty()
                && *part != "."
                && *part != ".."
                && part
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))),
        "invalid origin owner/repository"
    );
    let host = host.to_ascii_lowercase();
    Ok(["https", "http", "ssh"]
        .into_iter()
        .flat_map(|scheme| {
            [
                format!("{scheme}://{host}/{path}"),
                format!("{scheme}://{host}/{path}.git"),
            ]
        })
        .chain([
            format!("git@{host}:{path}"),
            format!("git@{host}:{path}.git"),
        ])
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identity_requires_one_uri_and_complete_results() {
        let row = json!({"repo":{"type":"uri","value":"https://example.org/repos/one"}});
        let good = json!({"results":{"bindings":[row.clone()]}});
        assert_eq!(
            decode_identity(&good.to_string()).unwrap(),
            "https://example.org/repos/one"
        );
        for bad in [
            json!({"results":{"bindings":[]}}),
            json!({"results":{"bindings":[row.clone(),row.clone()]}}),
            json!({"results":{"bindings":[{"repo":{"type":"literal","value":"https://example.org/repos/one"}}]}}),
            json!({"results":{"bindings":[row.clone()]},"truncated":true}),
            json!({"results":{"bindings":[row]},"truncated":"false"}),
            json!({"rows":[]}),
        ] {
            assert!(decode_identity(&bad.to_string()).is_err(), "{bad}");
        }
    }
}
