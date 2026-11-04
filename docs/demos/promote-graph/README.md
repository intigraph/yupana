# Explicit promotion graph

The same `promote --graph urn:fixture:ingest` command refuses on base and reaches
the selected graph in the candidate's HTTP request. The control omits the flag
and compares the complete request envelope: Turtle, snapshot, source and valid
time are unchanged after removing only the new `graph` field.

- Base source: `e927870fabae38a2236d0420173d74f57e290321`.
- Demonstrated source: `d6114f66a4b7c23bc426a1b66022d87ce1261555`.
- Target: disposable Rust repository and loopback HTTP capture, not an installed
  Quipu service. The receiver returns a synthetic success receipt; this proves
  request routing and payload identity, not server persistence or migration.
- [Recording output](terminal.output), [timing](terminal.timing),
  [reproducer](reproduce.py). These small files are retained in this repository.

Build `yupana` with the `quipu` feature at each revision, copying each binary to a
separate file before changing revisions. Then run:

```sh
python3 docs/demos/promote-graph/reproduce.py --base /path/to/base --head /path/to/head
```

The script supplies a fixture-only token to the local receiver. It does not read
the operator's token. Repository names, timestamps and graph IRIs are synthetic.
The recording includes the actual CLI responses and the observed request checks.

Download both recording files and replay with util-linux `scriptreplay`:

```sh
scriptreplay --log-out terminal.output --log-timing terminal.timing
```

Playback was checked with that command. This is a terminal recording with a
readable transcript, not an inline video. The remaining feature tests cover full,
subset and append modes, invalid graph refusal, dry-run and discovered-endpoint
write refusal. Graph selection does not activate a new production ingest lane.
