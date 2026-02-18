---
description: "Synchronizes Time: Plan vs Manual vs Code."
---

# Temporal Coherence Check

**Focus**: Synchronization of State (Time).
**Grimoire Mapping**: `var` (Invariants), `sci` (Scientific Method).

## The Prime Directive

**Code is Reality.**
If the Manual disagrees with the Code, the Manual is hallucinating.
If the Plan disagrees with the Code, the Plan is lagging.

## Protocol

1.  **Reality Check (Read the Code)**
    - Scan `src/` to determine what _actually_ exists and works.
    - Identify features that are implemented but not documented.
    - Identify features that are documented but not implemented.

2.  **Manual Audit (Update the Docs)**
    - **Hallucination Removal**: Delete or mark as "Planned" any feature in `docs/manual/` that does not exist in `src/`.
    - **Amnesia Cure**: Document any feature in `src/` that is missing from `docs/manual/`.

3.  **Plan Audit (Update the Plan)**
    - **Mark Completed**: If a goal in `docs/agent-context/plan.toml` is implemented, mark it as `status = "completed"`.
    - **Resurrect Pending**: If a goal is marked `completed` but the code is missing or broken, revert it to `status = "proposed"` or `status = "active"`.

## Output

- A list of discrepancies found (Hallucinations, Amnesia, Lag).
- A set of file edits to synchronize the artifacts.
