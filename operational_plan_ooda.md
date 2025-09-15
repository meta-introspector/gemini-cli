# Operational Plan: OODA Loop for Gemini CLI Agent

This document outlines the operational framework for the Gemini CLI Agent, based on the Observe, Orient, Decide, Act (OODA) loop, especially when invoked via the `dwim.sh` script or in continuous interaction.

## Framework: Observe, Orient, Decide, Act (OODA) Loop

### 1. Observe: Collect Context

*   **Action**: Gather all available information relevant to the current state and user's request.
*   **Sources**:
    *   **User Input**: The explicit prompt, previous turns in the conversation, and any implicit cues.
    *   **File System**: Read relevant files (`current_task.md`, `new_task.md`, `agent_orchestration_design.md`, `vibe_lattice_concept.md`, `issue_for_parent_project_access.md`, `tasks_and_directories_for_parent_project.md`, `flake.nix`, `Cargo.toml`, etc.) to understand project state, blockers, and design documents.
    *   **Tool Outputs**: Analyze results from previous tool executions (`git status`, `nix build` errors, `cargo` errors, `google_web_search` results, etc.).
    *   **Internal Memory**: Access saved user preferences or facts.
    *   **Project Structure**: Understand the directory layout and file relationships.

### 2. Orient: Merge Context into Model

*   **Action**: Integrate the collected observations into my internal knowledge model, updating my understanding of the project, the user's goals, and the current challenges.
*   **Process**:
    *   **Identify Core Task**: Determine the primary objective the user is trying to achieve.
    *   **Analyze Blockers**: Pinpoint specific technical or environmental obstacles preventing progress.
    *   **Evaluate Tool Limitations**: Acknowledge constraints of available tools (e.g., file access, search capabilities).
    *   **Synthesize Conceptual Frameworks**: Incorporate high-level designs (e.g., Agent Orchestration, Vibe Lattice) and user-provided metaphors.
    *   **Prioritize Subtasks**: Based on the overall objective and current state, identify the most critical next subtask.
    *   **Anticipate Next Steps**: Project potential outcomes and required actions for various paths forward.

### 3. Decide: Reduce Context to Action

*   **Action**: Formulate a concrete plan of action, selecting the most appropriate tool(s) and strategy to address the prioritized subtask, while considering all known constraints and objectives.
*   **Process**:
    *   **Formulate Plan**: Develop a step-by-step plan to execute the prioritized subtask.
    *   **Select Tools**: Choose the most effective tool(s) for each step (e.g., `read_file`, `write_file`, `replace`, `run_shell_command`, `google_web_search`).
    *   **Construct Arguments**: Precisely define arguments for tool calls, ensuring accuracy (e.g., absolute paths, exact strings for `replace`).
    *   **Anticipate Outcomes**: Consider potential success and failure modes for the chosen actions.
    *   **Communicate Plan (if necessary)**: If the action is significant or requires user confirmation, clearly articulate the plan to the user.

### 4. Act: Execute Policy

*   **Action**: Execute the decided plan, performing the chosen tool calls or generating the required output.
*   **Process**:
    *   **Execute Tool Calls**: Invoke the selected tools with the constructed arguments.
    *   **Monitor Execution**: Observe tool outputs for success, errors, or unexpected results.
    *   **Provide Output**: Communicate results of actions to the user.

### Feedback Loop: Change is Observed in Next Step

*   **Continuous Cycle**: The output of the "Act" phase becomes new observations for the next iteration of the OODA loop, allowing for continuous adaptation and progress towards the user's goals.
*   **Adaptation**: If an action fails or yields unexpected results, the loop restarts from "Observe" to re-evaluate the situation and adjust the plan.

This framework will guide my operation to efficiently and effectively address user requests, even in complex and ambiguous scenarios.
