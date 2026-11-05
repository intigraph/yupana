#!/usr/bin/env python3
"""Fail-closed PreToolUse adapter. Never dispatches or logs message content."""
import argparse
import json
import os
import select
import selectors
import subprocess
import sys
import time

MAX_INPUT = 1024 * 1024
MAX_OUTPUT = 8192
TIMEOUT = 3
DENIALS = {
    "unclassified-action", "missing-receipts", "approval-unsatisfied",
    "content-gate-unsatisfied", "verification-unavailable", "unreadable-input",
    "unreadable-policy", "clock-unknown",
}


def bounded_input():
    body = bytearray()
    deadline = time.monotonic() + TIMEOUT
    while True:
        left = deadline - time.monotonic()
        if left <= 0 or not select.select([sys.stdin.buffer], [], [], left)[0]:
            raise TimeoutError
        chunk = os.read(sys.stdin.fileno(), min(65536, MAX_INPUT + 1 - len(body)))
        if not chunk:
            return bytes(body)
        body.extend(chunk)
        if len(body) > MAX_INPUT:
            raise ValueError


def bounded_child(command, body):
    """Bound both pipe directions, output bytes and elapsed time."""
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL)
    output = bytearray()
    offset = 0
    deadline = time.monotonic() + TIMEOUT
    try:
        with selectors.DefaultSelector() as poll:
            os.set_blocking(child.stdin.fileno(), False)
            os.set_blocking(child.stdout.fileno(), False)
            poll.register(child.stdin, selectors.EVENT_WRITE)
            poll.register(child.stdout, selectors.EVENT_READ)
            while poll.get_map():
                left = deadline - time.monotonic()
                if left <= 0:
                    raise TimeoutError
                for key, _ in poll.select(left):
                    if key.fileobj is child.stdin:
                        if offset < len(body):
                            offset += os.write(child.stdin.fileno(), body[offset:offset + 65536])
                        if offset == len(body):
                            poll.unregister(child.stdin)
                            child.stdin.close()
                    else:
                        chunk = os.read(child.stdout.fileno(), MAX_OUTPUT + 1 - len(output))
                        output.extend(chunk)
                        if len(output) > MAX_OUTPUT:
                            raise ValueError
                        if not chunk:
                            poll.unregister(child.stdout)
                            child.stdout.close()
            if child.wait(timeout=max(0.001, deadline - time.monotonic())) != 0:
                raise ValueError
        return json.loads(output)
    finally:
        if child.poll() is None:
            child.kill()
        child.wait(timeout=1)
        for pipe in (child.stdin, child.stdout):
            if not pipe.closed:
                pipe.close()


def run():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--yupana", required=True)
    parser.add_argument("--policy", required=True)
    parser.add_argument("--receipts", required=True)
    args = parser.parse_args()
    reason = "adapter-failure"
    allowed = False
    try:
        body = bounded_input()
        result = bounded_child([args.yupana, "hooks", "guard-outbound", "--policy", args.policy,
                                "--receipts", args.receipts], body)
        if not isinstance(result, dict) or set(result) != {"schema", "decision", "reason"}:
            raise ValueError
        if result["schema"] != "yupana-outbound/v1":
            raise ValueError
        if result["decision"] == "allow" and result["reason"] in {"approved", "non-send"}:
            allowed, reason = True, result["reason"]
        elif result["decision"] == "deny" and result["reason"] in DENIALS:
            reason = result["reason"]
    except Exception:
        pass
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse",
          "permissionDecision": "allow" if allowed else "deny",
          "permissionDecisionReason": "Outbound guard: " + reason}}))


if __name__ == "__main__":
    run()
