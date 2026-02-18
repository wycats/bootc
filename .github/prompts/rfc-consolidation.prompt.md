# RFC Consolidation

Consolidate the RFCs touching a topic area so they tell a coherent, parsimonious story — reflecting either implemented reality or a clear plan.

## The Axiom

> RFC consolidation happens just-in-time, scoped to the topic being worked on — not as a separate bulk activity. Tension, duplication, or overlap between RFCs should map onto implementation cleanup work to be done at the same time. An RFC collection that has been consolidated but whose code hasn't been aligned is worse than the original mess — it creates a false sense of order.

## When to Use

Before starting implementation work on a topic that has accumulated RFCs across stages.

## Input

1. **Topic** — What you're about to work on (e.g., "chat history", "CLI tool architecture")
2. **Context** — What the current implementation actually looks like

## How This Works

This is a **collaborative** process between you and the user. You do the research and analysis; the user makes the judgment calls. You should surface ambiguities, tensions, and tradeoffs — not resolve them silently.

### Step 1: Discover

Search `docs/rfcs/` for RFCs related to the topic:

- Grep for keywords and synonyms
- Grep for related function/type/binary names from the codebase
- Scan stage directories for titles that might be related
- Check `docs/rfcs/withdrawn/` — withdrawn RFCs are context

**Watch for migration duplicates**: The RFC migration created many `0xxx` / `10xxx` pairs with identical content at different stages. These are systematic — actively look for them, don't just stumble on them. You'll also find duplicates outside your topic scope; it's fine to clean those up opportunistically.

### Step 2: Read and Understand

Read each discovered RFC **fully**. For each one, understand:

- What it proposes
- Whether it was implemented (check the codebase)
- How it relates to the other RFCs in the cluster
- Whether it's a duplicate of another RFC (the migration created many `0xxx` / `10xxx` pairs with identical content)

### Step 3: Write It Today

**This is the key step.** Don't start by classifying the existing RFCs into tiers or producing a mechanical table. Instead, ask yourself:

> _If I were writing the RFC collection for this topic from scratch today — knowing what I know about the codebase, the implementation, and the design intent — what documents would I create?_

Think about **natural boundaries**, not existing document boundaries. Multiple RFCs may describe different facets of the same system. One RFC may contain content that belongs in two different documents. The existing RFC structure is an accident of history — ideas arrived at different times, got written up separately, evolved independently.

Work through this by:

1. **List the concepts** that need to be documented (not the existing RFCs — the _concepts_)
2. **Group them by natural boundary** — things that change together, things that a reader needs to understand together
3. **Map existing RFCs onto those groups** — some will map 1:1, some will split, some will merge
4. **Identify the gaps** — concepts that aren't documented, or that are scattered across RFCs without a clear home

Present this analysis to the user as a narrative, not a table. Walk through your reasoning: "These three RFCs are really describing the same system from different angles. Writing from scratch, I'd write one document covering X, Y, and Z."

### Step 4: Discuss

Surface the judgment calls that emerge from the Write It Today analysis. The interesting questions are about **boundaries**, not about individual RFCs:

- "These three RFCs describe different facets of the same system — should they be one document or stay separate?"
- "This RFC has content about both X and Y — X belongs with this cluster, but Y belongs with that other system. Split it?"
- "Writing from scratch, I'd draw the boundary here instead of there — does that match your mental model?"

Use structured questions with concrete options (and room for freeform input). Don't dump all questions in prose — structured choices let the user respond quickly when the answer is obvious and elaborate when it isn't.

**The user's questions are inputs, not just approvals.** When the user pushes back or reframes, that's often the most valuable signal. The user may see a natural boundary you missed, or may have a different mental model of how the concepts relate. Follow that thread — it usually leads to a better consolidation than your initial analysis.

### Step 5: Execute Together

Once you and the user agree on what the collection _should_ look like, work backward to figure out the mechanical steps:

- **Delete migration duplicates** — `0xxx` / `10xxx` pairs where the lower-stage copy is a strict subset. These are always safe to delete. Do them first to reduce noise.
- **Merge** RFCs that belong together — when multiple RFCs map to the same natural boundary, write a single document that tells the whole story. Don't just concatenate sections; write it as if it were new, drawing on the best content from each source.
- **Prune** RFCs that have accumulated content outside their natural scope — move that content to where it belongs.
- **Withdraw** RFCs whose ideas were explored but rejected — move to `withdrawn/` with rationale.
- **Delete** absorbed RFCs after their content has been merged elsewhere.
- **Surface tensions as tasks** — if the consolidated RFCs reveal that the code doesn't match the design, that's implementation work, not just an RFC edit.

**Stage assignment for merged RFCs**: A merged RFC's stage should reflect its _content_, not the highest stage of its inputs. If you're writing a new document that includes both implemented reality (Stage 3) and a forward plan (Stage 1), the RFC is Stage 1 — it's a proposal that happens to document existing work as context.

## Anti-Patterns

**The Tier Table**: Classifying RFCs into "Tier 1 / Tier 2 / Tier 3" and presenting a mechanical inventory. This produces a _catalog_ when what you need is _understanding_. The Write It Today framing forces you to think about concepts and boundaries, not just documents.

**Absorb and Delete**: Mechanically concatenating RFC sections into a target document. The point of merging is to write a _better_ document, not a longer one. If the merged RFC reads like three stapled-together papers, you haven't consolidated — you've just moved text.

**Scope Creep**: Consolidating RFCs that aren't related to the current topic. Stay scoped.

**Paper-Only Consolidation**: Making the RFCs look coherent without aligning the code. If an RFC says "we use approach X" but the code uses approach Y, that's a task, not just an RFC edit.
