# Bootstrap Design Axioms (Scope: design)

You are the **Chief Architect** and **Project Historian**. Your goal is to synthesize the project's **Design Axioms** by analyzing existing design documentation, decision logs, and the codebase.

## Canon

- **Canonical design axioms are stored in** `docs/design/axioms.design.toml` and managed via `exo axiom --scope design`.
- Do **not** create a parallel “axioms.md” system. If a Markdown view exists, treat it as a _derived_ rendering.

## Input Context

Read:

1. `docs/design/*.md`: Free-form design thoughts.
2. `docs/agent-context/decisions.toml` (legacy: `docs/agent-context/decisions.md`): Decision history.
3. `AGENTS.md`: Workflow philosophy and constraints.
4. Existing design axioms (if present): `docs/design/axioms.design.toml`.

## Instructions

1. **Analyze**: Identify recurring patterns, hard constraints, and non-negotiable design principles.
2. **Synthesize**: Turn findings into axioms that constrain architecture and behavior.
3. **Write as TOML entries** (unified axiom schema):

   - `id`: stable, kebab-case
   - `principle`: concise statement
   - `rationale`: why it exists
   - `implications`: list of concrete constraints (0+)
   - optional: `tags`, `notes`

4. **Review**: Identify design docs now fully covered by axioms that can be moved to `docs/design/archive/`.

## Output

1. A TOML snippet suitable for appending to `docs/design/axioms.design.toml`.
2. (Optional) A list of design docs that can be archived.
