# Repository-owned command advice

This is a transcript of a private synthetic library fixture, not an installed
hook test or a demonstration of live exploitation. No graph write or push hook
was invoked. The transcript is intentionally small; it is not a video.

Before, source `src/ci_shift.rs` at the v0.18.0 tag passed graph command text to
`sh -c`. This is a source-inspection finding; no before exploit was executed.
After, the same legacy executor API refuses a synthetic `touch` command; the
background compatibility entrypoint publishes no success, and the fixture's
marker and state directory remain absent. The separate planner produces advice
for a reviewed static check, keeps a complete nonmatch quiet, and rejects the
same path in another repository, graph-supplied command text, changed manifest
bytes, source mismatch and budget overflow.

[Actual focused-run transcript](output.txt). Reproduce the executor control:

```sh
just test --features quipu --lib graph_commands_cannot_start_children_or_publish_success
```

The full focused run also selects `ci_authority` tests. The fixture contains no
production identities or secrets. Test definitions name each positive/negative
control. The graph-map authority is not activated by this source change; no
child executor or manifest enrollment adapter is introduced. Installed hash
verification and a post-install negative control remain required separately.
