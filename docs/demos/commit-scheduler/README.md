# Scheduler commit admission recording

Watch the same `[Timer]` / `OnCalendar=hourly` input: the baseline Git fixture
accepts the commit (exit 0); the candidate's actual commit-msg hook refuses it
(exit 1). Ordinary commits pass in both repositories. The JSON refusal names
the added unit and the governed rule.

The source base is `e927870fabae38a2236d0420173d74f57e290321`; candidate source
is `41c80b3`. The baseline demonstrates the absence of a scheduling boundary,
using normal Git without that new hook; it does not claim a fleet-wide hook
census or compare a nonexistent base `commit-check` command. The candidate uses
a finite local policy endpoint with block severity, not production credentials.
Binary SHA256:
`582b7fedef3e484a036619dc2ee47b1f41741cf48898ada91122474b98c0900b`.
This is source-fixture behavior, not installed fleet acceptance.

Download [output.log](output.log) and [timing.log](timing.log), then replay:

```sh
scriptreplay --log-out output.log --log-timing timing.log
```

These are a replayable terminal recording and readable transcript, with actual
output and timing preserved. Inline video playback is not provided. The small
recording is retained with its source under the repository's existing demo
directory. Its CRLF bytes are preserved because timing refers to those bytes.

Repeat with a Quipu-feature candidate binary:

```sh
YUPANA_DEMO_BINARY=/path/to/yupana python3 docs/demos/commit-scheduler/demo.py
```

The script uses only disposable repositories and a loopback policy fixture.
Run it under the deployment's normal test-state/HOME isolation. Live policy
measurement and actual repository hook adoption are separate checks.
