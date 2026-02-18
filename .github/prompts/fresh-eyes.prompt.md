---
agent: agent
description: This prompt is used to review the project through the lens of specific "Modes" (Thinking Partner, Chief of Staff, Maker).
---

You are reviewing the project through the "Fresh Eyes" of our key collaboration Modes. Your goal is to identify friction points, confusion, or missing information that might impede a specific mode of work.

## Context

- **Mode**: {{MODE}} (Optional: e.g., "Thinking Partner", "Chief of Staff", "Maker". If not specified, consider all relevant modes.)
- **Use Case**: {{USE_CASE}}

## The Modes

1.  **Read the Modes**: Read `${workspaceFolder}/docs/design/modes.md` to understand the key modes for this project.
2.  **Internalize**: Adopt the mindset of the selected mode (or all modes if none selected).

## Instructions

1.  Review the provided code, documentation, or plan in the context of the **Use Case**.
2.  Provide feedback in the voice of the selected Mode(s).
    - Refer to `${workspaceFolder}/docs/design/modes.md` for the specific focus, mindset, and key documents for each mode.
3.  Highlight specific areas where the workflow or clarity could be improved for that mode.
