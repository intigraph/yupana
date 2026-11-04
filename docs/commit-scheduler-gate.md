# Governed commit scheduler admission

`yupana commit-check` adds an explicit commit boundary. It requires the `quipu`
feature. This is separate from the pre-edit text plane: it reads Git objects and
the commit message, and applies equally to public and internal repositories.

At `commit-msg`, pass Git's message file:

```sh
yupana commit-check --endpoint "$QUIPU_SERVER" --message-file "$1"
```

For a CI range, pass exact commits. The target commit's actual message supplies
the trailer:

```sh
yupana commit-check --endpoint "$QUIPU_SERVER" --base "$BASE_SHA" --to "$HEAD_SHA"
```

Staged mode requires an existing HEAD. It snapshots the index with `git
write-tree`, reads immutable blobs and compares them to HEAD. Unstaged edits
cannot conceal staged definitions. Neither mode changes files or refs. Renames
are additions at their new paths. Removed files and unchanged definitions pass.
New duplicate entries count as additions. Timer reset directives and individual
launchd calendar-array entries participate in shrink-only comparison.

## Governed policy

Create a governed `TextRule` through the deployment's normal attributed graph
writer. Its `identifierClass` is `commit-scheduler/v1:<migration-parent-id>`.
The suffix binds the exception parent; it is policy data, not a caller flag.
The regex receives canonical facts beginning with `external-scheduler/v1:`.
Use an anchored regex covering every admitted family, and record both matching
and nonmatching examples. Missing policy, invalid regex, uncovered facts,
unreadable Git data and malformed configuration return `unknown` and fail the
command. UNKNOWN never means a clean commit.

`enforcementTier=warn` returns `advise` and permits measured additions.
`enforcementTier=block` returns `refuse` and exits nonzero. No exposure downgrade
applies. The CLI has no severity override. Confirm a bounded labelled set and
actual hook/CI controls in advise mode before changing the governed tier.

Native facts cover systemd timer directives, launchd calendar/interval keys,
Ansible cron modules, YAML schedule blocks, direct/wrapped crontab writes,
literal cron-file redirects/copies and conventional cron directories. Only a
workflow's direct `on.schedule` subtree is a CI-trigger exemption. Shell commands
inside workflow steps remain subject to admission. Comments, ordinary printed
shell strings and quoted heredocs are not executed commands.

This is a scheduling guard, not an interpreter for arbitrary generated programs
or computed command names. Unsupported source containing scheduler tokens is
reported UNKNOWN; it is not silently admitted. Non-UTF8 scheduler configuration,
opaque changed Git modes and source blobs over 4 MiB also require explicit
resolution. Ordinary documentation and binary assets are not scheduler source.

## Data-bound migration exceptions

The terminal Git trailer is:

```text
Schedule-Exception: change-123
```

The normal deployment-selected `sd` reader must resolve exactly one item at the
same Quipu endpoint as the policy. It must have status `open` and be a direct
child of the policy's migration parent. Its description must name every exact
`exception_target` returned in the report, each on a separate line (backticks
are allowed). Timer/plist targets are exact unit paths; command/config targets
include their path and complete extracted definition. Substring matches do not
count. Duplicate trailers, the obsolete `Scheduler-Exception` spelling, closed
items, wrong parents, missing descriptions, failed reads and a 10-second reader
timeout refuse. No signing key or enrollment is involved.

A trailer is not an approval by itself. Keep the migration child open only for
the intended scope, then close it to revoke the allowance. The command performs
one bounded read; it neither creates an item nor retries it.

## Counting, adoption and rollback

Each invocation emits a normal `commit-schedule` metric-spool event containing
`result`, addition count and evaluated rule ids. A metrics write cannot alter the
decision. Reports distinguish `allow`, `advise`, `refuse` and `unknown`.

This source does not install Git hooks or alter existing hook chains. Deploy the
reviewed binary and compose the command through the repository's existing
commit-msg/CI owner. Preserve existing hooks and verify both a clean commit and
a refused timer commit at the actual boundary. A CI endpoint must be explicitly
reachable and governed; never substitute a fixture endpoint in production.
Rollback disarms only this newly registered boundary through its owner, retains
the measured events and existing hooks, and restores the prior governed tier.

`tests/commit_check_cli.rs` is armed by the existing Quipu-feature CI test matrix.
It uses a finite local policy fixture and an actual Git commit-msg hook. It proves
candidate refusal and counting, not installed fleet coverage. Live-policy advise
measurement and host/repository adoption require separate read-back.
