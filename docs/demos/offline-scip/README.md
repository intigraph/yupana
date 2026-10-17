# Offline SCIP navigation demonstration

The same synthetic index/binding input is sent to the installed baseline binary
and the candidate checkout binary. Before: `import-scip` is unrecognized (exit 2).
After: one covered document imports with two distinct `duplicate` identities,
each with exactly its own definition and reference. Identifier byte ranges and
AST body line ranges remain separate. Editing the source makes the next import
refuse with exit 1 and zero facts.

The baseline is the installed 0.14.0 artifact; its SHA256 is recorded in the
[transcript](output.log). The source baseline is `df1fb29dbdb70b59789f83fa9cfe3c630af4af5f`;
no claim of a locally rebuilt baseline binary is made. The candidate recording
was captured from the source changes in this commit; the binary digest is also
recorded. This is synthetic checkout acceptance, not installed activation.

Repeat after `just build`, pointing the variables at the two binaries:

```bash
YUPANA_SCIP_BASE=/path/to/baseline/yupana \
YUPANA_SCIP_HEAD=/path/to/candidate/yupana \
python3 docs/demos/offline-scip/run.py
```

[Terminal output](output.log) and [timing](timing.log) are a replayable util-linux
recording, retained here with the source. Download both and run:

```bash
scriptreplay --log-out output.log --log-timing timing.log
```

No inline video player is promised. The raw output and metadata were inspected
for private paths/credentials. The checked-in compiler index is synthetic; no
indexer, network request or live graph mutation runs during this demo.
