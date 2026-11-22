"""Exercise installed baseline and candidate CLIs against a loopback refusal fixture."""
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading


class Refusal(http.server.BaseHTTPRequestHandler):
    requests = 0

    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        type(self).requests += 1
        body = json.dumps({"error": "write guard refused: owner grant missing; "
                           "Bearer fixture-only-not-a-credential"}).encode()
        self.send_response(400)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    (root / "lib.rs").write_text("pub fn entry() {}\n")
    (root / "config.toml").write_text(
        "[yupana.quipu]\nenabled=false\n[yupana.serve]\nuse_daemon=false\n")
    for args in (["init", "-q"], ["add", "lib.rs"],
                 ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.org",
                  "commit", "-qm", "fixture"]):
        subprocess.run(["git", *args], cwd=root, check=True)
    server = http.server.HTTPServer(("127.0.0.1", 0), Refusal)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for label, executable in (("before", os.environ["YUPANA_BEFORE"]),
                                  ("after", os.environ["YUPANA_AFTER"])):
            start = Refusal.requests
            env = dict(os.environ, HOME=str(root), XDG_STATE_HOME=str(root / "state"),
                       QUIPU_AUTH_TOKEN="fixture-only-not-a-credential")
            env.pop("QUIPU_AUTH_TOKEN_FILE", None)
            result = subprocess.run(
                [executable, "promote", "--replace-snapshot", "--repo", "fixture", "--to",
                 f"http://127.0.0.1:{server.server_port}", "--config",
                 str(root / "config.toml"), str(root)],
                env=env, capture_output=True, text=True, timeout=30)
            diagnostic = next((line for line in result.stderr.splitlines()
                               if "POST " in line and "status 400" in line), None)
            if diagnostic is None:
                raise RuntimeError(result.stderr.replace("fixture-only-not-a-credential", "[REDACTED]"))
            assert result.returncode != 0 and Refusal.requests - start == 1
            assert "fixture-only-not-a-credential" not in diagnostic
            assert ("owner grant missing" in diagnostic) == (label == "after")
            print(f"{label}: one request, refused (exit {result.returncode})")
            print(diagnostic)
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
