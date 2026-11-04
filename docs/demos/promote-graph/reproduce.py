#!/usr/bin/env python3
"""Compare two explicit local binaries against a disposable HTTP capture."""
import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--head", type=Path, required=True)
    args = parser.parse_args()
    bodies = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            assert self.path == "/knot"
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            bodies.append(body)
            reply = json.dumps({"count": 1, "tx_id": len(bodies), "conforms": True,
                                "valid_from": body["valid_from"]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(reply)))
            self.end_headers()
            self.wfile.write(reply)

        def log_message(self, *_):
            pass

    server = HTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "lib.rs").write_text("pub fn entry() {}\n")
            for command in [["init", "-q"], ["add", "lib.rs"],
                            ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org",
                             "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]]:
                subprocess.run(["git", *command], cwd=root, check=True,
                               env={**os.environ, "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z",
                                    "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"})
            config = root / "config.toml"
            config.write_text("[yupana.quipu]\nenabled=false\n[yupana.serve]\nuse_daemon=false\n")
            token = root / "fixture-token"
            token.write_text("fixture-only-not-a-credential")
            env = {**os.environ, "QUIPU_AUTH_TOKEN_FILE": str(token)}
            env.pop("QUIPU_AUTH_TOKEN", None)
            command = ["promote", "--repo", "fixture", "--replace-snapshot", "--to",
                       f"http://127.0.0.1:{server.server_port}", "--config", str(config), str(root)]

            def run(binary, graph=True):
                return subprocess.run([str(binary.resolve()), *command,
                                       *(["--graph", "urn:fixture:ingest"] if graph else [])],
                                      env=env, capture_output=True, text=True, timeout=30)

            print("Same input: promote --repo fixture --replace-snapshot --to <loopback> --graph urn:fixture:ingest", flush=True)
            before = run(args.base)
            print("BASE: " + before.stderr.strip(), flush=True)
            assert before.returncode != 0 and not bodies
            print(f"BASE exit={before.returncode}; captured writes=0", flush=True)
            after = run(args.head)
            print("HEAD: " + after.stdout.strip(), flush=True)
            assert after.returncode == 0, after.stderr
            assert len(bodies) == 1 and bodies[0]["graph"] == "urn:fixture:ingest"
            print("HEAD captured graph=urn:fixture:ingest; replace_snapshot=true", flush=True)
            control = run(args.head, graph=False)
            assert control.returncode == 0 and len(bodies) == 2, control.stderr
            selected = dict(bodies[0])
            selected.pop("graph")
            assert selected == bodies[1]
            print("Control: omit --graph -> ROOT; exact Turtle/snapshot/source/valid-time envelope unchanged", flush=True)
            print("PASS: selected graph reaches HTTP; no entity identity changes; no production requests", flush=True)
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=5)


if __name__ == "__main__":
    main()
