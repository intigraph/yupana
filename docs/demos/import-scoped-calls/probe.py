#!/usr/bin/env python3
"""Isolated same-input CLI demonstration; no daemon or graph writes."""
import json
import os
import subprocess
import tempfile
from pathlib import Path

with tempfile.TemporaryDirectory(prefix="yupana-import-demo-") as tmp:
    root = Path(tmp)
    for name in ("a", "b"):
        (root / f"{name}.py").write_text("def leaf(): pass\n")
    (root / "main.py").write_text(
        "from a import leaf\ndef invoke(): leaf()\ndef positive(): invoke()\n"
    )
    for label, variable, expected in (
        ("BEFORE: installed 0.14.0 baseline", "BASELINE_BIN", ["a.py", "b.py"]),
        ("AFTER: candidate source build", "CANDIDATE_BIN", ["a.py"]),
    ):
        print(label, flush=True)
        print("$ yupana callers invoke FIXTURE --json", flush=True)
        data = json.loads(subprocess.check_output(
            [os.environ[variable], "callers", "invoke", str(root), "--json"], text=True
        ))
        print(json.dumps(data, indent=2), flush=True)
        assert sorted(r["file"] for r in data["callees"]) == expected
        assert [r["name"] for r in data["callers"]] == ["positive"]
    print("PASS: two targets -> imported target only; positive caller retained.", flush=True)
