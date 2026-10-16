//! A semantic briefing consumes admitted context, never diagnostic candidates.
use serde_json::Value;

pub(super) fn admitted(response: &Value) -> Result<Vec<(String, f64)>, &'static str> {
    let context = &response["context"];
    if response.get("error").is_some() || context["contract"].as_str() != Some("quipu-context/v1") {
        return Err("UNKNOWN");
    }
    let floor = context["similarity_floor"]
        .as_f64()
        .filter(|f| f.is_finite() && (0.52..=1.0).contains(f))
        .ok_or("UNKNOWN")?;
    if response["insufficient_context"].as_bool() == Some(true)
        && context["outcome"].as_str() == Some("NO CONTEXT")
    {
        return Err("NO CONTEXT");
    }
    if response["insufficient_context"].as_bool() != Some(false)
        || context["outcome"].as_str() != Some("CONTEXT")
    {
        return Err("UNKNOWN");
    }
    let rows = response["context_results"]
        .as_array()
        .filter(|r| !r.is_empty())
        .ok_or("UNKNOWN")?;
    rows.iter()
        .map(|row| {
            let iri = row["entity"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("UNKNOWN")?;
            let similarity = row["similarity"]
                .as_f64()
                .filter(|s| s.is_finite() && (floor..=1.0).contains(s))
                .ok_or("UNKNOWN")?;
            Ok((iri.to_owned(), similarity))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_stub::stub;
    use serde_json::json;

    #[test]
    fn unsupported_floor_never_certifies_no_context() {
        for floor in [json!(0.2), Value::Null, json!(true), json!(1.1)] {
            let body = json!({"insufficient_context":true,"context_results":[],
                "context":{"contract":"quipu-context/v1","outcome":"NO CONTEXT","similarity_floor":floor}});
            assert_eq!(admitted(&body), Err("UNKNOWN"));
        }
    }

    #[test]
    fn actual_source_no_context_never_falls_back_to_lexical_neighbors() {
        for (body, expected) in [
            (
                json!({"results":[{"entity":"urn:wrong","score":0.99}],"insufficient_context":true,
                "context":{"contract":"quipu-context/v1","outcome":"NO CONTEXT","similarity_floor":0.52},"context_results":[]}),
                "NO CONTEXT",
            ),
            (
                json!({"results":[{"entity":"urn:wrong","score":0.99}]}),
                "UNKNOWN",
            ),
        ] {
            let (endpoint, server) = stub(1, move |_, _| (200, body.clone()));
            let mut outcome = None;
            let items = super::super::similar_items(
                &endpoint,
                "fixture-1",
                "quaffleplex",
                None,
                &[],
                &mut outcome,
            );
            assert!(items.is_empty());
            assert_eq!(outcome, Some(expected));
            assert_eq!(server.join().unwrap().len(), 1);
            let rendered = crate::brief::render(&crate::brief::Brief {
                item: "fixture-1".into(),
                context_outcome: outcome,
                ..Default::default()
            });
            assert!(rendered.contains(expected));
            assert!(!rendered.contains("urn:wrong"));
            println!(
                "outcome={}; contributed_items={}; diagnostic_present={}",
                outcome.unwrap_or("UNKNOWN"),
                items.len(),
                rendered.contains("urn:wrong")
            );
            if expected == "UNKNOWN" {
                assert!(!rendered.contains("NO CONTEXT"));
            }
        }
    }

    #[test]
    fn admitted_positive_uses_raw_cosine_and_keeps_only_context_rows() {
        let mut body = json!({"results":[{"entity":"urn:wrong","score":100}],
            "insufficient_context":false,"context":{"contract":"quipu-context/v1","outcome":"CONTEXT","similarity_floor":0.52},
            "context_results":[{"entity":"urn:good","similarity":0.8,"score":0.01}]});
        assert_eq!(admitted(&body).unwrap(), vec![("urn:good".into(), 0.8)]);
        for similarity in [json!(0.51), json!("0.9"), Value::Null] {
            body["context_results"][0]["similarity"] = similarity;
            assert_eq!(admitted(&body), Err("UNKNOWN"));
        }
    }
}
