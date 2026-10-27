# Terminal demo asset

This asset shows one question: **what would changing `parse` affect?**
The local Rust fixture has `load` calling `parse` and `run` calling `load`.
The recording shows the direct caller, then the otherwise invisible second hop.
It does not require a server, network access, a model, or a Cargo build.

- Animation: [terminal/demo.svg](terminal/demo.svg)
- Recording tape: [terminal/demo.tape.json](terminal/demo.tape.json)
- Actual terminal output and timing: [terminal/demo.cast](terminal/demo.cast)
- Static alternative: [terminal/transcript.txt](terminal/transcript.txt)
- Tool version, input hashes, execution times and render hashes:
  [terminal/manifest.json](terminal/manifest.json)

The self-contained SVG loops in about 19 seconds. The terminal uses 24 px text
on an opaque dark background, so it has the same contrast on a light or dark
page. Its final frame is also the fallback for viewers without SVG animation.
This is an asset handoff; the repository README is deliberately unchanged.

## Re-record

Requirements: Python 3.11+ and an installed `yupana` executable. The checked-in
recording used Yupana 0.15.4; the exact executable digest is in the manifest.
No recording package or browser is needed to render it.

```sh
python3 docs/demos/terminal/render.py --yupana "$(command -v yupana)"
sha256sum docs/demos/terminal/demo.svg
```

The renderer copies only the checked-in fixture into a temporary directory and
uses an empty private HOME and an allowlisted environment. LSP, CPG and Quipu
are disabled in the fixture. It records actual command responses and measured
execution times. The tape adds explicit reading pauses; these are not reported
as command latency. A failed command, unexpected stderr, missing graph result,
clipped output or a recording longer than 30 seconds refuses publication.
There is no fallback success transcript.

Every complete displayed frame, command output and raw recording is checked
for private paths, infrastructure names, addresses, credentials and possible
work-item identifiers before any output file is written. Suspected data refuses
the recording instead of being redacted. Run a secret scanner as a separate
check and inspect the animation before publishing it:

```sh
gitleaks dir docs/demos/terminal --no-banner
```

Open `terminal/demo.svg` in a browser to check all six states and a full loop,
including embedding it on light and dark page backgrounds. The SVG has no
scripts, external fonts, remote resources or links. GitHub-hosted playback has
not been claimed merely from local browser playback; the README owner should
verify the final embed after integration.

Re-recording with the same tool produces the same command output. Actual
execution timings can vary, so the cast and SVG hashes can change. Re-check the
new render's hash and every frame; do not copy an older manifest onto it.
