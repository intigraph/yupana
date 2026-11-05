"""Boundary failures cannot turn into a browser click. Synthetic inputs only."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "guard-outbound.py"


class OutboundAdapter(unittest.TestCase):
    def invoke(self, code, body=b'{"tool_name":"fixture-click","tool_input":{}}'):
        with tempfile.TemporaryDirectory(prefix="outbound-") as root:
            helper = Path(root) / "helper"
            helper.write_text("#!" + sys.executable + "\nimport sys,time\nsys.stdin.buffer.read()\n" + code)
            helper.chmod(0o700)
            result = subprocess.run([sys.executable, str(SCRIPT), "--yupana", str(helper),
                                     "--policy", str(Path(root) / "policy"),
                                     "--receipts", str(Path(root) / "receipts")],
                                    input=body, capture_output=True, timeout=9, check=True)
            self.assertNotIn(b"PRIVATE_FIXTURE_MARKER", result.stdout + result.stderr)
            return json.loads(result.stdout)["hookSpecificOutput"]["permissionDecision"]

    def test_only_exact_allow_protocols_reach_the_fake_dispatch(self):
        clicks = 0
        for reason in ("approved", "non-send"):
            protocol = json.dumps({"schema":"yupana-outbound/v1", "decision":"allow", "reason":reason})
            if self.invoke("print(" + repr(protocol) + ")") == "allow":
                clicks += 1
        self.assertEqual(clicks, 2)

    def test_backend_failures_unknown_output_and_denial_reach_zero_clicks(self):
        denied = json.dumps({"schema":"yupana-outbound/v1", "decision":"deny", "reason":"missing-receipts"})
        cases = [
            "raise SystemExit(1)",
            "print('not JSON PRIVATE_FIXTURE_MARKER')",
            "print('{}')",
            "print('x' * 20000)",
            "time.sleep(4)",
            "print(" + repr(denied) + ")",
            "print('{\"schema\":\"yupana-outbound/v1\",\"decision\":\"allow\",\"reason\":\"unknown\"}')",
        ]
        clicks = 0
        for code in cases:
            with self.subTest(code=code):
                if self.invoke(code) == "allow":
                    clicks += 1
        self.assertEqual(clicks, 0)

    def test_stderr_and_oversized_input_are_not_relayed(self):
        self.assertEqual(self.invoke("sys.stderr.write('PRIVATE_FIXTURE_MARKER');raise SystemExit(1)"), "deny")
        self.assertEqual(self.invoke("print('{}')", b"x" * (1024 * 1024 + 1)), "deny")

    def test_missing_binary_denies_before_dispatch(self):
        result = subprocess.run([sys.executable, str(SCRIPT), "--yupana", "/nonexistent-fixture-helper",
                                 "--policy", "/nonexistent-fixture-policy", "--receipts", "/nonexistent-fixture-receipts"],
                                input=b"{}", capture_output=True, timeout=9, check=True)
        self.assertEqual(json.loads(result.stdout)["hookSpecificOutput"]["permissionDecision"], "deny")

    def test_unfinished_harness_input_has_a_bounded_denial(self):
        child = subprocess.Popen([sys.executable, str(SCRIPT), "--yupana", "/nonexistent-fixture-helper",
                                  "--policy", "/nonexistent-fixture-policy", "--receipts", "/nonexistent-fixture-receipts"],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            child.stdin.write(b"{")
            child.stdin.flush()
            child.wait(timeout=6)
            output = child.stdout.read()
            self.assertEqual(json.loads(output)["hookSpecificOutput"]["permissionDecision"], "deny")
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=1)
            for pipe in (child.stdin, child.stdout, child.stderr):
                pipe.close()


if __name__ == "__main__":
    unittest.main()
