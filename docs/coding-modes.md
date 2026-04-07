# Coding Modes — Twelve Programming Workflows

> **SenWeaverCoding (sen)** — An autonomous AI agent runtime and CLI code editor built entirely in Rust.

This document describes the twelve switchable programming modes available in `sen`. Each mode configures a different behavioral profile: system prompt injection, tool allowlists, approval policies, post-tool hooks, and auto-verify behavior.

Switch modes in the REPL with `/mode <name>` or `/m <name>` (e.g., `/m tdd`, `/m harness`). Modes can be switched at any time during a session.

---

## Quick Reference

| Mode | Aliases | Type | Approval | Auto-Verify |
|------|---------|------|----------|-------------|
| **Vibe** | `v` | Full autonomy | Default | No |
| **Agent** | `ag`, `auto`, `agentic` | Fully autonomous orchestrator | Auto-Approve | Yes |
| **Spec** | `s` | Specification-driven | Default | Yes |
| **Plan** | `p` | Read-only planning | Read-Only | No |
| **Ask** | `a`, `q` | Read-only Q&A | Read-Only | No |
| **TDD** | `t` | Test-driven development | Default | Yes |
| **Debug** | `d`, `dbg` | Systematic debugging | Default | Yes |
| **Architect** | `arch`, `design` | Architecture & review | Default | No |
| **Pair** | `pp`, `collab` | Pair programming | Default | Checkpoint |
| **ContextEng** | `ce` | Context engineering | Default | Yes |
| **MVAI** | `mvai` | Interface architecture | Default | Yes |
| **Harness** | `hn`, `hs` | Engineering-grade workflow | Auto-Approve | Yes |

---

## Part I — The First Eight Modes

### 1. Ask Mode (`ask`, `a`, `q`)

**Type:** Read-only Q&A

**Approval Policy:** Read-Only — all write operations are blocked at the tool filtering level.

**Tools:** File read, search, and web tools only. `todo_write` and `update_plan` are excluded.

**Description:**

Ask mode is the safest mode for understanding unfamiliar code. The agent reads files, searches the codebase, runs web searches, and explains what it finds — but makes zero file modifications and zero shell calls with side effects.

This is the ideal mode when you want to:

- Understand the architecture of a new codebase
- Explain what a specific module does
- Trace data flow through a complex system
- Research a library or API before deciding on an approach

The agent focuses on clear explanations backed by code references, citing exact file paths and line numbers.

**When to use:**

```
sen  --mode ask
/m ask
```

Use Ask when you need answers, not changes.

---

### 2. Plan Mode (`plan`, `p`)

**Type:** Read-only planning and analysis

**Approval Policy:** Read-Only

**Tools:** All read tools + `todo_write` and `update_plan`. No file writes, no shell.

**Description:**

Plan mode extends Ask with task management capabilities. The agent can create structured plans, break down work into ordered steps, and assess risks — but still makes no file modifications.

This is the mode for architectural discussions, feature design sessions, and impact analysis. The agent will:

1. Map all files affected by the proposed change
2. Break the work into independently verifiable steps
3. Identify inter-step dependencies and external requirements
4. Specify verification commands for each step
5. Flag risks and breaking-change concerns

**Plan → Spec: The Natural Progression**

After planning, you can approve the plan and switch to Spec mode to execute it:

```
/m plan
> Design a plan to add OAuth2 authentication to the API
[Agent produces a detailed plan]

/m spec
> Execute the plan we just created for OAuth2 authentication
[Agent executes the plan step by step with verification]
```

The **Plan → Spec pipeline** is the recommended workflow for complex tasks:

```
Plan (read-only) ──[user approves]──▶ Spec (structured execution) ──▶ Verify
     ↑                                    │
     └────────[feedback / revision]────────┘
```

Because Plan mode only reads, you can switch freely between Plan and any other mode. Plan your work first, then execute with the appropriate execution mode.

**When to use:**

```
sen  --mode plan
/m plan
```

Best for: architectural discussions, feature design, understanding blast radius, and creating executable plans before writing code.

---

### 3. TDD Mode (`tdd`, `t`)

**Type:** Test-driven development with cycle tracking

**Approval Policy:** Default (supervised)

**Auto-Verify:** Yes — after every file write/edit, the test suite is run automatically.

**Post-Tool Behavior:** `AutoVerify`

**Description:**

TDD mode enforces the Red-Green-Refactor discipline strictly. The agent must follow the cycle in order:

1. **Red** — Write a failing test that describes the desired behavior. Run the test suite and confirm it fails as expected.
2. **Green** — Write the minimum code to make the failing test pass. Do not add extra functionality. Run all tests and confirm they pass.
3. **Refactor** — Clean up the implementation while keeping all tests green. Run tests after each refactoring step.

The agent uses `incremental_optimize` to track each TDD cycle: checkpoint at cycle start, track the Red phase, track the Green phase, track the Refactor phase, and generate a final cycle report.

**Rules:**

