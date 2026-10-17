//! Offline JSON export: cargo run --features langs-extra --example `http_candidates` -- ROOT REPO REF
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err("expected ROOT REPOSITORY REVISION".into());
    }
    let snapshot =
        yupana::export::http_candidates_at(std::path::Path::new(&args[0]), &args[1], &args[2])?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}
