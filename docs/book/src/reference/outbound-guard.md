# Outbound approval guard

The outbound adapter refuses a selected human-facing send unless a protected
producer supplies fresh, signed direct-user approval and a domain content-gate
verdict of **send** for the exact attempted message. It does not send anything.
Unknown actions refuse; a generic successful certification is insufficient.

This is separate from the existing fail-open edit and command hooks. Run the
outer adapter as a harness **PreToolUse** hook, with a timeout of at least ten
seconds:

```sh
python3 /managed/scripts/guard-outbound.py \
  --yupana /managed/bin/yupana \
  --policy /protected/outbound-policy.json \
  --receipts /protected/outbound-receipts
```

The wrapper invokes `yupana hooks guard-outbound`. The plural `hooks` command
does not use the singular hook command's parse-error fail-open path. Missing,
old, failing, malformed or silent backends produce a denial. Input acquisition
and backend execution each have a three-second deadline. Input is bounded at
one MiB; backend output is bounded at eight KiB. Only fixed decision codes reach
the harness; message text, recipients, parser errors and backend stderr do not.
The wrapper has Python's standard-library dependency and requires a POSIX host.

## Protected producer and configuration

The policy pins a revision digest, separate approval and gate issuer public
keys, a maximum age of one to 3,600 seconds, exact non-send tool names and
explicit send adapters. Adapter JSON pointers select message text, recipient
and thread from the actual tool input. Replies/forwards can require a thread;
new-message adapters bind an absent thread explicitly. Match tool names exactly.
Validate each mapping against its actual connector contract before activation.

Do not put a mutable tool schema or issuer key in tool input. The policy,
receipt store, hook configuration and wrapper must be controlled outside the
agent's write authority. A broadly available host signing key, an environment
variable or another agent repeating “the user approved” does not establish human
approval. A shared Unix identity is not that isolation boundary. Existing
generic certification commands are not an approval issuer.

The protected producer must show the user the complete operation, including
all recipients, thread, subject, formatting and attachments, and attest a direct
user-chat approval of those exact inputs. A generic instruction to send unseen
drafts is insufficient. The content-gate producer independently checks the same
operation. Do not mint production keys or infer an issuer from a received proof.

Both receipts use the existing signed action-certification envelope. The
approval has `verb: approve-outbound`, `result: approved`, and a satisfied
`human-approval` check whose expected and observed values are `direct-user-chat`.
The gate has `verb: content-gate`, `result: send`, and a satisfied `content-gate`
check whose expected and observed values are `send`. Evidence references are
SHA-256 digests, not private content. Both have
`target_class: outbound-human-message`, `sha` equal to the exact text SHA-256,
and `target` equal to the recipient/thread fingerprint. Signed scope provenance
binds `policy_revision` and `action_sha256`. Future or expired evidence refuses.

`action_sha256` covers the entire actual tool name and input, with recursively
sorted object keys and unchanged array order and string values. The text hash
uses UTF-8 bytes without trimming or Unicode/newline normalization. Adapters accept only top-level text, recipient and thread fields plus an
explicit subset of literal `subject`, `cc` and `bcc` fields. Undeclared fields,
attachments, external drafts, file references and nested schemas refuse before
receipt verification. CC and subject changes invalidate approval even when body
text is unchanged. Hashing a reference cannot freeze its backing bytes; these
references are unsupported rather than treated as approved content. The store entry is `<action_sha256>.json` with
`approval` and `content_gate` records; moving old evidence under a new lookup
name cannot repair its signature binding. Signature verification requires a
build with the existing `quipu` feature; other builds refuse sends.

## Browser and coverage limits

A raw coordinate click or keypress does not contain the composed message.
Never populate its supposed current text/hash from the approval receipt: that
would compare the proof to itself. Such browser actions remain refused until
a protected browser adapter can independently establish current composed text,
destination and exact operation. Screenshot/read-only or draft tools may be
explicitly classified as non-send only after their actual contracts are reviewed.
An unknown action must be classified by the operator before retrying; a blind
permission dialog is not a substitute for exact-text approval.

The generic shipped hook bundle is unchanged. Enabling this blocking hook
requires a reviewed tool matcher/catalogue, protected issuer custody and normal
managed installation. A matcher omitting a connector or script-based escape
does not guard that path. CLI fixtures do not prove installed harness coverage,
DOM freshness, one-shot delivery, browser race freedom or live enforcement.
This guard does not replace the domain content gate or a send adapter's own
duplicate-delivery protections. Receipt lifetime alone is not single-use proof.

## Qualification

`just outbound-tests` exercises backend failure, bad output, deadlines, privacy
and zero fake dispatches. Rust tests cover exact signed input bindings, changed
text/destination, blocked gate, relayed approval, keys, expiry and revision.
The `outbound_guard` integration tests run actual CLI/wrapper processes with
fresh synthetic keys: unapproved browser click, changed-text send and missing
gate refuse before the fake dispatch; exact receipts and draft creation allow.
No real account, message, private policy or credential is used.

Before claiming production acceptance, observe a refused real-harness fixture
with zero dispatches, an approved fixture pass, configuration/issuer custody,
all covered execution paths and a rollback to a documented safe manual path.
Source tests and a valid signature alone are not installed acceptance.
