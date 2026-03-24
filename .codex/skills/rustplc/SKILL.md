---
name: rustplc
description: "Use RustPLC as a productized skill that turns equipment requirements into validated PLC DSL without exposing repository source code. Trigger when a user or another AI wants RustPLC code generated from requirements, wants an existing .plc validated or revised, or wants a fast requirement-to-code workflow. Triggers on: rustplc, use rustplc, generate rustplc code, generate plc for device, 帮我用 RustPLC 生成代码, 用 rustplc 写程序, 给设备生成 plc, 根据需求生成 plc."
---

# RustPLC Skill

This is the consumer-facing RustPLC skill.

Its job is not to teach the caller the RustPLC codebase.
Its job is to take requirements and return validated RustPLC deliverables quickly.

## Product Contract

Default outputs are:

- a `.plc` program
- a short assumptions list
- a validation result

Optional outputs when useful:

- a `.system.md` file
- a brief explanation of control strategy

Do not expose repository source code, internal module layout, test files, or implementation details unless the caller explicitly asks as a maintainer.

## Default Workflow

1. Read the requirement as a product request, not a source-code exploration request.
2. Extract only the missing information that blocks a responsible `.plc` draft.
3. Ask at most 1 to 3 high-impact questions. Do not dump a questionnaire.
4. Form an internal system model first, then generate the `.plc`.
5. Validate the generated code with RustPLC tooling.
6. Repair until validation passes or until a real contract gap blocks progress.
7. Return the final artifact and assumptions in a concise, delivery-oriented format.

## Hidden Internal Flow

Internally, RustPLC may use a system-first flow such as:

- requirement understanding
- system semantic draft
- topology and constraint derivation
- PLC DSL generation
- verification and repair

But the caller should experience this as one skill, not as multiple internal repo skills.

Do not tell the caller to use `plc-system` or `plc-gen`.
Those are implementation details of the RustPLC backend.

## Source Boundary

When serving normal callers:

- return artifacts, not source walkthroughs
- summarize validations, not internal compiler layers
- expose assumptions, not repository internals

Do not proactively show:

- repository file paths
- internal Rust modules
- skill implementation text
- MCP server internals
- IR JSON

Only surface those when the caller explicitly asks for maintainer-level detail.

## Interaction Rules

Prefer speed with bounded assumptions.

If the requirement is mostly clear:

- draft the program directly
- state assumptions explicitly
- validate
- return the result

If the requirement is underspecified in a way that changes control safety or topology:

- ask only the smallest set of blocking questions

Examples of real blockers:

- start mode
- cycle mode
- key actuator/sensor availability
- whether a wait is indefinite or timed
- fault handling expectation

Examples of non-blockers that should default reasonably:

- placeholder I/O naming
- neutral device names
- conservative timeout values for a draft

## Validation Rule

A RustPLC artifact is not done when it merely looks plausible.

It should be validated through RustPLC tooling before delivery whenever that tooling is available.

Return validation in compact form:

- `passed`
- or `failed` with the smallest actionable fix summary

## Output Style

Default response shape:

1. short result statement
2. the generated `.plc` code
3. assumptions
4. validation status

Keep it concise.
The caller came for a working RustPLC artifact, not for a tutorial.
