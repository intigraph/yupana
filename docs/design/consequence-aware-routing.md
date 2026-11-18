# Consequence-aware action routing

**Status: design only, nothing here is built.** When any part ships, correct
this banner in the same change. A reader who believes a capability is absent
does not look for it, and one who believes it is present builds on a hole.

This design takes three ideas from CASEL (Zhang, Shen, Yan, Zhang,
*Consequence-Aware Selective Execution for Reliable Long-Horizon Tool-Using
Agents*, arXiv 2610.36570, October 2026) and fits them onto surfaces Yupana
already has:

- [action certification](../action-certification.md) (signed ActionRecords,
  verbs `push`/`land`, `certified`/`uncertified`);
- the [golden-path guard](../golden-path-guard.md) (`warn`/`deny`, projection
  freshness on every verdict);
- the capability-scoped edit policy in `src/policy.rs`, which already sizes an
  edit's blast radius against a tenant ceiling (FR-12 used as a guard, FR-25).

It also answers a standing question from the project owner, 2026-09-03:
"maybe yupana should certify certain agent actions automatically". The answer
here is narrower and more useful: Yupana can *route* an action, saying how much
scrutiny it deserves. Certifying an action's correctness is a separate claim.

## What the paper does and does not show

CASEL routes each proposed tool call to one of six modes: **direct, verify,
sandbox, clarify, approve, abstain**. Its inputs are a calibrated failure
probability, a consequence estimate, a reversibility class, an ambiguity score
and evidence staleness. It picks the mode with the lowest assumed cost.

Every result is from a **simulation**: 20 seeds, 120,000 test actions per seed,
with intervention effectiveness assumed, not measured. The authors say it "does
not establish deployment effectiveness". The numbers below are therefore a
design argument for which signals matter, not a measured effect:

| configuration | harmful-trajectory rate |
|---|---|
| uncertainty-ranked gate (baseline) | 14.75% |
| CASEL | 7.02% |
| CASEL without reversibility-aware routing | 15.53% |
| CASEL plus a trajectory risk budget | 11.82% |
| failure-probability x consequence gate | 5.82%, but 45% more actions sent to human approval |

Two observations carry into this design:

1. **Reversibility mattered most.** Removing it made routing worse than the
   baseline. In the paper, reversibility does not scale a score. It changes
   which modes are *feasible*. Sandbox needs a reversible mutation or a dry-run
   interface. Approve applies to compensable and irreversible actions.
2. **A trajectory budget made things worse.** A full budget escalates low-risk
   actions late in a trajectory, and the budget does not cover harm from
   intervened actions at all.

## Decisions

### Adopted: a reversibility class on every routed action

Four classes, as the paper names them:

| class | meaning | examples in Yupana's vocabulary |
|---|---|---|
| `read` | changes nothing | `status`, `impact`, `query` |
| `reversible` | undone exactly by a local or unprotected operation | edit in a worktree, commit, push to an unprotected branch |
| `compensable` | cannot be undone, but a compensating action restores the intended state | `land` on a protected branch (a revert commit), an apply with a recorded rollback |
| `irreversible` | no compensating action restores the state | history rewrite, branch delete with no surviving ref, publishing a release or package, any disclosure |

The class comes from a **versioned verb table inside Yupana**, not from the
caller. A caller may narrow a class toward `irreversible`, never widen it. An
unknown verb is classed `irreversible` for routing, with the reason code
`reversibility_unknown_verb`. This is the same rule `src/mcp/tool_policy.rs`
applies to MCP tools: an unclassified tool fails its test, so a new tool cannot
ship without a declared effect. That module's `Read`/`ProcessWrite`/
`ExternalWrite` effects are the seed of the verb table; this design widens them
rather than declaring a second vocabulary.

### Adopted: consequence from the code graph, in bands

The paper estimates consequence as a frozen per-tool severity multiplied by a
bounded magnitude factor. Yupana has both halves already:

- **severity** is the verb's reversibility class plus the target's protection
  (a protected ref is more severe than a feature branch);