- Never write implementation code without a failing test
- One behavior per test — keep tests focused and descriptive
- Run the full test suite after each cycle, not just the new test
- If a bug is discovered during implementation, write a regression test first

**When to use:**

```
sen  --mode tdd
/m tdd
```

Best for: adding new features to an existing test suite, building modules with well-defined contracts, or when you want maximum confidence in correctness.

---

### 4. Debug Mode (`debug`, `d`)

**Type:** Systematic debugging with four-stage protocol

**Approval Policy:** Default

**Auto-Verify:** Yes

**Post-Tool Behavior:** `AutoVerify`

**Description:**

Debug mode applies a disciplined four-stage root-cause analysis. The agent is **forbidden from jumping straight to fixes** — it must follow the protocol:

**Stage 1 — Reproduce:** Run the failing command/test and capture the exact error output. Identify the failing assertion, exception, or unexpected behavior.

**Stage 2 — Hypothesize:** Form at most 3 ranked hypotheses about the root cause. Rank them by likelihood based on the error message, stack trace, and recent changes.

**Stage 3 — Isolate:** For each hypothesis (most likely first), add diagnostic output (debug prints, logging, assertions) to narrow down the cause. Gather evidence before making changes.

**Stage 4 — Fix & Verify:** Apply the minimal fix, run the original failing command, and confirm it passes. Then run the full test suite to check for regressions. Remove diagnostic code after the fix.

**Rules:**

- Do NOT guess-and-check — gather evidence first
- Do NOT apply multiple fixes simultaneously; change one thing at a time
- Always verify the fix resolves the original issue AND passes regression tests
- Remove diagnostic code after the bug is fixed

The agent uses `incremental_optimize` to track the full debugging session: checkpoint at session start, track the reproduction test case, document ranked hypotheses, track diagnostic additions, track the fix, and generate a final session report.

**When to use:**

```
sen  --mode debug
/m debug
```

Best for: mysterious bugs, intermittent failures, and any situation where previous fix attempts have failed.

---

### 5. Architect Mode (`architect`, `arch`)

**Type:** High-level design and code review with targeted edits

**Approval Policy:** Default

**Tools:** Read tools + targeted write tools (`file_write`, `file_edit`, `multi_edit`) + `diagnostics` + `shell` + `git_operations` + `code_to_spec` + `incremental_optimize`.

**Description:**

Architect mode is for high-level design and technical decision-making. The agent reads extensively, analyzes architecture and dependencies, identifies technical debt, and makes targeted structural edits — but focuses on the big picture rather than implementation details.

**Responsibilities:**

1. Analyze codebase structure, dependencies, and architectural patterns
2. Identify technical debt, design flaws, and improvement opportunities
3. Design solutions at the component/module level with clear interfaces
4. Review existing code for correctness, security, and maintainability
5. Document architectural decisions and their rationale

**Workflow:**

- Start by reading project structure (Cargo.toml, mod.rs files, key entry points)
- Map module dependencies and data flow before suggesting changes
- When proposing changes, specify: affected files, new interfaces/traits, migration path, and testing strategy
- Prefer refactoring existing abstractions over introducing new ones
- Flag breaking changes explicitly and propose backward-compatible alternatives

**Constraints:**

- Focus on WHAT and WHY, not detailed HOW (leave implementation to Agent/Vibe/Spec mode)
- Limit edits to: interface definitions, module structure, configuration, and documentation
- Always consider: performance implications, backward compatibility, security surface

**When to use:**

```
sen  --mode architect
/m architect
```

Best for: refactoring projects, introducing new subsystems, architectural reviews, and technical debt assessment.

---

### 6. Pair Mode (`pair`, `pp`)

**Type:** Interactive collaborative pair programming

**Approval Policy:** Default

**Post-Tool Behavior:** `Checkpoint` — the agent pauses after each batch and prompts for user confirmation before proceeding.

**Description:**

Pair mode turns the agent into a collaborative programming partner. The agent communicates its thinking out loud, proposes changes one at a time, and checks in frequently before proceeding.

**Workflow:**

1. **Discuss** — share the mental model and approach before writing code
2. **Propose** — explain what you're about to do and why
3. **Implement** — after implicit or explicit agreement
4. **Verify together** — run tests and review output with the user
5. **Iterate** — ask for feedback, adjust course as needed

**Communication Style:**

- Think out loud — explain reasoning as you go
- Raise potential issues immediately when spotted
- Suggest alternatives when trade-offs exist
- Ask "does this look right?" at natural breakpoints
- Propose the next step if the user is quiet

**Checkpoint behavior:** After each meaningful change (file write/edit), the agent pauses and waits for user confirmation. The user can approve, request modifications, or redirect the conversation entirely.

**When to use:**

```
sen  --mode pair
/m pair
```

Best for: onboarding to a new codebase, teaching, design discussions, code reviews, and situations where you want full visibility and control over every change.

---

### 7. ContextEng Mode (`context`, `ce`)

**Type:** Context engineering — explore-first, precision-strike for large codebases

**Approval Policy:** Default

**Auto-Verify:** Yes

