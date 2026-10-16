# Exact-target landing admission

The `landing_admission` library vocabulary evaluates evidence issued by a
canonical admission producer. It does not classify changes or implement a
second risk-tier or review-exemption ladder. Those decisions belong to the
producer and its current policy.

The predicate rides the existing command hook and graph projection. Adding it
does not enable a landing rule: a deployment must supply governed policy data
and a trusted evidence producer. An old cache without this catalogue reports
UNKNOWN; an observed empty catalogue selects nothing.

## Evidence contract

An `AdmissionPolicy` supplies a policy revision digest, a pinned Ed25519
issuer public key, an evidence lifetime and nonempty required check identifiers.
The revision must identify the complete producer/policy contract. A change to
required checks, review exemptions or producer behavior needs a new revision.
The key is supplied by governed policy, never discovered from the evidence.

Evidence uses the existing signed action-certification envelope. It must say
`verb: merge`, `result: admitted`, and `certification_status: certified`.
The record binds `remote_authority`, full `repo` slug, exact head `sha`, and
base `ref`. Its signed `scope_provenance` carries numeric `pr`, exact `base`,
`policy_revision`, `command_sha256` and nonempty `transport`. The command digest
binds the exact selected command evidence to the producer's signed operation.
Every required check must explicitly
expect and observe boolean `true`, with a nonempty evidence reference. Duplicate
check identifiers are rejected. A valid signature on a generic trace, an empty
check set or a failed admission does not satisfy this predicate.

The caller must independently resolve the attempted operation into an
`AdmissionTarget`. In particular, a head copied from the proof does not prove
that the actual merge operation uses a head compare-and-swap. Missing exact
target identity produces `Unknown`; it must not be filled from the proof.
Freshness uses a caller-supplied clock: future evidence is invalid and evidence
expires immediately after the selected lifetime. The engine caps that lifetime
at one hour.

`action_certification::verify` authenticates the record against the pinned key
and reconstructs its signed payload. It also validates the derived outcomes,
reason codes, certification status, payload hash and verdict identity. It does
not turn a signer's observations into independently verified facts.

## Integration requirements

Policy discovery selects `targets: landing-admission`, then reads each typed
Policy, Selector and Predicate by exact subject. The selector's `evidenceSource`
is JSON with `repositories`, each containing `authority`, `repository` and
`protected_refs`. The predicate's `evidenceSource` is the `AdmissionPolicy` JSON
contract, with `matchType: must-match`. The policy supplies `enforcementTier`,
`effect`, `verificationPoint: PAG` and its rationale. Warn/warn and block/deny
are the supported tier/effect pairs; the local mode remains the ceiling.

The producer supplies `landing_admission_context` and signed `landing_admission`
on the existing pre-bash hook payload. Context fields are `authority`,
`repository`, numeric `pr`, `head`, `base` and `base_ref`. The repository identity
is independently resolved from the command or its Git remote; command evidence
is hashed locally. A supplied context cannot evade scope by claiming an
unprotected ref without a valid matching signed proof. Raw merge commands with
an unstated PR base are conservatively selected by repository; count that
over-approximation during the advice soak.

Selected policy verdicts, including missing and unknown evidence, are signed
on the existing action-certification spool. The `landing-admission` check
separates these records from historical owner-policy verdicts. Failed proof
transport attribution is UNKNOWN, rather than inferred from the raw command.
Missing signing keys produce a loud NOT RECORDED notice. Unknown catalogues
and unresolved repository authorities advise unevaluated without claiming a
recorded predicate verdict or silently enabling a global block. Parser
abstentions and unobserved execution paths remain coverage limitations.

The canonical producer must issue evidence only after current preflight and
required review checks pass. An internal review exemption or a release-specific
exception is evaluated there; it does not exempt a merge from admission.
The actual operation must retain its current head/base safeguards. A dry-run
preflight is not a completed merge and must not enter the completed-merge
denominator.

Issuer custody is a separate requirement. A generic certification command can
sign caller-provided observations. Pinning its broadly available host key does
not establish that the canonical producer performed preflight. A shared Unix
identity with access to a signing key is not an isolation boundary. Deployment
must resolve the producer/key authority before treating this vocabulary as an
enforcing control.

Start with advice. Compare observed verdicts with authoritative completed
merges by repository identity, PR and exact head, separated by execution path.
Measure missing coverage and adjudicated false positives before the policy
owner promotes enforcement. Source tests or signature verification alone do
not establish that readiness.
