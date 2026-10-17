#!/usr/bin/env python3
"""Reproduce offline SCIP acceptance against synthetic source, without indexing."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

REPO = Path(__file__).resolve().parents[3]
FIXTURE = REPO / "tests/fixtures/scip"


def invoke(binary, args):
    result = subprocess.run([binary, *args], capture_output=True, text=True, check=False)
    return result.returncode, result.stdout, result.stderr


def main():
    before = os.environ["YUPANA_SCIP_BASE"]
    after = os.environ["YUPANA_SCIP_HEAD"]
    for label, binary in [("before", before), ("candidate", after)]:
        version = invoke(binary, ["--version"])[1].strip()
        digest = hashlib.sha256(Path(binary).read_bytes()).hexdigest()
        print(f"{label}: {version}; binary sha256={digest}", flush=True)
    with tempfile.TemporaryDirectory() as directory:
        root = Path(directory)
        (root / "src").mkdir()
        source = (FIXTURE / "src/lib.rs").read_bytes()
        (root / "src/lib.rs").write_bytes(source)
        def git(*args):
            return subprocess.check_output(["git", "-C", directory, *args], stderr=subprocess.DEVNULL, text=True).strip()
        git("init", "-q")
        git("remote", "add", "origin", "https://example.com/offline-fixture.git")
        git("add", ".")
        git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.com", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture")
        index = FIXTURE / "index.scip"
        binding = {
            "version": 1, "repository": "https://example.com/offline-fixture.git",
            "revision": git("rev-parse", "HEAD"),
            "index_sha256": hashlib.sha256(index.read_bytes()).hexdigest(),
            "project_root": "file:///offline-scip-fixture", "producer": "rust-analyzer",
            "producer_version": "1.98.0 (88d9e12 2026-08-18)",
            "sources": {"src/lib.rs": hashlib.sha256(source).hexdigest()},
        }
        binding_file = root / "binding.json"
        binding_file.write_text(json.dumps(binding))
        args = ["import-scip", "--root", directory, "--index", str(index), "--binding", str(binding_file)]
        print("$ yupana import-scip --root SYNTHETIC --index FIXTURE --binding ATTESTATION", flush=True)
        rc, _, error = invoke(before, args)
        print(f"before: exit={rc}; {error.splitlines()[0]}", flush=True)
        assert rc != 0 and "import-scip" in error
        rc, stdout, _ = invoke(after, args)
        assert rc == 0, stdout
        report = json.loads(stdout)
        definitions = [o for o in report["occurrences"] if o["definition"] and "duplicate" in o["symbol"]]
        assert len(definitions) == 2 and definitions[0]["symbol"] != definitions[1]["symbol"]
        print(f"candidate: tier={report['tier']}; coverage={report['coverage']}", flush=True)
        for definition in definitions:
            identity = definition["symbol"]
            related = [o for o in report["occurrences"] if o["symbol"] == identity]
            assert len(related) == 2 and definition["ast_body"] is not None
            print(f"  {identity}: one definition + one reference; identifier={definition['identifier']}; AST body lines={definition['ast_body']['start_line']}..{definition['ast_body']['end_line']}", flush=True)
        (root / "src/lib.rs").write_bytes(source + b"// stale edit\n")
        rc, stdout, _ = invoke(after, args)
        report = json.loads(stdout)
        assert rc != 0 and not report["occurrences"] and not report["coverage"]["accepted"]
        print(f"stale-byte control: exit={rc}; facts=0; {report['coverage']['refusal']}", flush=True)
        print("PASS: distinct identities, separate body spans, stale-byte refusal", flush=True)


if __name__ == "__main__":
    main()