**Post-Tool Behavior:** `ImpactAnalysis` — after each batch, the agent analyzes and reports context changes and impact scope.


**Description:**

ContextEng mode is designed for working in large, complex codebases where context management is the primary challenge. It enforces a four-phase protocol that must be completed in order — no writing code before understanding the terrain.

**Phase 1 — Explore (mandatory before any writes):**

1. Scope the task: identify the user's requirement and list the likely affected subsystems
2. Use `code_to_spec` for fast analysis: `code_to_spec(action="summarize", paths=["."])` for a quick overview, then `code_to_spec(action="analyze", paths=["./src"])` to extract structural information
3. Search before read: use `content_search` and `glob_search` to locate relevant code. **Never** start with `file_read` on entire files
4. Read surgically: once located, read only the relevant line ranges. Track every file you read
5. No stale reads: if you modified a file earlier in this session, re-read the changed sections before referencing them

**Phase 2 — Map (dependency & impact analysis):**

6. Build a context map listing: files to modify (with specific functions/sections), files that import/depend on them (blast radius), and test files covering the affected code
7. Report the map: present a structured summary to the user with estimated line counts and token cost
8. Budget check: confirm the context budget can accommodate the remaining work. If below 30%, summarize and drop non-essential history first

**Phase 3 — Strike (precise, verified edits):**

9. One concern at a time: make the smallest edit that achieves the goal. Do NOT batch unrelated changes into one edit
10. Verify immediately: after each edit, run the project's check/test command and confirm the edit did not break any dependency in the blast radius
11. Re-read after edit: after modifying a file, if you need its content again, re-read the current state

**Phase 4 — Consolidate:**

12. Impact report: summarize files changed (with line count deltas), dependencies verified (tests run + results), and context budget consumed vs. remaining
13. Memory persistence: store key decisions and architectural context in `memory_store` so future sessions don't re-explore the same territory

**Tool Discipline:**

- Preferred call order: `content_search` → `glob_search` → `file_read` (targeted) → `file_edit` → verify
- **Never** `file_read` a file > 500 lines without narrowing to specific line ranges first
- Batch parallel reads in a single tool call group when exploring multiple files
- Use `memory_recall` before re-reading files that may already be in session memory

**Anti-Patterns (enforced):**

- Reading entire large files without prior search — BLOCKED
- Editing code without completing the Map phase — BLOCKED
- Ignoring context budget warnings — must summarize/drop before continuing
- Multiple unrelated edits in one tool call — split them

**When to use:**

```
sen  --mode context
/m context
```

Best for: large production codebases, unfamiliar territory, complex refactors touching many files, and any situation where context management is critical.

---

### 8. MVAI Mode (`mvai`)

**Type:** Model-View-Agent-Interface — interface-first, observable, testable architecture

**Approval Policy:** Default

**Auto-Verify:** Yes


**Description:**

MVAI mode applies the Model-View-Agent-Interface architectural pattern to agent-driven development. It ensures reliable, observable, and testable systems by enforcing clear separation of concerns at every layer.

**Architecture Layers:**

1. **Model** — Data structures and business logic. Define clear types and interfaces BEFORE implementation. Every public function should have documented input/output types.
2. **View** — User-facing output and presentation. Keep presentation logic separate from business logic. Format output consistently.
3. **Agent** — Orchestration layer that decides what actions to take. Log every decision point with reasoning. Make the agent's decision process transparent and auditable.
4. **Interface** — Contracts between layers. Define explicit interfaces (traits, types, schemas) at boundaries. These contracts enable mocking and testing.

**Development Rules:**

- **Interface-first**: Define the trait/interface before writing the implementation. This ensures consumers and tests can be written in parallel.
- **Observable decisions**: Before each significant action (file write, shell command, API call), state WHY you're taking that action. This creates an audit trail.
- **Validate non-determinism**: Treat LLM-generated content (including your own reasoning) as untrusted. Validate outputs against schemas/types before acting on them.
- **Test at boundaries**: Write tests at interface boundaries, not implementation details. Mock the Agent layer in tests to verify deterministic behavior.

**Architecture Decision Tracking:**

The agent uses `incremental_optimize` to track all significant architecture decisions:

- Checkpoint before starting each architecture layer
- Track each new interface/trait definition
- Use `incremental_optimize(action="suggest")` after interface changes for quality recommendations
- Generate a final architecture report

**Quality Checklist:**

- Every new module has a defined interface (trait or type signature)
- Every agent decision is logged with reasoning
- Every external interaction (API, file, shell) has error handling
- Non-deterministic outputs are validated before use

**When to use:**

```
sen  --mode mvai
/m mvai
```

Best for: building robust agent systems, designing multi-layer architectures, creating testable agentic workflows, and establishing clear contracts between components.

---

---

## Part II — The Four Advanced Modes (Detailed)

The following four modes represent progressively more structured engineering workflows. **Vibe** and **Agent** are the two fully autonomous modes with different levels of structure. **Spec** adds spec-first discipline with OpenSpec and skill orchestration. **Harness** combines all engineering best practices into a single cohesive six-layer protocol.

