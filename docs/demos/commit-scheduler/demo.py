#!/usr/bin/env python3
"""Actual Git commit comparison against a finite local governed-policy fixture."""
import hashlib
import http.server
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import threading


class Policy(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if "SELECT ?s WHERE" in request["query"]:
            rows = [{"s": {"type": "uri", "value": "https://example.org/scheduler-policy"}}]
        else:
            values = {"regex": "(?m)^external-scheduler/v1:", "enforcementTier": "block",
                      "identifierClass": "commit-scheduler/v1:change-root"}
            rows = [{"property": {"type": "uri", "value": "http://aegis.gastown.local/ontology/" + k},
                     "value": {"type": "literal", "value": v}} for k, v in values.items()]
        body = json.dumps({"results": {"bindings": rows}}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)


def main():
    binary = Path(os.environ["YUPANA_DEMO_BINARY"]).resolve()
    print("Candidate binary SHA256:", hashlib.sha256(binary.read_bytes()).hexdigest())
    print("Target: disposable Git repositories; policy: finite local fixture; no host installation.")
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Policy)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    endpoint = f"http://127.0.0.1:{server.server_address[1]}"
    try:
        for candidate in (False, True):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                for args in [("init", "-q"), ("config", "user.name", "fixture"),
                             ("config", "user.email", "fixture@example.invalid")]:
                    assert git(root, *args).returncode == 0
                (root / "README.md").write_text("control\n")
                assert git(root, "add", "--", "README.md").returncode == 0
                assert git(root, "commit", "-qm", "base fixture").returncode == 0
                if candidate:
                    hook = root / ".git/hooks/commit-msg"
                    hook.write_text("#!/bin/sh\nexec " + shlex.quote(str(binary)) +
                                    " commit-check --endpoint " + shlex.quote(endpoint) +
                                    ' --message-file "$1"\n')
                    hook.chmod(0o755)
                print("\nAFTER: native commit-msg gate" if candidate else "\nBEFORE: no scheduler commit boundary")
                (root / "README.md").write_text("ordinary control\n")
                assert git(root, "add", "--", "README.md").returncode == 0
                ordinary = git(root, "commit", "-qm", "ordinary control")
                print("$ git commit -qm 'ordinary control'\nexit:", ordinary.returncode)
                assert ordinary.returncode == 0, ordinary.stderr
                (root / "new.timer").write_text("[Timer]\nOnCalendar=hourly\n")
                assert git(root, "add", "--", "new.timer").returncode == 0
                timer = git(root, "commit", "-qm", "new timer control")
                print("$ git commit -qm 'new timer control'\nexit:", timer.returncode)
                print(timer.stdout + timer.stderr, end="")
                assert (timer.returncode != 0) == candidate
                if candidate:
                    output = timer.stdout + timer.stderr
                    report, _ = json.JSONDecoder().raw_decode(output[output.index("{"):])
                    assert report["outcome"] == "refuse", report
        print("\nPASS: same timer accepted before, refused after; ordinary controls pass both.")
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == "__main__":
    main()
