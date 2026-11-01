#!/usr/bin/env python3
"""Render real commits to prove scrubbing preserves their release entries."""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
import unittest


ROOT = Path(__file__).resolve().parent.parent
CONFIG = Path(os.environ.get("CLIFF_CONFIG", ROOT / "cliff.toml")).resolve()


class ChangelogScrubTests(unittest.TestCase):
    def test_delimiters_children_and_entry_preservation(self):
        config = tomllib.loads(CONFIG.read_text())
        # Exercise every configured tracker prefix, without maintaining another list.
        patterns = "\n".join(
            p["pattern"] for p in config["git"]["commit_preprocessors"]
        )
        prefixes = re.search(r"\(\?:([^)]*)\)", patterns).group(1).split("|")
        cases = []
        for prefix in prefixes:
            ref = f"{prefix}-abc123"
            cases.extend([
                (f"fix: bare child {ref}.2.3", "Bare child"),
                (f"fix: public first (#42, {ref}.2)", "Public first (#42)"),
                (f"fix: bracket public [{ref}.2, #42]", "Bracket public [#42]"),
                (f"fix: bracket public first [#42, {ref}.2]", "Bracket public first [#42]"),
                (f"fix: bare reference {ref}", "Bare reference"),
                (f"fix: bracket reference [{ref}]", "Bracket reference"),
                (f"fix: bracket child [{ref}.3]", "Bracket child"),
                (f"fix: parenthesized child ({ref}.2)", "Parenthesized child"),
                (f"fix: two references [{ref}] [{prefix}-def456]", "Two references"),
                (f"fix: nested child [{ref}.2.3]", "Nested child"),
                (f"fix: retain public reference ({ref}.2, #42)", "Retain public reference (#42)"),
                (f"{ref}.2: leading child reference", "Leading child reference"),
            ])
        with tempfile.TemporaryDirectory(prefix="changelog-scrub-") as directory:
            def git(*args):
                return subprocess.check_output(
                    ["git", "-C", directory, *args], text=True
                ).strip()

            git("init", "-q")
            git("config", "user.name", "Changelog test")
            git("config", "user.email", "test@example.invalid")
            git("config", "commit.gpgsign", "false")
            git("commit", "--allow-empty", "-qm", "chore: baseline")
            git("tag", "v1.0.0")
            expected = {}
            for subject, message in cases:
                git("commit", "--allow-empty", "-qm", subject)
                expected[git("rev-parse", "HEAD")] = message
            rendered = subprocess.check_output(
                ["git-cliff", "--offline", "--config", str(CONFIG),
                 "v1.0.0..HEAD"], cwd=directory, text=True
            )

        entries = [line for line in rendered.splitlines() if line.startswith("- ")]
        self.assertEqual(len(entries), len(expected), rendered)
        for sha, message in expected.items():
            with self.subTest(message=message):
                matching = [line for line in entries if f"/commit/{sha}" in line]
                self.assertEqual(len(matching), 1, rendered)
                self.assertEqual(
                    matching[0].split("([", 1)[0].strip(), f"- {message}"
                )
        for prefix in prefixes:
            self.assertNotIn(f"{prefix}-abc123", rendered)
        self.assertNotRegex(rendered, r"(?m)^###\s+\.\d")
        self.assertIn("### Other", rendered)

    def test_tracker_only_trailer_does_not_drop_the_commit(self):
        """A body trailer holding only tracker refs must not become an empty
        footer: git-cliff rejects that as non-conventional and drops the commit
        silently (0.16.3 lost #194 this way). The window guard must also catch a
        conventional-looking commit git-cliff did not parse."""
        config = tomllib.loads(CONFIG.read_text())
        patterns = "\n".join(
            p["pattern"] for p in config["git"]["commit_preprocessors"]
        )
        prefixes = re.search(r"\(\?:([^)]*)\)", patterns).group(1).split("|")
        cases = []
        for prefix in prefixes:
            ref = f"{prefix}-abc123"
            cases.extend([
                (f"docs(readme): only trailer {prefix}", f"Work-item: {ref}",
                 f"*(readme)* Only trailer {prefix}"),
                (f"fix: two refs trailer {prefix}", f"Refs: {ref}, {prefix}-def456.2",
                 f"Two refs trailer {prefix}"),
                (f"fix: body and trailer {prefix}", f"Context mentions {ref}.\n\nWork-item: {ref}",
                 f"Body and trailer {prefix}"),
            ])
        with tempfile.TemporaryDirectory(prefix="changelog-trailer-") as directory:
            def git(*args):
                return subprocess.check_output(
                    ["git", "-C", directory, *args], text=True
                ).strip()

            git("init", "-q")
            git("config", "user.name", "Changelog test")
            git("config", "user.email", "test@example.invalid")
            git("config", "commit.gpgsign", "false")
            git("commit", "--allow-empty", "-qm", "chore: baseline")
            git("tag", "v1.0.0")
            expected = {}
            for subject, body, message in cases:
                git("commit", "--allow-empty", "-qm", subject, "-m", body)
                expected[git("rev-parse", "HEAD")] = message
            rendered = subprocess.check_output(
                ["git-cliff", "--offline", "--config", str(CONFIG),
                 "v1.0.0..HEAD"], cwd=directory, text=True
            )
            entries = [line for line in rendered.splitlines() if line.startswith("- ")]
            self.assertEqual(len(entries), len(expected), rendered)
            for sha, message in expected.items():
                with self.subTest(message=message):
                    matching = [line for line in entries if f"/commit/{sha}" in line]
                    self.assertEqual(len(matching), 1, rendered)
                    self.assertEqual(
                        matching[0].split("([", 1)[0].strip(), f"- {message}"
                    )
            for prefix in prefixes:
                self.assertNotIn(f"{prefix}-abc123", rendered)

            # The guard: with a config lacking the trailer rule, the same window
            # must FAIL rather than agree with git-cliff's shortened list.
            weakened = dict(config)
            weakened["git"] = dict(config["git"])
            weakened["git"]["commit_preprocessors"] = [
                p for p in config["git"]["commit_preprocessors"]
                if not p["pattern"].startswith("\\n")
            ]
            self.assertLess(len(weakened["git"]["commit_preprocessors"]),
                            len(config["git"]["commit_preprocessors"]))
            weak_path = Path(directory) / "weak-cliff.toml"
            weak_path.write_text(_dump_toml(weakened))
            guard = ROOT / "scripts" / "check-release-window.py"
            for cfg, want in ((CONFIG, 0), (weak_path, 1)):
                with self.subTest(config=cfg.name):
                    result = subprocess.run(
                        ["python3", str(guard), "--range", "v1.0.0..HEAD",
                         "--section-stdin", "--cliff-config", str(cfg)],
                        cwd=directory, input="", text=True, capture_output=True,
                    )
                    self.assertEqual(result.returncode, want, result.stderr)


def _dump_toml(value, prefix=""):
    """Minimal TOML writer for the cliff config shape (tables, scalars, lists
    of scalars or inline tables); enough to round-trip a weakened copy."""
    def scalar(v):
        if isinstance(v, bool):
            return "true" if v else "false"
        if isinstance(v, (int, float)):
            return str(v)
        if isinstance(v, str):
            return json.dumps(v)
        if isinstance(v, list):
            return "[" + ", ".join(scalar(x) for x in v) + "]"
        if isinstance(v, dict):
            return "{ " + ", ".join(f"{k} = {scalar(x)}" for k, x in v.items()) + " }"
        raise TypeError(type(v))
    lines, tables = [], []
    for key, val in value.items():
        if isinstance(val, dict):
            tables.append((key, val))
        else:
            lines.append(f"{key} = {scalar(val)}")
    out = "\n".join(lines) + ("\n" if lines else "")
    for key, val in tables:
        name = f"{prefix}.{key}" if prefix else key
        out += f"\n[{name}]\n" + _dump_toml(val, name)
    return out


if __name__ == "__main__":
    unittest.main()
