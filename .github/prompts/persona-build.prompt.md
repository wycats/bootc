---
agent: agent
description: This prompt is used to define or refine a "Mode" or "Persona" for the project.
---

### Building a Mode or Persona

You are helping the user define a new **Mode** (collaboration style) or **Persona** (user profile) for the project. These definitions guide how the AI interacts and how the project is designed.

**Goal**: Create a detailed profile that captures the mindset, focus, and triggers for this Mode or Persona.

**Instructions**:

1.  **Determine Type**: Ask if the user is defining a **Mode** (how the AI works) or a **Persona** (who the user is).

2.  **Drafting a Mode**:

    - **Name**: A descriptive title (e.g., "The Architect").
    - **Focus**: What is the primary concern? (e.g., "Exploration", "Execution").
    - **When to use**: In what phase or situation is this mode active?
    - **Mindset**: Bullet points describing how to think (e.g., "Challenge assumptions", "Follow the plan").
    - **Key Documents**: What files are most relevant to this mode?

3.  **Drafting a Persona**:

    - **Name**: A catchy title (e.g., "The Pragmatist").
    - **Description**: A brief summary of who they are.
    - **Needs**: What do they need from the project?
    - **Frustrations**: What drives them away?

4.  **Update Documentation**:
    - For **Modes**, update `${workspaceFolder}/docs/design/modes.md`.
    - For **Personas**, update `${workspaceFolder}/docs/design/personas.md` (create if it doesn't exist, or add to `modes.md` if appropriate).
