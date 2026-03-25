---
name: rustplc
description: "Use RustPLC as a product-facing requirement-to-artifact skill that turns equipment requirements into validated RustPLC deliverables, scaffolded projects, and runnable command sequences without assuming repository knowledge or source access. Trigger when the user wants RustPLC code generated from requirements, wants an existing .plc validated or repaired, needs the RustPLC scaffold/workflow explained, or needs exact CLI commands to create, validate, simulate, gate, or export a project."
---

# rustplc

Use RustPLC as a product, not as a codebase tour.

Return validated deliverables quickly.
Do not assume the caller has source code, repo context, or command discovery ability.
Do not proactively expose repository internals unless the caller explicitly asks for them.

Keep this file lean.
Load only the reference file you need:

- `references/workflow.md`
  Use for end-to-end delivery flow and decision points.
- `references/commands.md`
  Use for exact CLI invocations, launcher selection, and command discovery.
- `references/project-layout.md`
  Use when scaffolding a project or explaining which files to edit.
- `references/output-contract.md`
  Use to shape the final response and required deliverables.
- `references/troubleshooting.md`
  Use when a command fails, help text is missing, or the environment is unclear.

## Core Rules

Treat RustPLC as a service with two launch modes:
- installed binary mode: `rust_plc ...`
- source workspace mode: `cargo run --release --bin rust_plc -- ...`

Never suggest `cargo run --release -- ...` without `--bin rust_plc`.
This workspace has multiple binaries, so the shorter form is unreliable.

Do not rely on top-level `--help`.
If command discovery matters, use `references/commands.md` and give the caller the exact subcommand syntax.

## Default Workflow

1. Read the requirement as a delivery request.
2. Ask only the smallest set of blocking questions.
3. Prefer a scaffolded project when the request is broader than a single file.
4. Generate or repair the required artifacts.
5. Validate with the real RustPLC tooling.
6. Repair until validation passes or a real contract gap remains.

## Internal Composition

You may internally think in a system-first flow:
- requirement understanding
- `.system.md` clarification
- `.plc` generation
- validation and repair

But present the result as one cohesive RustPLC delivery.
Do not tell normal callers to manually switch to internal repo skills.

## Completion Rule

A RustPLC result is not done when it only looks plausible.
Return only validated artifacts or a precise blocking gap.
