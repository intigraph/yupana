"""Record real CLI summaries for one disposable code/document fixture."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


def export(binary, root):
    env = {k: v for k, v in os.environ.items()
           if not any(t in k for t in ("TOKEN", "QUIPU", "SHANTY", "SEEDS", "YUPANA"))}
    env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "home/config"),
               XDG_CACHE_HOME=str(root / "home/cache"))
    result = subprocess.run([binary, "export", str(root), "--repo", "label-fixture",
                             "--format", "turtle"], env=env, capture_output=True,
                            text=True, check=True)
    ttl = result.stdout
    return ttl, {
        "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
        "module_basename_label": 'rdfs:label "lib.rs"' in ttl,
        "document_basename_label": 'rdfs:label "guide.md"' in ttl,
        "section_leaf_label": 'rdfs:label "Setup"' in ttl,
        "section_heading": 'bobbin:heading "Setup"' in ttl,
        "owned_symbol_label": 'rdfs:label "parse"' in ttl,
        "full_document_path": 'bobbin:filePath "docs/guide.md"' in ttl,
    }


def structure(ttl):
    return [line.strip() for line in ttl.splitlines() if line.strip()
            and "rdfs:label " not in line and "bobbin:heading " not in line]


with tempfile.TemporaryDirectory(prefix="label-ownership-demo-") as directory:
    root = Path(directory)
    for folder in ("src", "docs", "home"):
        (root / folder).mkdir()
    (root / "src/lib.rs").write_text("pub fn parse() {}\n")
    (root / "docs/guide.md").write_text("# Guide\n\n## Setup\nUse `parse`.\n")
    print("$ yupana export FIXTURE --repo label-fixture --format turtle", flush=True)
    print("Real export summaries, same nested-path fixture:", flush=True)
    before, b = export(os.environ["DEMO_BEFORE"], root)
    print("BEFORE " + json.dumps(b, sort_keys=True), flush=True)
    after, a = export(os.environ["DEMO_AFTER"], root)
    print("AFTER  " + json.dumps(a, sort_keys=True), flush=True)
    for key in ("module_basename_label", "document_basename_label",
                "section_leaf_label", "section_heading"):
        assert b[key] and not a[key], key
    assert a["owned_symbol_label"] and a["full_document_path"]
    assert structure(before) == structure(after)
    print("CONTROL: all structural lines and IRIs identical; symbol label retained", flush=True)
