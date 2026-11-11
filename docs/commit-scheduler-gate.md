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
The committed CI range is authoritative, including for amended commits. Staged
mode compares against the current HEAD and cannot certify an amend that removes
only an exception trailer: Git's commit-msg interface supplies no reliable amend
flag. Require the exact committed range check before landing; a local hook alone
is insufficient. The CLI integration suite actually amends a commit to remove its
exception and proves the committed range refuses it.

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
command, including when the governed rule is at WARN. Advise mode permits
measured violations; it does not turn unmeasured input into a clean commit.

One case is deliberately different. When quipu cannot be reached for the rule
catalogue at all (connection failure, timeout, HTTP 408 or 5xx, or a spent
projection budget), the result is `unavailable`: the commit proceeds, the
command prints a `WARNING: commit scheduler check UNAVAILABLE` line on stderr,
and the metric records `unavailable`, never `allow`. An outage therefore cannot
block every commit, and it is still counted rather than mistaken for a pass.
This applies only to reaching the catalogue. A reachable quipu that answers with
a refusal (any other HTTP status), a malformed or empty catalogue, and an
exception that cannot be verified all still return `unknown` and fail. Bound
the wait with `YUPANA_PROJECTION_HTTP_TIMEOUT_SECS` and
`YUPANA_PROJECTION_TOTAL_BUDGET_SECS` in the hook environment.

`enforcementTier=warn` returns `advise` and permits measured additions.
`enforcementTier=block` returns `refuse` and exits nonzero. No exposure downgrade
applies. The CLI has no severity override. Confirm a bounded labelled set and
actual hook/CI controls in advise mode before changing the governed tier.

Native facts cover systemd timer directives, launchd calendar/interval keys,
Ansible cron modules, YAML schedule blocks, direct/wrapped crontab writes,
literal cron-file redirects/copies and conventional cron directories. Only a
workflow's direct `on.schedule` subtree is a CI-trigger exemption. Shell commands
inside workflow steps remain subject to admission. Every parser result passes a
common token guard. A token hit with no extracted fact returns UNKNOWN, including
read-only commands, printed strings or comments embedded in structured formats.
Whole-line shell comments remain inert.

This is a scheduling guard, not an interpreter for arbitrary generated programs
or computed command names. Unsupported source containing scheduler tokens is
reported UNKNOWN; it is not silently admitted. Non-UTF8 scheduler configuration,
opaque changed Git modes and source blobs over 4 MiB also require explicit
resolution. Ordinary documentation, JSON/JSONL, RDF NT/NQ/TTL, CSV and binary
assets outside scheduler paths are data, even when they quote scheduler tokens.
Scheduler paths take precedence over extensions and encoding: cron/crontab-prefixed
components, timer templates and unit directories cannot use the inert-data skip.
For example, `deploy/crontab.txt` is UNKNOWN while ordinary `notes.txt` is data. Unsupported executable source (for example Python constructing a cron
command) still returns UNKNOWN and refuses even at WARN.

## Data-bound migration exceptions

The terminal Git trailer is:

```text
Schedule-Exception: change-123
```

The normal deployment-selected `sd` reader must resolve exactly one item at the
same Quipu endpoint as the policy. It must have status `open` and be a direct
child of the policy's migration parent. Its description must contain exactly one
`Repository: <absolute Quipu GitRepo IRI>` line (without angle brackets).
Yupana reads the checked repository's actual `origin` remote and resolves its
host plus owner/repository through a unique governed `GitRepo` `externalUrl`
registration. HTTP/HTTPS/SSH and `.git` spellings of the same forge remote are
normalized; local paths, missing origins, missing registrations, ambiguous
registrations and non-IRI results refuse. A matching basename is insufficient.
The commit message cannot supply the repository identity. Missing repository
fields refuse, including on older exceptions. Its description must name every exact
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
decision. Reports distinguish `allow`, `advise`, `refuse`, `unknown` and
`unavailable`; count `unavailable` separately so an outage is visible.

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

## Repository-binding regression demonstration

An open direct child with description `clock.timer` and no repository field
previously admitted that path. The same focused assertion now requires refusal:

```sh
just test --locked --features quipu --lib exception_missing_repository_must_refuse
```

Observed source-control transcript (base `4002cae5`, then this repair):

```text
before: exception_missing_repository_must_refuse ... FAILED
        assertion failed: validate_exception(...).is_err()
        test result: FAILED. 0 passed; 1 failed
        exit 101
after:  exception_missing_repository_must_refuse ... ok
        exit 0
```

This is a readable transcript of a library reproducer, not an installed-fleet
claim. The CLI integration control
`actual_cli_exception_is_bound_to_derived_repo_path_and_open_lifetime` also runs
five isolated cases: matching repository/path permits; another repository,
another path, no repository field, and a closed child each refuse. Its finite
HTTP and Seeds fixtures carry no real credentials. Normal release and installed
boundary controls remain necessary before promoting the governed tier.