- **magnitude** is the blast radius that `src/policy.rs` already computes for
  edits (`BlastRadius`, symbol and file counts against the tenant's ceilings).
  For `push` and `land`, the same sizing runs over the files changed at the
  certified `sha`.

Consequence is reported as a **band**, `low`, `medium` or `high`, with
thresholds in tenant policy. It is not a calibrated probability. A band
over a graph Yupana did not fully index is `unknown`, never `low`. Index
coverage comes from the existing tier and coverage reporting, and an `unknown`
band routes as `high`.

### Adopted: staleness forces a re-read

Every guard verdict already names its projection freshness (`projected_at`,
`scope_provenance.as_of`). Routing uses it: if the evidence behind a
non-`read` action is older than the tenant's limit, the route is `verify`, and
the model-facing text names the stale input and asks for a re-read. Missing
freshness is not fresh. It routes `verify` with `freshness_unknown`.

### Adopted, narrowed: graded routes

The routing rule is a **table**, not a cost minimisation:

| reversibility | consequence | other signal | route |
|---|---|---|---|
| `read` | any | any | `direct` |
| any non-read | any | evidence stale or freshness unknown | `verify` |
| any non-read | any | no work item, or work-item scope unknown | `clarify` |
| `reversible` | `low` | none | `direct` |
| `reversible` | `medium` or `high` | none | `sandbox` (worktree or dry run) |
| `compensable` | `low` | none | `direct` |
| `compensable` | `medium` | none | `verify` |
| `compensable` | `high` | none | `approve` |
| `irreversible` | `low` or `medium` | none | `approve` |
| `irreversible` | `high`, or class from an unknown verb | none | `abstain` |

Rows are evaluated top to bottom and the first match wins, so staleness and
missing scope are handled before consequence. The table is versioned
(`routing_table: car/1`) and signed into the record, so the verifier can
recompute the route exactly as it recomputes `certification_status` today.

**Ambiguity is narrowed to what Yupana can observe.** The paper's ambiguity is
missing user intent, which Yupana cannot measure. Yupana can observe structural
absence of intent: no work item, or a work item whose scope the existing
`work_item_scope` ladder cannot resolve. Only those route `clarify`.

### Rejected: the trajectory risk budget

The paper's own ablation shows it raising harm at equal cost (11.82% against
7.02%). Its ledger also leaves out harm from intervened actions, about half of
the total. Yupana will not keep a per-session risk ledger.

### Rejected: calibrated failure probability and cost constants

Calibration needs real outcome traces. Yupana's replay corpus cannot supply
them. The 2026-08-27 re-evaluation turned 8,569 hook spool lines into only 6
replayable events (4,192 were resolver abstentions). The paper's per-mode costs
are, in its own words, "assumed constants". The table above uses neither. If a
future corpus supports calibration, that is a new design with its own
measurement, not a parameter added here.

## How a route reaches an agent

Routes are produced in the existing hook path and surface through the existing
`Outcome` (`Allow`, `Deny`, `Notify`). There is no new channel.

| route | advise mode | enforce mode |
|---|---|---|
| `direct` | `Allow` | `Allow` |
| `verify` | `Notify`: names the stale input and asks for a re-read | `Deny` until fresh evidence is supplied |
| `sandbox` | `Notify`: use a worktree or dry run | `Deny` outside a worktree or dry-run interface |
| `clarify` | `Notify`: put the question on the work item | `Notify` (never blocks) |
| `approve` | `Notify`: needs a human decision, with the reason codes | `Deny`, with a pointer for the human approval path |
| `abstain` | `Notify` | `Deny` |

**`approve` never emits the harness `ask` permission decision by default.**
Agents here often run unattended. An `ask` blocks the pane until a human
answers, which turns a routing decision into an outage. Interactive sessions
may opt in with `routing_approve_as_ask = true`. Without it, approval is
recorded and either advised or denied.

`clarify` never blocks, in either mode. It asks the agent to record the
question on the item and continue independent work, which is the operating
rule for unattended agents.

