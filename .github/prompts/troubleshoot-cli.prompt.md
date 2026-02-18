---
agent: agent
description: Use this prompt when you are stuck trying to modify a read-only file and cannot find the correct `exo` command.
---

# CLI Troubleshooting & Bug Reporting

You are attempting to modify a **Read-Only** context file (e.g., `plan.toml`, `ideas.toml`) but cannot find the appropriate `exo` command.

**CRITICAL RULE**: Do **NOT** change the file permissions (`chmod`). Do **NOT** edit the file manually.

## Protocol: The "Broken Tool" Check

1.  **Discovery**: Attempt to find the command using the help system.

    - Run `exo --help`.
    - Run `exo <category> --help` (e.g., `exo plan --help`, `exo task --help`).

2.  **Diagnosis**:

    - **Case A: Command Found**: You found the command, but it wasn't obvious or you missed it initially.
    - **Case B: Command Missing**: The command simply does not exist.
    - **Case C: Command Broken**: The command exists but fails or produces unexpected output.

3.  **Action**:
    - **For Case A (Found)**: Proceed with the task using the command. **BUT**, you must still report a "Discovery Friction" bug.
    - **For Case B (Missing)**: Stop the task. Report a "Missing Feature" bug.
    - **For Case C (Broken)**: Stop the task. Report a "Crash/Bug".

## Reporting Format

Since you cannot edit the files directly, output the following block for the user:

```markdown
## 🚨 Tooling Issue Report

**Type**: [Missing Feature / Discovery Friction / Bug]
**Context**: I was trying to [Action] in [File].
**Observation**: I could not find a command to do this. I checked `exo --help` and ...
**Request**: Please implement a command like `exo [suggested-command]` or clarify how to perform this action.
```

## Philosophy

If the tool is hard to use, **that is a bug**. We do not work around bugs by hacking file permissions; we report them so they can be fixed.
