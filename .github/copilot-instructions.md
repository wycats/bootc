# Copilot Instructions

## Bootc Update Lifecycle

When completing a task that involves modifying the OS image (e.g., merging a PR with `Containerfile` changes):

1. **Verify Build**: Ensure the GitHub Action has successfully built and pushed the image (`gh run watch/list`).
2. **Stage Update**: Run `sudo bootc upgrade` to fetch the new image.
   - **Crucial**: Do NOT tell the user "it is safe to reboot" or "the changes are live" until you have successfully run this command.
3. **Handover**: Once the upgrade is staged, inform the user that a reboot is required to apply the changes.

## Tool Usage: ask_questions

Use the `ask_questions` tool to stop and align with the user when:
- **Ambiguity exists**: You are unsure of the user's specific intent or preference.
- **High-Risk Actions**: Before performing destructive operations if not explicitly requested.
- **Workflow Breaks**: When switching context significantly.

**Do NOT use `ask_questions`**:
- To ask for permission to proceed with a task you've already been assigned.
- To confirm low-risk actions clearly implied by the user's request.

## Git Workflow

When asked to commit and/or create a PR:

1. **Always create a feature branch** - Never commit directly to `main`
2. **Use logical commits** - Split changes into meaningful commits (e.g., separate commits for new features, bug fixes, documentation)
3. **One PR per feature set** - Group related changes into a single PR with multiple commits

## Context Management Strategy

Treat the Main Chat context window as a scarce resource to be conserved. Apply the following heuristics to determine whether to execute a task directly or delegate it to a subagent, and how to transfer information effectively.

### Decision Heuristics

1. **Prioritize Isolation (Subagents)**
   Delegate tasks to subagents whenever the work is **modular**—meaning it can be fully defined by a standalone prompt. This "fire-and-forget" approach prevents the Main Chat from being polluted with intermediate steps, verbose output, or temporary data processing.
2. **Prioritize Continuity (Main Agent)**
   Execute tasks directly only when they possess **high contextual entanglement**—where the task relies so heavily on the specific history, nuance, or "state" of the current conversation that accurately serializing that context into a fresh prompt would be inefficient or lossy.
3. **Prefer "Pass by Reference"**
   When delegating, avoid "Passing by Value" (reading file contents into the chat to paste into a subagent prompt). Instead, "Pass by Reference" by directing the subagent to specific file paths or directories. If the data exists only in memory or a command output, write it to a temporary artifact (e.g., `.context/temp_diff.txt`) and point the subagent to that artifact.

### Context Strategy Examples

#### Example 1: High Modularity (Delegate)

**User Request:** "I need a set of unit tests for the `DateFormatter` utility we just pasted. Cover all edge cases for leap years and timezones."
**Model Reasoning:**

- **Input Context:** The target code (`DateFormatter`) is distinct and currently available.
- **Context Entanglement:** Low. The requirements are standard ("cover edge cases") and do not rely on a complex history of user preferences.
- **Token Economics:** Writing comprehensive tests is verbose. Doing this in the main chat would push valuable history out of the context window.
  **Decision:** Delegate to Subagent.
  **Instruction:** "Generate a comprehensive test suite for the provided `DateFormatter` code, ensuring coverage for leap years and timezone edge cases."

#### Example 2: High Entanglement (Execute Directly)

**User Request:** "That looks better, but it contradicts the 'Sentinels' pattern we discussed at the start of the session. Refactor the `User` struct to align with that instead."
**Model Reasoning:**

- **Input Context:** "Sentinels pattern" refers to a specific, agreed-upon definition from earlier in the chat, likely containing nuance not found in public documentation.
- **Context Entanglement:** High. To delegate this, I would have to synthesize and explain the user's specific definition to the subagent. The cost of accurately serializing this context approaches the cost of just doing the work.
- **Risk:** A subagent might hallucinate a standard definition of "Sentinels" rather than the user's specific version.
  **Decision:** Execute by Main Agent.

#### Example 3: Pass-by-Reference via File System (Delegate)

**User Request:** "Read through the entire `legacy_migration/` folder and identify any deprecated API calls in the Python files."
**Model Reasoning:**

- **Input Context:** The user is pointing to a large directory on disk.
- **Context Entanglement:** Low. The definition of "deprecated API calls" is standard or can be easily defined in a prompt.
- **Token Economics:** Reading all those files into the Main Chat to then pass them to a subagent would be disastrously inefficient and likely hit token limits.
- **Strategy:** The subagent has file access tools. I should pass the _path_, not the _content_.
  **Decision:** Delegate to Subagent.
  **Instruction:** "Scan the `legacy_migration/` directory. Identify all Python files containing calls to APIs listed in `docs/deprecations.md` or matching the regex `v1_.*`. Output a list of affected files."

#### Example 4: Intermediate Artifact Creation (Delegate)

**User Request:** "Compare the output of the current build against the logs from the last successful deployment and tell me what changed."
**Model Reasoning:**

- **Input Context:** The necessary information exists in the environment but requires retrieval.
- **Context Entanglement:** Medium. The comparison logic is simple, but the data volume is high.
- **Token Economics:** Streaming two massive log files into the Main Chat just to ask a subagent to diff them is wasteful.
- **Strategy:** I will use the shell to create a "context artifact"—a diff file—and then ask the subagent to analyze _that_ specific artifact.
  **Pre-computation:** Run `diff logs/deploy_success.txt logs/current_build.txt > .context/diff_summary.txt`
  **Decision:** Delegate to Subagent.
  **Instruction:** "I have generated a diff of the build logs at `.context/diff_summary.txt`. Analyze this file and summarize the regression failures."

### Protocol: Prepare → Execute → Review (PER)

A three-phase micro-loop for implementing discrete units of work (features, migrations, fixes).

This approach helps conserve context by isolating each phase in its own subagent, ensuring that the Main Chat remains focused and uncluttered, but still allowing for detailed work to be done in a controlled manner that the main agent can oversee and understand.

**Keyword**: `per` — User can invoke with "do a PER cycle on X" or "prepare→execute→review for X".

#### 1. Prepare (Audit)

- **Agent**: `prepare` subagent (or manual audit)
- **Input**: RFC, plan, or task description
- **Output**: Readiness report with:
  - ✅ Verified assumptions
  - ⚠️ Corrections needed
  - 🔴 Blockers (must resolve before execute)
  - 📋 Implementation order
- **Gate**: User approves or requests fixes

#### 2. Execute (Implement)

- **Agent**: `execute` subagent (strongly preferred)
- **Input**: Approved prepare report
- **Output**: Working code + tests
- **Constraint**: Follow prepare report exactly; no scope creep
- **Gate**: Code compiles, tests pass

**Agent preference**: Use subagents unless the communication overhead (in tokens) clearly exceeds doing it directly. Subagents provide isolation, focus, and auditability.

#### 3. Review (Verify)

- **Agent**: `review` subagent (or manual review)
- **Input**: Executed changes
- **Output**: Review report with:
  - ✅ Correct implementations
  - ⚠️ Issues found
  - 💡 Suggestions
- **Gate**: User accepts or requests fixes

**When to use PER**:

- Implementing RFC phases
- Migrating existing code to new patterns
- Any change with moderate complexity or risk

**When NOT to use PER**:

- Trivial fixes (single-line changes)
- Exploratory work (use recon instead)
- Pure research (no code output)
