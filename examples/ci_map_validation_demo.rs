//! Repeatable CI-map integrity demonstration; no commands are executed.

use yupana::ci_shift::{decode_map, select, CiCheck};

fn main() {
    let valid = CiCheck {
        id: "ci:source".into(),
        label: "source check".into(),
        command: "true".into(),
        scopes: vec!["src/**/*.rs".into()],
    };
    println!(
        "valid matching scope: {:?}",
        select(std::slice::from_ref(&valid), &["src/lib.rs".into()])
    );
    println!(
        "unrelated path: {:?}",
        select(std::slice::from_ref(&valid), &["docs/readme.md".into()])
    );
    let malformed = CiCheck {
        scopes: vec!["src/[".into()],
        ..valid
    };
    println!(
        "malformed scope: {:?}",
        select(&[malformed], &["src/lib.rs".into()])
    );
    let partial = serde_json::json!({"truncated":true,"results":{"bindings":[{
        "job":{"value":"ci:source"},"label":{"value":"source check"},
        "command":{"value":"true"},"scope":{"value":"src/**/*.rs"}
    }]}});
    println!(
        "truncated projection: {:?}",
        decode_map(&partial.to_string())
    );
}
