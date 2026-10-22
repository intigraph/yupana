# Shared label ownership demo

The same nested code/document fixture is exported by an installed Yupana 0.15.0
binary and the candidate built from clean source `6b8f2e8`. Basename labels and
the Section leaf label/heading disappear; the symbol label, IRIs and structural
lines remain identical. This is fixture evidence, not installed acceptance.
The installed baseline is identified by version and binary hash in the recording;
its source SHA is not independently attested. Separately, both regression tests
fail on source base `4574278a` and pass on the candidate.

Download `output.log.gz` and `timing.log`, then replay with:

```sh
gzip -dc output.log.gz > output.log
scriptreplay --log-out output.log --log-timing timing.log
```

No inline playback is claimed. `transcript.txt` is the readable text fallback. The compressed log preserves
the recorder bytes and timing unchanged.
The small recording and its reproducer are retained in Git with this change.

To reproduce using your own before/after binaries:

```sh
DEMO_BEFORE=/path/to/before/yupana DEMO_AFTER=/path/to/after/yupana python3 run.py
```

The script invokes both real CLIs against one temporary fixture and compares
all non-presentation lines. It performs no network requests or promotion.
Receiver SHACL enforcement and stored-label cleanup are separate acceptance.
