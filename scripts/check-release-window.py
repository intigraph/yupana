#!/usr/bin/env python3
"""Require explicit coverage for nonconventional commits in a release window.

Merge containers are excluded; their constituent commits are checked. Existing
history is immutable, so a linked entry in the newest section repairs a bare
subject. The generator must preserve that entry as well.

With --cliff-config, also reconcile the raw window against what git-cliff
actually parsed: a commit whose SUBJECT looks conventional but which git-cliff
rejected (it drops parse failures silently under filter_unconventional) fails
the check. Without this, generator and verifier agree on a list that is
missing the commit, and the guard stays green (yupana #194 in 0.16.3).
"""
import json

import argparse
import re
import subprocess
import sys
from pathlib import Path


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--range")
    parser.add_argument("--section-stdin", action="store_true")
    parser.add_argument("--cliff-config",
                        help="also fail on conventional-looking commits git-cliff did not parse")
    args = parser.parse_args()
    if git("rev-parse", "--is-shallow-repository") != "false":
        raise ValueError("full history is required to check the release window")
    window = args.range
    if not window:
        # Match the package's tags, not an unrelated crate or historical v-tag.
        tags = git("tag", "--list", "quipu-ai-v[0-9]*", "--sort=-v:refname").splitlines()
        previous = next((tag for tag in tags if subprocess.run(
            ["git", "merge-base", "--is-ancestor", tag, "HEAD"], check=False
        ).returncode == 0), None)
        if previous is None:
            raise ValueError("no package-qualified ancestor release tag")
        window = f"{previous}..HEAD"
    if args.section_stdin:
        section = sys.stdin.read()
    else:
        sections = re.split(r"(?m)^## \[", Path("CHANGELOG.md").read_text())
        if len(sections) < 2:
            raise ValueError("no changelog section")
        section = sections[1]
    documented = set(re.findall(r"\[([0-9a-f]{7})\]", section))
    prefix = re.compile(r"^[a-zA-Z]+(?:\([^\r\n()]+\))?!?: .+")
    bare = []
    conventional = []
    for row in git("log", "--no-merges", "--format=%H%x09%s", window).splitlines():
        sha, subject = row.split("\t", 1)
        if not prefix.match(subject):
            bare.append((sha[:7], subject))
        else:
            conventional.append((sha, subject))
    if args.cliff_config:
        context = json.loads(subprocess.check_output(
            ["git-cliff", "--offline", "--config", args.cliff_config,
             "--context", window], text=True, stderr=subprocess.DEVNULL))
        parsed = {commit["id"] for release in context for commit in release["commits"]}
        unparsed = [(sha[:7], subject) for sha, subject in conventional if sha not in parsed]
        if unparsed:
            print(f"FAIL — git-cliff could not parse conventional-looking commits in {window}"
                  " (run git-cliff -vv to see why):", file=sys.stderr)
            for sha, subject in unparsed:
                print(f"  {sha} {subject}", file=sys.stderr)
            return 1
    missing = [(sha, subject) for sha, subject in bare if sha not in documented]
    if missing:
        print(f"FAIL — nonconventional commits omitted from release window {window}:", file=sys.stderr)
        for sha, subject in missing:
            print(f"  {sha} {subject}", file=sys.stderr)
        return 1
    print(f"release-window: {window}; {len(bare)} bare subjects explicitly documented", file=sys.stderr)
    for sha, _ in bare:
        print(sha)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"ERROR: release window could not be checked: {error}", file=sys.stderr)
        sys.exit(2)
