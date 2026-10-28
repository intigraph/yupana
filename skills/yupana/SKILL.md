---
name: yupana
description: Orient agents and inspect code structure with Yupana.
---

<!-- BEGIN yupana prime -->
# Yupana agent primer

## What yupana is

Yupana explains code structure: symbols, callers, impact and data dependence.
Bobbin searches code by meaning; Quipu remembers governed knowledge.
Yupana supplies the structural signal that complements those two tools.
Facts carry their actual extraction tier; configured tiers are not measured precision.

## Agent workflow

1. Query Quipu for prior decisions, constraints and provenance before acting.
2. Use Bobbin to find code, then Yupana for callers and impact before structural edits.
3. Start impact at one hop and disambiguate repeated symbol names by location.
4. Inspect tiers and freshness; unknown coverage is not proof of no impact.
5. Validate proposed edits with verification and ordinary repository checks.

## Core commands

| Command | Use |
| --- | --- |
| `yupana prime` | Read-only orientation and bounded local state |
| `yupana prime --brief` | Short orientation for stack aggregation |
| `yupana prime --json` | Structured `stack-prime/1` output |
| `yupana prime --static` | Embedded guidance without config or cache reads |
| `yupana prime --section core --static` | One section by heading prefix |
| `yupana analyze PATH` | Build a structural summary |
| `yupana refs SYMBOL PATH` | Find definitions and references |
| `yupana callers SYMBOL PATH` | Direct callers |
| `yupana impact SYMBOL PATH --hops 1` | Direct structural impact |
| `yupana dataflow FUNCTION PATH` | Data dependence for a function |
| `yupana verify --file FILE --buffer BUFFER` | Check a proposed buffer file |
| `yupana status` | Detailed state; may query the governed projection |
| `yupana serve --read-only` | Serve code query MCP tools with the mcp feature |

## MCP tools

| Tool | Use |
| --- | --- |
| `yupana_status` | Base ref, tiers and policy state |
| `yupana_symbols` | Symbol tree for a file |
| `yupana_references` | Definitions and references by name or position |
| `yupana_callers` | Direct callers |
| `yupana_impact` | Transitive callers and structural impact |
| `yupana_dataflow` | Local dependence; optional Rust CPG |
| `yupana_analyze` | Files and symbols summary |

MCP needs the `mcp` build feature. The full tool reference lists additional
feature-gated capabilities; a configured feature does not prove it is compiled.

## Reading live state

Prime reads local configuration and, with the quipu feature enabled and graph
projection configured, the existing durable projection cache. Its entire live
read has a 650 ms deadline. It does not build a graph, refresh projections,
contact a server, write caches, generate signing keys or emit a usage spool.

The base ref is configured, not a measurement of a resident graph's loaded
commit. Compiled tiers come from the same source as status; phased LSP/CPG configuration
is not reported as served capability. Local rules are configuration entries. Governed counts
cover cached structural and text entries, with cache age, endpoint match and
expiry status; they never claim a fresh live query or current enforcement.
Other policy planes are not included in that count. Missing, unreadable or
expired cache state remains explicit; unknown is never reported as zero rules.

`--static` skips state reads. Degraded reads retain guidance and exit 0.
`--section` matches a case-insensitive heading prefix; usage errors exit 2.

## Next steps

- [Book](https://scbrown.github.io/yupana/)
- [README](https://github.com/scbrown/yupana#readme)
- [MCP reference](https://github.com/scbrown/yupana/blob/main/docs/book/src/reference/mcp-tools.md)
- [Skill](https://github.com/scbrown/yupana/blob/main/skills/yupana/SKILL.md)
- Command help: `yupana prime --help` (no shipped man page).
<!-- END yupana prime -->
