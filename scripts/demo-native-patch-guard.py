"""Compare two source-built hooks on the same isolated native patch."""
import json
import pathlib
import subprocess
import sys
import tempfile

patch = """*** Begin Patch
*** Add File: added.txt
+new
*** Update File: workflow.yml
@@
   - 'tests/existing.py'
+  - 'tests/new.py'
*** End Patch"""
with tempfile.TemporaryDirectory() as directory:
    root = pathlib.Path(directory)
    before = "paths:\n  - 'tests/existing.py'\nother_paths:\n  - 'tests/existing.py'\n"
    (root / "workflow.yml").write_text(before)
    (root / ".bobbin").mkdir()
    (root / ".bobbin/config.toml").write_text("[yupana.quipu]\nenabled = false\n")
    payload = json.dumps({"tool_name": "apply_patch", "cwd": directory,
                          "tool_input": {"command": patch}})
    print("Same input: Add File followed by Update File with repeated context", flush=True)
    for label, binary in zip(["BASE", "CANDIDATE"], sys.argv[1:], strict=True):
        result = subprocess.run([str(pathlib.Path(binary).resolve()), "hook", "pre-edit"],
                                cwd=directory, input=payload, capture_output=True, text=True, check=True)
        output = json.loads(result.stdout)
        print(f"{label} exit={result.returncode}: {result.stdout.strip()}", flush=True)
        denied = output.get("hookSpecificOutput", {}).get("permissionDecision") == "deny"
        assert denied == (label == "CANDIDATE")
        assert (root / "workflow.yml").read_text() == before
        assert not (root / "added.txt").exists()
    print("PASS: advisory -> explicit deny; hook itself writes neither target", flush=True)
