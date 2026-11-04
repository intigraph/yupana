# CI source-cache demonstration

The base caches Cargo downloads plus compiled `target/` and permits a broad
cross-feature fallback. The candidate caches only registry/git downloads and
uses a new key namespace whose fallback cannot select the old target archives.

Base: `ba1811278903e7f3c5d3f23c993027588deb2a22`. Candidate: this workflow change.
The script parses both actual YAML definitions, checks that every non-cache job
and feature matrix is unchanged, and runs a small prefix-selection fixture.
It does not restore a GitHub archive or reproduce a runner filesystem limit.

Run `just --command uv run --no-project --with pyyaml python3
docs/demos/ci-source-cache/demo.py` as one command from the checkout.
[Actual output](output.txt) and [timing](timing.txt) can be replayed with
`scriptreplay --log-out output.txt --log-timing timing.txt`.

The observed failing job ran out of space during cache restoration before its
test command started. A cache inventory contained a 5,242,390,658-byte archive;
its expanded size and the job's selected archive were not available. This is a
candidate mitigation, with cold hosted execution still required for acceptance.
No test arm, test command or gate is removed, and no existing cache is deleted.
