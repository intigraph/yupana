Native patch reconstruction refusal
===================================

The same isolated Add-then-Update patch contains repeated workflow context.
Watch BASE return an advisory and CANDIDATE return an explicit deny. Both hooks
exit zero; neither hook writes files. The native editing tool is not invoked.

Base: `cd3d027f256250239e02e639310382b34532b354`.
Candidate implementation: `d6abadc9bc2239960d9429b2449cc6c01ac32d5a`.
Both binaries were built from those source trees with default Cargo features.
Subsequent recording-only changes do not change the demonstrated implementation.
This demonstrates source candidate behavior, not installation or a live tool edit.

Readable [transcript](transcript.txt), replayable [output](output.log.gz), and
[timing](timing.log) are retained with this repository. Download all three.
Replay requires util-linux scriptreplay; inline playback is not claimed.

```sh
gzip -dc output.log.gz > /tmp/native-patch-demo.log
scriptreplay --log-out /tmp/native-patch-demo.log --log-timing timing.log
```

Repeat using your builds of the two revisions:

```sh
python3 scripts/demo-native-patch-guard.py /path/to/base/yupana /path/to/candidate/yupana
```

The recording uses a disposable local fixture with graph projection disabled.
The actual retained incident was reproduced separately; this public fixture
removes private paths while retaining its repeated-context failure.