The mode is `[yupana.policy] routing = off | advise | enforce`, default `off`.
The ambient policy `Mode` is a ceiling, exactly as `work_item_scope` uses it:
`routing = enforce` under an `advise` ceiling advises.

## Schema changes

### ActionRecord v2

Routed records use `schema_version: aegis-action-certification/v2`. v2 is a
strict superset of v1. It adds, inside the signed payload:

- input: `reversibility` (an optional caller narrowing), `evidence_as_of`, and
  the consequence magnitude inputs (`blast_symbols`, `blast_files`,
  `index_coverage`);
- derived: `reversibility` (resolved), `consequence` (band), `route`,
  `route_reasons` (stable codes), and `routing_table` (`car/1`).

`certification_status` keeps its v1 meaning exactly: every check satisfied.
**A route is not a certification outcome.** A record can be `certified` with
route `approve`: the checks passed, and the consequence still warrants a human.

### Compatibility with v1

- `sign` keeps producing v1 records whenever `routing = off`. Nothing changes
  for a deployment that does not opt in.
- `verify` dispatches on `schema_version`. v1 records verify byte-for-byte as
  today. A v2 record verifies only if its recomputed reversibility, consequence
  and route match the signed values.
- Existing consumers are unchanged: `landing_admission` and `outbound_proof`
  compare `certification_status == "certified"`, and that field keeps its
  meaning. They also gain an explicit `schema_version` allowlist, so a future v3
  cannot be accepted by accident. Whether admission should also consult `route`
  is a separate, later decision. It is deliberately not made here.
- `append` identity is unchanged: `record_id` plus `signed_payload_hash`.

### New enum variants and exhaustive matching

`verb`, `result` and `certification_status` are strings on the wire today. The
new values become Rust enums (`Reversibility`, `Consequence`, `Route`) with
three rules:

1. Inside Yupana every `match` on them is exhaustive, with no wildcard arm. A
   new variant fails to compile until every consumer decides what it means.
2. On the wire they stay lowercase strings. A reader that meets an unknown
   value parses it as `Unrecognized(String)`, which routes as `abstain` and is
   reported. It is never silently treated as `direct`.
3. The routing table version is part of the signature. Changing a row is a new
   version, never an edit to `car/1`.

## Advisory versus enforcing

Everything above lands **advisory first**. Enforcement is a separate,
per-tenant step taken after a soak shows two things:

- how often each route fires over real traffic;
- how many `verify`, `sandbox` and `approve` routes a reviewer judges wrong.

Until that measurement exists, no route is presented as an authorization
decision. This is the same rule the golden-path guard applies to unsigned L5
paths.

## Rollback

| change | rollback |
|---|---|
| verb table and reversibility enum | revert the commit; nothing reads it yet |
| ActionRecord v2 | set `routing = off` (signing returns to v1); keep v2 verification so stored v2 records stay verifiable |
| consequence bands | remove the tenant thresholds; consequence reports `unknown` and routes conservatively |
| staleness routing | remove the freshness limit; no `verify` route is produced from freshness |
| hook surfacing | set `routing = off`; the hook returns to its current outcomes |
| enforcement for a tenant | lower that tenant to `advise` |

No change rewrites or deletes stored records. A rollback stops new routing and
keeps old records verifiable.

## Implementation order

Each step lands on its own, in this order:

1. Reversibility vocabulary and versioned verb table: a pure module with
   exhaustive tests, no wire change.
2. ActionRecord v2: signing, `verify` dispatch, and a fixture proving every v1
   record and every `landing_admission` / `outbound_proof` decision is
   unchanged.
3. Consequence bands from blast radius, reusing `src/policy.rs` sizing, with
   `unknown` coverage routing conservatively.
4. Staleness routing from `as_of` and `projected_at`.
5. Hook surfacing behind `routing = off | advise | enforce`, with no `ask`
   unless opted in.
6. Soak measurement: count routes in the replay corpus and report the reviewer
   disagreement rate. This is the prerequisite for any enforcement.
