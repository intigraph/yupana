# Briefing context demo

The actual briefing source reads controlled HTTP responses. Both NO CONTEXT and
UNKNOWN contribute zero semantic items; the diagnostic neighbour never appears
in the rendered briefing. A separate test retains an admitted positive and
rejects missing, malformed or below-floor similarity values.

Runtime implementation checkpoint: 465df7fda90da7cb79b5f0179b0a26f641113c11.
Only observation prints were added to the tests for this recording. The earlier
base 7b22edbe8aecd7a3ebad9898f2ed948e2d754d0e read raw ranked results
(static source evidence); this recording does not execute that old hook.

[Terminal recording / transcript](recording.txt), [timing](timing.txt).
Repeat with `bash docs/demos/search-context/run.sh`. The command enables the
`quipu` feature, selects the exact library module and refuses a zero-test result.
Replay here with:

```sh
scriptreplay --log-out recording.txt --log-timing timing.txt
```

Three feature-enabled tests passed. Earlier default-feature or wrong-filter
runs selected zero tests and are not acceptance evidence. This is a terminal
recording of source fixtures, not an installed hook or a live graph run. The
small recording remains in Git with its reproducer.
