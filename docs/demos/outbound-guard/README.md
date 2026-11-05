# Outbound guard fixture demo

Run from the repository root after `just build --features quipu`:

```sh
python3 docs/demos/outbound-guard/run.py /path/to/built/yupana
```

This synthetic source-checkout demo calls the actual wrapper and compiled CLI.
It displays refusal of an unapproved click, a literal send without receipts,
and an attachment reference, plus allowed draft creation. No connector is called.
The base has no outbound verifier command; this is a new boundary, not an
installed before/after measurement. Signed allow and changed-text refusal are
covered separately by `just test --test outbound_guard --features quipu`.

The recording uses terminal output and timing files for `scriptreplay`, with a
readable transcript fallback. It does not establish protected issuer custody or
installed harness coverage. Fixtures use only invented text and public key-shaped
bytes; no credentials, signatures or private approval contents are recorded.

[Terminal output](output.log) and [timing](timing.log) are retained in this
repository's demo directory. Download both and replay with:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

The recording ran against base `a34038af24970c07ad9f9136e4cab9881112f9d6`
plus this PR's source candidate, with compiled binary SHA-256
`fdedb16e26875d9e54c230df5d363284b0cd6a8c8e67cf12bb57891b7eb5b845`.
The binary was copied out of its build target before use. The guard reported
158 watched paths and two UID resources unchanged. This terminal recording
does not promise browser embedding.
