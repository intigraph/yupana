# One disk/class subject across command runs

The recorded control runs four real pre-Bash hook processes against an isolated
localhost HTTP fixture. Three aggregate writes reuse one unique subject, and
any legacy `/knot` write fails the control. The credential, session provenance
and quiet-refusal cases run separately in the same integration target.

Baseline producer behavior creates a subject per run; the baseline executable
was not replayed. Candidate source is commit `3397b38ae3f9a06d5a024d8dc418a214c9b5150c`. The qualified
binary was built from these same source files before the commit metadata was
created; this is a source fixture, not an installed release.

[Terminal output](session.log) and [timing](session.time) can be replayed with:

```sh
scriptreplay --log-out session.log --log-timing session.time
```

Playback was verified with util-linux scriptreplay. To repeat the actual control:

```sh
just test --features quipu --test quipu_auth_session \
  aggregate_hook_reuses_one_subject_across_real_processes -- --exact --nocapture
```

Observed output reports `writes: 3`, `unique_subjects: 1` and
`per_run_knot_writes: 0`. This proves the hook's wire protocol and identity
reuse. The fixture does not execute Quipu persistence or prove server-side
concurrency, deployment, or historical-node migration.

Qualified test executable SHA-256: `774fc23dd8aed0af484b74a9b06d87946b5dbee5200e7e07f05a82e1363ef66d`.
Qualified child executable SHA-256: `034260fe5981fe435a1920478d4c58f0694cecf251a0b67fdd93fa0f16825923`.
