# HTTP refusal diagnostics

This offline fixture runs the installed baseline and the candidate CLI against
one loopback HTTP 400 guard refusal. Each CLI makes one request and exits with a
failure. The candidate preserves the reason and redacts the reflected fixture
bearer. No production endpoint or credentials are used.

Set `YUPANA_BEFORE` and `YUPANA_AFTER` to the corresponding executable paths,
then run `python3 docs/demos/promote-http-error/demo.py` from this checkout.
Replay [the recording](output.log) with [its timing](timing.log) using
`scriptreplay --log-out output.log --log-timing timing.log`.

The baseline is installed Yupana 0.19.1. The candidate is the guarded build of
this source change; the recording does not claim a production rollout.