---

### 9. Vibe Mode (`vibe`, `v`)

**Type:** Full autonomy — fast prototyping and free coding

**Approval Policy:** Default (supervised)

**Default Mode:** Vibe is the default mode when `sen ` starts without an explicit `--mode` argument.

**Tools:** All tools — no restrictions.


**Description:**

Vibe mode gives the agent full tool access with minimal prompting. Work autonomously, move fast, and ask the human only when the requirement is genuinely ambiguous. This is the default mode because most coding sessions start with exploratory, low-stakes work — and Vibe's lack of overhead makes it the fastest path from idea to prototype.

**Core Philosophy:**

Vibe mode embodies the principle of **flow state**: remove friction, trust the agent, and let it operate at full speed. The agent has all 130+ tools available, can read and write any file, execute any shell command, and make decisions independently.

**When to use:**

- Quick prototypes and experiments
- One-off tasks with clear requirements
- Exploratory coding where the goal is not yet fully defined
- When you want the agent to just "do it" without ceremony

**When NOT to use:**

- Complex multi-file changes with dependencies (use **Agent** or **Spec**)
- Bug fixes that need root-cause analysis (use **Debug**)
- Adding features to a tested codebase (use **TDD**)
- Large codebases where context management is critical (use **ContextEng**)
- Engineering-grade workflows requiring checkpoints and verification (use **Harness**)

**Discipline:**

Despite being the most free-form mode, Vibe mode still injects the **Verification Discipline** into the system prompt. The agent must never claim a fix is complete without showing passing command output. Evidence before assertions is enforced in all modes.

**Example session:**

```
sen 
> Add a rate limiter to the API client
> Make it configurable via environment variables
> Write tests for the rate limiting logic
> Run the full test suite
```

The agent will work autonomously through this sequence, reading files, writing code, running tests, and self-correcting — without asking for permission at every step.

**Shortcut:** Since Vibe is the default, you rarely need to explicitly switch to it. Just start `sen ` and go.

---

### 10. Agent Mode (`agent`, `ag`, `auto`, `agentic`)

**Type:** Fully autonomous orchestrator — decompose, execute, verify, synthesize

**Approval Policy:** Auto-Approve — all operations are auto-granted. No interactive prompts during execution.

**Auto-Verify:** Yes


**Description:**

Agent mode is the most powerful autonomous mode. It combines full tool access with auto-approval and a three-phase execution protocol: analyze/decompose → execute/self-correct → synthesize/verify. The agent is both the executor and the orchestrator — it decomposes complex tasks into subtasks, executes them with quality gates, self-corrects on failures, and synthesizes the final result.

Unlike Vibe mode (which is fast and unstructured) and Spec mode (which follows a spec you create), Agent mode creates its own execution plan for complex tasks and drives through it autonomously.

**Phase 1 — Analysis & Decomposition:**

1. **Analyze** the task completely before acting. Read relevant files, understand the codebase structure, and identify all affected components.
2. **Decompose** the task into independent, verifiable subtasks. Each subtask should have a clear input, output, and success criterion.
3. **Plan via `todo_write`**: For any task touching 3+ files, register every subtask as a todo item. Include: affected files, expected changes, and verification command.

**Phase 2 — Execution:**

4. **Execute** subtasks in dependency order. For each subtask: read context, implement, verify it compiles/passes, then mark complete.
5. **Self-correct** — if a build/test fails after your change, immediately diagnose and fix it using the systematic debugging protocol. Do not leave broken state.
6. **Verify per-step** — after each subtask, run the relevant check command and confirm success before moving to the next.

**Phase 3 — Synthesis & Verification:**

7. **Synthesize** — after all subtasks, review the aggregate result. Does it satisfy the original requirement? Run the full build + test suite.
8. **Self-evaluate** — compare output against the original spec. If gaps exist, create follow-up subtasks rather than patching inline.
9. **Report** the final status with a summary of all changes made.

**Autonomy Rules:**

- Make decisions independently. Do NOT ask the user for clarification unless the requirement is genuinely ambiguous (multiple valid interpretations).
- When multiple approaches exist, pick the simplest one that meets requirements.
- Commit to completing the entire task in one session. Do not leave partial work.

**Orchestration Discipline:**

- Each subtask must be independently verifiable (can run its own test/check).
- Minimize cross-subtask dependencies; when unavoidable, document them.
- Use `content_search` / `glob_search` to discover all affected files before planning.
- When a subtask fails verification, debug it in isolation before proceeding.
- If a subtask requires research (e.g. API docs), use web search tools proactively.
- Use `shell` for builds/tests/git operations as needed without hesitation.

**Quality Gates:**

- No subtask is complete without passing its verification command.
- The final synthesis step must run the full project build + test suite.
- If total changes exceed 10 files, create a summary of all modifications.

**Spec Discipline for Large Tasks:**

For tasks touching 5+ files, the Agent mode also incorporates spec-first discipline:

1. Run `code_to_spec(action="summarize", paths=["."])` first to understand the codebase
2. Run `code_to_spec(action="analyze", paths=["./src"])` to map the dependency structure
3. Create SPEC.md with `code_to_spec(action="generate", paths=["."], title="<task>", description="<desc>")`
4. Use `incremental_optimize(action="checkpoint", description="Agent: <phase> started")` at phase boundaries
5. Use `incremental_optimize(action="suggest")` after each implementation batch for optimization hints
6. Final synthesis: `incremental_optimize(action="report", description="Agent Task Complete: <name>")`

**When to use:**

```
sen  --mode agent
/m agent
```

Best for: complex multi-step tasks where you want the agent to own the full execution, auto-approve batch operations, and deliver a complete result without stopping for confirmation.

**Agent vs. Vibe:**

| Aspect | Vibe | Agent |
|--------|------|-------|
| Approval | Default (supervised) | Auto-Approve |
| Task decomposition | None | Mandatory for 3+ file tasks |
| Self-correction | Optional | Required on failure |
| Quality gates | None | Per-subtask + final |
| Planning | Ad-hoc | Structured (todo_write) |
| Spec creation | No | Yes (5+ file tasks) |
| Best for | Quick tasks | Complex, multi-step tasks |

---

### 11. Spec Mode (`spec`, `s`) — Specification-Driven with OpenSpec + Superpowers

**Type:** Specification-driven development with incremental optimization and engineering skills

**Approval Policy:** Default

**Auto-Verify:** Yes

**Tools:** All read tools + write tools + `shell` + `git_operations` + `diagnostics` + `todo_write` + `update_plan` + `memory_store` + `memory_search` + `code_to_spec` + `incremental_optimize`.


**Description:**

Spec mode is the bridge between planning and execution. It is built on **OpenSpec** (agree before you build) and **Superpowers** (engineering skills as default behavior), combining structured specification with proactive engineering discipline. It takes the structured thinking from Plan mode and adds the discipline to execute it properly: create SPEC.md before coding, use engineering skills proactively, verify each step compiles, track all changes with `incremental_optimize`, and run the full test suite before declaring done.

**The Three-Step Spec Workflow:**

**Step 1 — Analyze and Spec (OpenSpec: Agree Before You Build):**

Before any code change, use `code_to_spec` to understand the existing codebase:

- Run `code_to_spec(action="summarize", paths=["."])` for a quick overview of the codebase structure
- Run `code_to_spec(action="analyze", paths=["./src"])` to extract structural information from the relevant directories
- Run `code_to_spec(action="generate", paths=["./src"], title="<title>", description="<desc>")` to create SPEC.md
- Before modifying existing code, run `code_to_spec(action="compare", spec_path="SPEC.md")` to check for gaps between the spec and reality

**Step 2 — Track and Optimize (incremental improvement):**

Use `incremental_optimize` to manage changes systematically:

- `incremental_optimize(action="checkpoint", description="pre-change snapshot")` — set a snapshot before starting
- `incremental_optimize(action="track", file="<path>", change_type="modified", summary="<desc>", lines_added=N, lines_removed=M)` — track each change
- `incremental_optimize(action="suggest")` — get optimization recommendations
- `incremental_optimize(action="verify", change_id=N, suggestion_id="<id>")` — mark improvements as applied
- `incremental_optimize(action="report", description="<title>")` — summarize the optimization loop

**Step 3 — Execute and Verify (standard spec-driven workflow):**

1. Clarify requirements and edge cases with the user
2. Create a detailed plan using `todo_write` with file-level changes
3. Execute the plan step-by-step, verifying each step with build/test commands
4. After completing all steps, run the full test suite and report results

**Engineering Skills (Superpowers — used proactively without being told):**

Spec mode injects the following engineering skills as default behavior:

- **Brainstorming**: explore alternatives before choosing an approach. Use the brainstorming skill when facing non-trivial design decisions.
- **Planning**: break work into ordered, independently verifiable steps. Use `todo_write` for every multi-step task.
- **TDD**: write failing tests FIRST, then implement, then refactor. Use the TDD cycle for all new features.
- **Debugging**: Reproduce → Hypothesize → Isolate → Fix. Never guess when debugging.
- **Code Review**: correctness, security, tests, performance, readability. Use the review checklist for all changes.
- **Git Worktrees**: isolate experiments in worktrees. Never pollute the main working directory with risky experiments.

**Hard Rules:**

- **You MUST create a SPEC.md before making significant code changes**
- **You MUST verify each step compiles before moving to the next**
- **You MUST track changes with `incremental_optimize` for any non-trivial modification**

**Planning & Specification Discipline (inherited from `planning_rules`):**

- **File Map**: Start by listing all files that will be created, modified, or deleted
- **Step Breakdown**: Break the work into ordered, executable steps. Each step should be independently verifiable
- **Dependencies**: Identify inter-step dependencies and external requirements
- **Test Strategy**: For each step, specify how to verify it worked (test command, expected output, manual check)
- **Risk Assessment**: Flag any step that might break existing functionality and describe mitigation

