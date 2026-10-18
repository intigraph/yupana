#!/usr/bin/env python3
"""Offline before/after evidence using a synthetic committed Python repository."""
import json
import pathlib
import subprocess
import sys
import tempfile


def run(*args, cwd=None):
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


baseline = subprocess.run(["git", "grep", "-n", "-E", "HttpRoute|HttpClientCall|http.candidates", "df1fb29dbdb70b59789f83fa9cfe3c630af4af5f", "--", "src"], capture_output=True, text=True)
assert baseline.returncode == 1 and not baseline.stdout
control = subprocess.run(["git", "grep", "-n", "extract_structure\\|to_turtle", "df1fb29dbdb70b59789f83fa9cfe3c630af4af5f", "--", "src/export.rs"], capture_output=True, text=True)
assert control.returncode == 0 and control.stdout
print("BEFORE df1fb29: scoped HTTP candidate symbol probe exit1/zero; export extraction positive control present")

with tempfile.TemporaryDirectory(prefix="http-candidate-demo-") as directory:
    root = pathlib.Path(directory)
    run("git", "init", "-q", cwd=root)
    run("git", "config", "user.name", "Fixture", cwd=root)
    run("git", "config", "user.email", "fixture@example.com", cwd=root)
    source = '''from fastapi import FastAPI, APIRouter
import requests
app = FastAPI()
r = APIRouter(prefix="/api")
app.include_router(r, prefix="/v1")
@r.get("/items")
def listing():
    def inner():
        return requests.get("https://example.com/v1/api/items?token=hidden")
    return inner()
@app.post(dynamic_path)
def unresolved(): pass
'''
    (root / "api.py").write_text(source)
    run("git", "add", "api.py", cwd=root)
    run("git", "commit", "-qm", "synthetic fixture", cwd=root)
    snapshot = json.loads(run(sys.argv[1], str(root), "fixture", "HEAD"))
    print("AFTER: committed static evidence; no runtime dependency claim")
    for candidate in snapshot["files"][0]["candidates"]:
        caller = candidate["caller"]
        print(json.dumps({"role": candidate["role"], "method": candidate["method"],
                          "path": candidate["path"], "resolution": candidate["resolution"],
                          "caller": caller["name"] if caller else None,
                          "reasons": candidate["unresolved_reasons"]}, sort_keys=True))
    print("coverage:", json.dumps(snapshot["coverage"], sort_keys=True))
    assert "token=" not in json.dumps(snapshot)
    assert snapshot["coverage"]["literal_records"] == 2
    assert snapshot["coverage"]["unresolved_records"] == 1
    (root / "api.py").write_text("# uncommitted replacement\n")
    second = json.loads(run(sys.argv[1], str(root), "fixture", "HEAD"))
    assert second["files"] == snapshot["files"]
    print("Uncommitted replacement excluded: committed source hash and records unchanged")
