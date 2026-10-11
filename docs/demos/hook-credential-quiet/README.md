# Quiet credential failures in hooks

The same private localhost probe submits a pre-Bash payload without executing
its command. On released 0.12.5 (source fd89dd6), a rejected write prints one
credential banner. The candidate changes that to zero stderr lines and records
one authentication failure event instead. No-history output stays empty and
the governed low-headroom WARN remains visible.

[Recorded output](output.log) and [timing](timing.log) form a replayable terminal
recording. Download both and run:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

Playback was verified with util-linux scriptreplay. Repeat each arm with the
corresponding executable:

```sh
python3 probe.py /path/to/yupana
```

The recording identifies both executable hashes. The candidate executable is a
private build of the runtime changes in this PR; it proves candidate behavior,
not installed rollout. The server, credential and homes are isolated fixtures.
No real service is contacted, and no build payload is executed.