**Why Spec over Vibe:**

Vibe is fast and unconstrained. Spec adds overhead — but that overhead pays off when:

- The task touches 3+ files with interdependencies
- The change has a clear specification that can be verified
- You need an audit trail of what was changed and why
- You want engineering skills (TDD, brainstorming, etc.) to fire automatically

**Why Spec over Harness:**

Harness is the most structured mode (six layers, auto-approve, skill orchestration, agent-team coordination). Spec gives you OpenSpec discipline, Superpowers skill automation, and change tracking without the full overhead of Harness. For most real-world tasks, Spec is the right balance.

**Plan → Spec: The Natural Progression:**

```
/m plan
> Design a plan to add OAuth2 authentication to the API gateway

[Plan produces: FILE MAP → STEP BREAKDOWN → DEPENDENCIES → TEST STRATEGY → RISK ASSESSMENT]
[User reviews and approves the plan]

/m spec
> Execute the OAuth2 authentication plan we just created

[Spec creates SPEC.md → tracks changes with incremental_optimize → executes
 step-by-step → verifies each step → runs full test suite → reports results]
```

Because Plan mode only reads, you can switch freely between Plan and Spec. Plan your work first, then execute with Spec mode.

**Example:**

```
sen  --mode spec
> Add a webhook delivery system with retry logic to the gateway module
```

The agent will:

1. **OpenSpec**: Create SPEC.md documenting the webhook delivery contract, retry strategy, failure handling, and acceptance criteria
2. **Superpowers**: Use brainstorming for the caching strategy, TDD for the core algorithm, and planning for the task breakdown
3. **Tracking**: Set a checkpoint for the pre-change snapshot
4. **Execution**: Break the work into: define webhook structs → implement delivery queue → add retry logic → add tests → verify
5. **Verify**: Execute each step, running build/test after each
6. **Report**: Track every change with `incremental_optimize` and run the full test suite

---

### 12. Harness Mode (`harness`, `hn`, `hs`) — Engineering-Grade Workflow

**Type:** Engineering-grade harness — spec → skill orchestration → session checkpoints → verification

**Approval Policy:** Auto-Approve — all operations are auto-granted. No interactive prompts during execution.

**Auto-Verify:** Yes

**Post-Tool Behavior:** `AutoVerify`

**Tools:** All spec_tools + `skill_tool` + `skill_http` + `read_skill` + `enter_plan_mode` + `exit_plan_mode` + `agent_delegate` + `agent_summary` + `agent_compact`.


**Description:**

Harness mode is the most advanced and structured mode in `sen`. It combines the best practices from six engineering methodologies — **OpenSpec**, **Superpowers**, **GSD**, **OMC**, **ECC**, and **Trellis** — into a single cohesive six-layer protocol. Every task flows through all six layers in order, with auto-approval enabling uninterrupted execution.

Harness is designed for engineering-grade workflows where quality, traceability, and systematic discipline are non-negotiable. It is the mode you reach for when:

- The task is mission-critical or production-facing
- Multiple engineers or agents need to collaborate
- The codebase has complex interdependencies
- You need a full audit trail from spec to verification
- You want to extract reusable skills and patterns from the session

**The Six Layers:**

---

#### Layer 1 — Spec Layer (OpenSpec: Agree Before You Build)

**Purpose:** Define what you are building before writing any code.

1. **Clarify the requirement** — identify the exact input, output, and edge cases
2. **Write a structured spec** — proposal → specs → design → tasks structure
3. **Align with the user** — confirm understanding before proceeding
4. **Document acceptance criteria** — how will you know the task is done?

**Rules:**

- Never start coding before completing the spec alignment step
- If requirements are ambiguous, ask the user to clarify — do NOT guess
- Write the spec in `.senweavercoding/plans/` or `STATE.md` for traceability
- Specs are living documents: update them when requirements change

**OpenSpec principle:** The most expensive mistake in software is building the wrong thing. OpenSpec enforces a contract between the agent and the user: agree on what to build before building it.

---

#### Layer 2 — Skill Orchestration Layer (Superpowers: Engineering Discipline as Default)

**Purpose:** Use engineering skills proactively. Make them your default behavior.

**Built-in engineering skills (use them without being told):**

- **Brainstorming** — explore alternatives before choosing an approach. Use the brainstorming skill when facing non-trivial design decisions.
- **Planning** — break work into ordered, independently verifiable steps. Use `todo_write` for every multi-step task.
- **TDD** — write failing tests FIRST, then implement, then refactor. Use the TDD cycle for all new features.
- **Debugging** — Reproduce → Hypothesize → Isolate → Fix. Never guess when debugging.
- **Code Review** — correctness, security, tests, performance, readability. Use the review checklist for all changes.
- **Git Worktrees** — isolate experiments in worktrees. Never pollute the main working directory with risky experiments.
- **Subagent-Driven Development** — decompose complex tasks into parallel subtasks and assign them to specialized agents.

