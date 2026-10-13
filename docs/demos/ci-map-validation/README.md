# CI-map integrity demo

The same malformed scope and partial projection returned `Quiet` and `Ok` at
base `252d1aeb1db0d0ecfffe87a92eccb9b95a6a8f55`. At candidate
`fd63fbd350c5ec5baaca8cfe1147a0aa75085d7a`, they return `Unknown` and an error.
The valid matching scope and unrelated-path quiet controls are preserved.

This is a library fixture importing the exact mapper source at each revision,
with a small error-type adapter. It does not execute mapped commands or demonstrate
an installed hook, repository isolation, or a reduction in hosted CI failures.

Repeat from the repository root with Cargo and its cached dependencies available:

```sh
python3 docs/demos/ci-map-validation/reproduce.py \
  252d1aeb1db0d0ecfffe87a92eccb9b95a6a8f55 \
  fd63fbd350c5ec5baaca8cfe1147a0aa75085d7a
```

[Output](output.log) and [timing](timing.log) form a replayable terminal recording,
not an inline video. Download both and replay with util-linux:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The small sanitized text recording is retained here with its source reproducer.
Later documentation-only commits preserve the demonstrated mapper blobs.
