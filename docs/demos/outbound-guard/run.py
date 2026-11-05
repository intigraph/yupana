#!/usr/bin/env python3
"""Synthetic refusal/draft demo. Never invokes a connector or issues approvals."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory() as fixture:
    root = Path(fixture)
    policy = {"revision": "a" * 64, "approval_public_key": "b" * 64,
              "gate_public_key": "c" * 64, "max_age_secs": 60,
              "non_send_tools": ["fixture_create_draft"],
              "send_adapters": [{"tool_name": "fixture_send", "text_pointer": "/body",
                  "recipient_pointer": "/to", "thread_pointer": "/thread",
                  "thread_required": False, "literal_fields": ["subject"]}]}
    (root / "policy.json").write_text(json.dumps(policy))
    (root / "receipts").mkdir()
    cases = [
        ("unapproved browser click", {"tool_name": "fixture_browser", "tool_input":
            {"action": "click"}}, "deny"),
        ("literal send without receipts", {"tool_name": "fixture_send", "tool_input":
            {"body": "Synthetic text", "to": "fixture@example.invalid"}}, "deny"),
        ("mutable attachment reference", {"tool_name": "fixture_send", "tool_input":
            {"body": "Synthetic text", "to": "fixture@example.invalid",
             "attachments": ["same-reference"]}}, "deny"),
        ("draft creation", {"tool_name": "fixture_create_draft", "tool_input":
            {"body": "Synthetic text"}}, "allow"),
    ]
    for label, payload, expected in cases:
        reply = subprocess.run([sys.executable, "scripts/guard-outbound.py", "--yupana",
            sys.argv[1], "--policy", str(root / "policy.json"), "--receipts",
            str(root / "receipts")], input=json.dumps(payload), text=True,
            capture_output=True, check=True)
        output = json.loads(reply.stdout)["hookSpecificOutput"]
        print(label + ": " + output["permissionDecision"] + " / " +
              output["permissionDecisionReason"], flush=True)
        assert output["permissionDecision"] == expected
