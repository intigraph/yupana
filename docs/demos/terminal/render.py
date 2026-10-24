#!/usr/bin/env python3
"""Record real local CLI output and render a self-contained animated SVG."""
from __future__ import annotations

import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
FORBIDDEN = [
    r"/(?:home|Users|root|srv|run)/",
    r"\b[\w.-]+\.(?:lan|svc|internal|local)\b",
    r"\b(?:\d{1,3}\.){3}\d{1,3}\b",
    r"\b(?:[0-9a-fA-F]{1,4}:){2,}[0-9a-fA-F:]+\b",
    r"(?i)\bbearer\s+\S+",
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
    r"(?i)(?:token|password|secret)\s*[=:]\s*\S+",
]


def check_text(text: str, *, identifiers: bool = True) -> None:
    """Refuse, never redact a recording into a claimed successful command."""
    if any(re.search(pattern, text) for pattern in FORBIDDEN):
        raise ValueError("recording refused: possible private data")
    if any(ord(char) < 32 and char not in "\n\r\t" for char in text):
        raise ValueError("recording refused: unsupported terminal control")
    # Conservatively flag work-item-like identifiers; this tool's public parser
    # tier name is the sole expected hyphenated identifier in captured output.
    item_like = re.findall(r"\b[a-z][a-z0-9]{1,15}-[a-z0-9]{2,12}(?:\.\d+)*\b", text)
    if identifiers and any(value != "tree-sitter" for value in item_like):
        raise ValueError("recording refused: possible work item identifier")


