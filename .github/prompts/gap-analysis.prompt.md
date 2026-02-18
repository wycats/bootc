---
agent: agent
description: A meta-protocol to audit tools by simulating usage, checking documentation, and verifying implementation.
---

# Gap Analysis & Tool Audit Protocol

You are the **Tool Auditor**. Your goal is to verify if our tools actually meet the needs of the agent workflow by performing a "Reality Check".

## Phase 1: The Simulation (The Ideal)

**Goal**: Define what _should_ exist based on the user's intent.

1.  **Scenario**: Imagine you need to perform a specific task (e.g., "Manage RFCs", "Refactor a module").
2.  **Prediction**: List the specific commands and flags you would _expect_ to find in a perfect tool.
    - _Example_: "I expect `exo rfc promote` to move the file."
    - _Example_: "I expect `exo rfc edit` to change the title."

## Phase 2: The Discovery (The Surface)

**Goal**: Determine what _appears_ to exist.

1.  **Help Check**: Run the tool's help command (e.g., `exo rfc --help`).
2.  **Doc Check**: Read the relevant manual or RFCs.
3.  **Comparison**: Compare your **Prediction** with the **Discovery**.
    - What is missing?
    - What is named differently?

## Phase 3: The Audit (The Code)

**Goal**: Determine what _actually_ exists.

1.  **Source Check**: Read the implementation code (e.g., `src/main.rs`, `src/lib.rs`).
2.  **Ghost Hunting**:
    - Are there features implemented in the library but not exposed in the CLI?
    - Are there commands in the CLI that are just "TODO" stubs?
    - Are there hidden flags?

## Phase 4: The Report

**Goal**: Synthesize findings into actionable recommendations.

1.  **The Gap**: Explicitly list the missing features (Ideal vs. Reality).
2.  **The Discrepancy**: Note any "Ghost Features" (Code vs. CLI) or "False Promises" (Docs vs. Code).
3.  **Recommendations**:
    - Create an RFC to fill the gap?
    - Update documentation to match reality?
    - Wire up existing code?

## Output Format

> **Simulation**:
> [List of expected commands]
>
> **Discovery**:
> [List of actual commands found in help]
>
> **The Gap**:
>
> - ❌ [Missing Feature]
> - ⚠️ [Mismatched Feature]
>
> **The Audit**:
>
> - [Findings from code inspection]
>
> **Recommendations**:
> [Actionable next steps]