**Rules:**

- Use `read_skill` to load skill contexts when a relevant skill directory exists
- Use `todo_write` to register every subtask with expected changes and verification commands
- When the task touches 3+ files, decompose it into subtasks first
- Prefer git worktrees for risky experiments

---

#### Layer 3 — Session Management Layer (GSD: Solve Context Rot)

**Purpose:** Prevent context from degrading in long tasks. Maintain state across session boundaries.

1. **Checkpoint frequently** — after each logical phase, create a session checkpoint
2. **Keep context clean** — batch related tool calls together. Avoid mixing unrelated changes
3. **Use structured state files** — maintain `STATE.md`, `ROADMAP.md`, `TASKS.md` for long tasks
4. **Compact before it degrades** — if context exceeds 50%, summarize and drop stale history
5. **Atomic commits** — each session phase should produce a clean, revertable commit

**State file discipline:**

- `STATE.md` — current status, blockers, next action
- `ROADMAP.md` — overall plan with completed/in-progress/todo markers
- `TASKS.md` — per-file task checklist with checkmark markers
- After every session checkpoint: update all three files

**GSD principle:** Context rot is the enemy of long-running tasks. Every 30-50% of context consumed, stop and consolidate. The goal is that at any point in the session, you could stop and resume from the structured artifacts alone — not from chat history.

---

#### Layer 4 — Agent-Team Orchestration Layer (OMC: Team-First Execution)

**Purpose:** When parallel execution is beneficial, orchestrate subtasks across agents with a supervisor.

1. **Identify independent subtasks** — tasks with no shared state or sequential dependency
2. **Assign each subtask to an agent** — use `sessions_send` or parallel tool calls
3. **Coordinate with a supervisor agent** — the supervisor reviews all outputs, merges, resolves conflicts
4. **Synthesize the final result** — review aggregate output against the original spec

**Rules:**

- Only parallelize truly independent subtasks — do NOT parallelize dependent steps
- Each sub-agent must checkpoint its work before returning results
- The supervisor agent must verify all sub-agent outputs compile/passing before synthesizing
- If a sub-agent fails, diagnose the failure in isolation before retrying

**OMC principle:** The fastest way to complete a large task is to find the parallelism within it. Not all tasks can be parallelized — but many that look sequential actually have independent phases.

---

#### Layer 5 — Capability Enhancement Layer (ECC: Skills, Memory, Security, Verification)

**Purpose:** Engineer the harness itself — extract patterns, persist knowledge, and enforce quality gates.

1. **Skills and Instincts** — extract reusable patterns from completed tasks into skill files. The next time a similar task comes up, the skill should fire automatically.
2. **Memory persistence** — use `memory_store` to save key decisions, architectural context, and lessons learned. Future sessions should benefit from this session's discoveries.
3. **Security scanning** — verify no secrets, no path traversal, no injection vulnerabilities in every change.
4. **Verification loops** — every change must pass build → lint → test → security check.

**Verification sequence for every code change:**

1. `cargo check` or equivalent — does it compile?
2. `cargo clippy` or equivalent — does it pass lints?
3. `cargo test` or equivalent — do all tests pass?
4. Security check — no hardcoded secrets, no unsafe patterns

**ECC principle:** Every completed task is an opportunity to improve the system itself — through reusable skills, persistent memory, and enforced quality standards.

---

#### Layer 6 — Structure and Project Memory Layer (Trellis: Specs, Tasks, Workspace)

**Purpose:** Organize work around structured artifacts, not chat history. Enable team continuity and session resumption.

**Core structure:**

- `.senweavercoding/plans/*.md` or `.trellis/spec/` — requirements and design specs
- `.senweavercoding/plans/*.md` or `.trellis/tasks/` — per-task context and status
- `.senweavercoding/plans/*.md` or `.trellis/workspace/` — session journals and continuity

**Project memory discipline:**

- Store architectural decisions in `memory_store` after each major phase
- Before starting a new session, use `memory_recall` to restore context
- All team members should be able to join via the structured artifacts, not just chat

**Trellis principle:** Chat history is ephemeral. Structured artifacts are durable. Every decision, every plan, every task status should be captured in artifacts that outlast the session.

---

**Core Harness Rules (Always Active):**

1. **Spec before code** — never skip Layer 1
2. **Auto-verify every change** — run the full verification sequence after every file edit
3. **Checkpoint at every phase boundary** — spec done → plan done → implementation done → review done
4. **Memory is a first-class citizen** — persist decisions, not just code
5. **Never leave broken state** — if a verification step fails, debug it before moving on
6. **Evidence before assertions** — show command output, not just "it works"
7. **Checkpoint discipline** — if approaching session limit, checkpoint and summarize
8. **Context budget awareness** — if context is below 30%, summarize/drop stale history before continuing

---

**Harness vs. Spec vs. Agent:**