def render_svg(tape: dict, frames: list[dict], duration: float) -> str:
    width, height = tape["width"], tape["height"]
    title = html.escape(tape["title"])
    pieces = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
        f'viewBox="0 0 {width} {height}" role="img" aria-labelledby="title desc">',
        f'<title id="title">{title}</title>',
        '<desc id="desc">Real local commands show load calling parse, and run affected at hop two. '
        'The animation repeats. The adjacent transcript provides a static alternative.</desc>',
        '<rect width="100%" height="100%" rx="16" fill="#101827"/>',
        '<path d="M0 54H1000" stroke="#334155"/>',
        '<circle cx="28" cy="27" r="7" fill="#fb7185"/>',
        '<circle cx="52" cy="27" r="7" fill="#fbbf24"/>',
        '<circle cx="76" cy="27" r="7" fill="#34d399"/>',
        '<text x="110" y="34" fill="#e2e8f0" font-family="monospace" font-size="20">'
        f'{title}</text>',
    ]
    # Last frame is the static fallback when SVG animation is disabled.
    for index, frame in enumerate(frames):
        check_text("\n".join(frame["lines"]))
        last = index == len(frames) - 1
        start = frame["at"] / duration
        end = (frames[index + 1]["at"] / duration) if not last else 1
        values, keys = ["hidden"], [0.0]
        if start == 0:
            values[0] = "visible"
        else:
            values.append("visible")
            keys.append(start)
        if not last:
            values.append("hidden")
            keys.append(end)
        values.append(values[-1])
        keys.append(1.0)
        pieces.append(f'<g id="frame{index}" visibility="{"visible" if last else "hidden"}" '
                      f'font-family="DejaVu Sans Mono, Menlo, monospace" font-size="{tape["font_size"]}">')
        pieces.append(f'<animate attributeName="visibility" calcMode="discrete" '
                      f'values="{";".join(values)}" keyTimes="{";".join(f"{k:.6f}" for k in keys)}" '
                      f'dur="{duration:.3f}s" repeatCount="indefinite"/>')
        for row, line in enumerate(frame["lines"]):
            color = "#6ee7b7" if line.startswith("$ ") else "#f1f5f9"
            pieces.append(f'<text x="28" y="{94 + row * tape["line_height"]}" '
                          f'fill="{color}" xml:space="preserve">{html.escape(line)}</text>')
        pieces.append('</g>')
    pieces.append('</svg>')
    return "\n".join(pieces) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--yupana", default=shutil.which("yupana"))
    parser.add_argument("--output", type=Path, default=HERE)
    args = parser.parse_args()
    if not args.yupana:
        parser.error("provide an installed yupana executable with --yupana")
    binary = Path(args.yupana).resolve(strict=True)
    tape = json.loads((HERE / "demo.tape.json").read_text())
    check_text(tape["title"])
    frames, events, runs, screen = [], [], [], []
    # No caller environment is inherited, and no live user config is copied.
    with tempfile.TemporaryDirectory(prefix="yupana-demo-") as folder:
        root = Path(folder)
        project, home = root / "project", root / "home"
        shutil.copytree(HERE / "fixture", project)
        home.mkdir()
        env = {"PATH": os.defpath, "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"),
               "XDG_CACHE_HOME": str(home / ".cache"), "XDG_STATE_HOME": str(home / ".state"),
               "TMPDIR": str(root), "LANG": "C.UTF-8", "TERM": "dumb", "NO_COLOR": "1",
               "RUST_LOG": "off"}
        version = subprocess.check_output([str(binary), "--version"], env=env, text=True).strip()
        check_text(version)
        epoch = time.monotonic()
        for step in tape["steps"]:
            command = "$ yupana " + " ".join(step["args"])
            check_text(command)
            screen += ([""] if screen else []) + [command]
            stamp = time.monotonic() - epoch
            frames.append({"at": stamp, "lines": screen.copy()})
            events.append([stamp, "o", ("\r\n" if events else "") + command + "\r\n"])
            time.sleep(1.5)  # Deliberate reading pause, not claimed execution time.
            start = time.monotonic()
            result = subprocess.run([str(binary), *step["args"]], cwd=project, env=env,
                                    capture_output=True, text=True, timeout=10)
            elapsed = time.monotonic() - start
            check_text(result.stdout + result.stderr)
            if result.returncode or result.stderr:
                raise RuntimeError("demo command failed; no render published")
            screen += result.stdout.splitlines()
            stamp = time.monotonic() - epoch
            frames.append({"at": stamp, "lines": screen.copy()})
            events.append([stamp, "o", result.stdout.replace("\n", "\r\n")])
            runs.append({"args": step["args"], "exit": result.returncode,
                         "seconds": round(elapsed, 6), "stdout": result.stdout})
            time.sleep(step["hold"])
        duration = time.monotonic() - epoch
    if duration > 30:
        raise RuntimeError("demo exceeds 30 seconds; no render published")
    # Require the actual caller and both impact hops, not merely three exit zeros.
    if not ("load" in runs[1]["stdout"] and "hop 1" in runs[2]["stdout"]
            and "hop 2" in runs[2]["stdout"] and "run" in runs[2]["stdout"]):
        raise RuntimeError("fixture call graph was not observed; no render published")
    if max(map(len, screen)) > 65 or len(screen) > 16:
        raise RuntimeError("output would clip; no render published")
    frames[0]["at"] = events[0][0] = 0
    svg = render_svg(tape, frames, duration)
    ET.fromstring(svg)  # Refuse invalid escaping before writing the artifact.
    check_text(svg, identifiers=False)  # XML attributes are not terminal identifiers.
    transcript = "\n".join(screen) + "\n"
    cast = {"version": 2, "width": 65, "height": 16, "title": tape["title"],
            "env": {"TERM": "dumb"}}
    artifacts = {"demo.svg": svg, "demo.cast": json.dumps(cast) + "\n" +
                 "\n".join(json.dumps(event) for event in events) + "\n",
                 "transcript.txt": transcript}
    for name, body in artifacts.items():
        check_text(body, identifiers=name != "demo.svg")
    manifest = {"version": version, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
                "duration_seconds": round(duration, 3), "frames_checked": len(frames),
                "inputs_sha256": {name: hashlib.sha256((HERE / name).read_bytes()).hexdigest()
                                  for name in ("render.py", "demo.tape.json", "fixture/src/lib.rs",
                                               "fixture/.bobbin/config.toml")},
                "commands": runs, "sha256": {name: hashlib.sha256(body.encode()).hexdigest()
                                              for name, body in artifacts.items()}}
    args.output.mkdir(parents=True, exist_ok=True)
    for name, body in artifacts.items():
        (args.output / name).write_text(body)
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"frames_checked": len(frames), "duration": round(duration, 3),
                      "svg_sha256": manifest["sha256"]["demo.svg"]}))


if __name__ == "__main__":
    main()