| Aspect | Vibe | Spec | Agent | Harness |
|--------|------|------|-------|---------|
| Approval | Default | Default | Auto-Approve | Auto-Approve |
| Spec creation | No | Required | Partial | Required |
| Engineering skills | No | Yes (Superpowers) | No | Yes (all 7) |
| Change tracking | None | `incremental_optimize` | Partial | `incremental_optimize` + state files |
| Auto-verify | No | Yes (on edit) | Yes | Yes (full sequence) |
| Session management | None | Basic | Basic | Layer 3 — structured state |
| Agent-team | No | No | No | Layer 4 — orchestration |
| Capability enhancement | No | No | No | Layer 5 — skills + memory |
| Artifact structure | None | SPEC.md | Partial | Full `.trellis/` structure |
| Skill delegation | No | No | No | Yes (`agent_delegate`) |
| Best for | Quick prototypes | Structured tasks | Autonomous execution | Engineering-grade production |

**When to use Harness:**

```
sen  --mode harness
/m harness
```

Use Harness for:

- Mission-critical production features
- Complex multi-subsystem changes
- Team collaboration with shared context
- Tasks that need full audit trails
- Building systems that will be maintained long-term
- When you want to extract reusable skills from the session

---

## Mode Transitions — How Modes Work Together

The twelve modes are not isolated — they form a natural workflow where you switch between modes as your work progresses.

### The Core Progression

```
Ask (explore & understand)
    ↓
Plan (design & plan)
    ↓ [user approves]
Spec (spec-first execution)
    ↓ [if complex/large]
Harness (engineering-grade)
```

### Common Workflows

**Quick task** — Vibe is sufficient:

```
sen  → /m vibe → task → done
```

**Feature development** — Plan → Spec:

```
/m plan
> Design a plan to add user authentication
[Plan produces a detailed blueprint]

/m spec
> Execute the authentication plan
[Spec creates SPEC.md, tracks changes, executes step-by-step]

/m tdd
> Add tests for the new auth module
[TDD enforces Red-Green-Refactor]
```

**Large codebase refactor** — ContextEng → Spec:

```
/m context
> Refactor the payment processing module
[ContextEng explores the codebase, maps dependencies, executes with impact analysis]

/m spec
> Add comprehensive tests for the refactored payment module
[Spec tracks changes and verifies]
```

**Production-grade feature** — Plan → Harness:

```
/m plan
> Architecture review for the distributed cache system
[Plan analyzes blast radius, dependencies, risks]

/m harness
> Build the distributed cache system with agent-team orchestration
[Harness: OpenSpec → Superpowers → GSD → OMC → ECC → Trellis]
```

**Debugging session** — Debug:

```
/m debug
> Fix the memory leak in the connection pool
[Debug enforces Reproduce → Hypothesize → Isolate → Fix]
```

### Switching Modes Mid-Session

Modes can be switched at any time with `/m <name>`. Common mid-session switches:

- **Vibe → Spec**: When a quick experiment in Vibe mode works and needs to be hardened
- **Plan → Spec**: When the plan is approved and execution should begin
- **Spec → Debug**: When a bug is discovered during spec-driven execution
- **ContextEng → Spec**: When the exploration phase is complete and focused implementation begins
- **Any → Ask**: When you need to understand something before continuing
- **Harness → Plan**: When you need to redesign mid-execution

---

## Mode Comparison Matrix

| Property | Vibe | Agent | Spec | Plan | Ask | TDD | Debug | Architect | Pair | ContextEng | MVAI | Harness |
|----------|------|-------|------|------|-----|-----|-------|-----------|------|------------|------|---------|
| **Tool restrictions** | None | None | Spec tools | Read-only | Read-only (no todos) | None | None | Architect tools | None | None | None | Harness tools |
| **Approval policy** | Default | Auto-Approve | Default | Read-Only | Read-Only | Default | Default | Default | Default | Default | Default | Auto-Approve |
| **Auto-verify on edit** | No | Yes | Yes | No | No | Yes | Yes | No | No | Yes | Yes | Yes |
| **Post-tool behavior** | None | None | None | None | None | AutoVerify | AutoVerify | None | Checkpoint | ImpactAnalysis | None | AutoVerify |
| **Context budget** | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| **Code-to-spec** | No | Partial | Yes | No | No | No | No | Yes | No | Yes | No | Yes |
| **Incremental optimize** | No | Partial | Yes | No | No | Yes | Yes | Yes | Yes | No | Yes | Yes |
| **OpenSpec (agree first)** | No | No | Yes | No | No | No | No | No | No | No | No | Yes |
| **Superpowers (skills)** | No | No | Yes | No | No | No | No | No | No | No | No | Yes |
| **Session state files** | No | No | Basic | No | No | No | No | No | No | No | No | Layer 3 |
| **Agent-team** | No | No | No | No | No | No | No | No | No | No | No | Layer 4 |
| **Skill delegation** | No | No | No | No | No | No | No | No | No | No | No | Yes |
| **Best for** | Quick prototypes | Autonomous execution | Structured tasks | Planning | Understanding | TDD | Debugging | Architecture | Collaboration | Large codebases | Interface design | Engineering-grade |
