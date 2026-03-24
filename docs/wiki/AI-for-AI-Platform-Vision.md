# AI for AI Platform Vision (Repo-local Wiki Draft)

This page defines the product direction for RustPLC as an `AI for AI` software system rather than only an AI-assisted PLC authoring tool.

Date: 2026-03-24

---

## Positioning

RustPLC should be understood as a control-systems compiler and evidence pipeline for AI-generated automation artifacts.

The goal is not:

- "generate some PLC code from a prompt"
- "wrap a chatbot around industrial syntax"

The goal is:

- let AI systems produce industrial control intent
- compile that intent into a unified semantic model
- verify the result before deployment
- preserve enough evidence that another AI or engineer can audit the outcome

In short:

- AI creates candidate automation
- RustPLC decides whether that automation is semantically closed, verifiable, and shippable

---

## Why This Matters

Most "AI for software" products stop at text generation.

That is not enough for industrial control. The missing layers are:

- semantic closure
- formal verification
- deterministic execution
- traceability
- reproducible release evidence

RustPLC is interesting precisely because it can become the layer that sits between:

- AI systems that synthesize control logic
- and the engineering process that must trust, reject, replay, and ship that logic

---

## The Product Loop

The intended closed loop is:

1. AI agent generates `.system.md`, `.plc`, scenarios, I/O maps, and release metadata candidates.
2. RustPLC parses and lowers those artifacts into AST, semantic model, and IR.
3. Verification engines check safety, liveness, timing, and causality.
4. Simulation and runtime tooling produce traces, diagnostics, and replayable evidence.
5. Codegen emits target artifacts only after semantic closure is explicit.
6. Release bundle captures the exact evidence chain for human or machine review.

This is why RustPLC should not collapse into a prompt wrapper.

The durable asset is the pipeline, not the chat interface.

---

## Non-Negotiable Contracts

For RustPLC to qualify as an `AI for AI` platform, these constraints must remain true:

- AI-generated content must enter a single IR-centered semantic path.
- Verification is a first-class gate, not a post-hoc plugin.
- Runtime must execute defined semantics, not invent missing semantics.
- Codegen must reject or explicitly erase semantics; it must never silently drop them.
- Diagnostics and release outputs must be machine-consumable and auditable.

One recent example is workpiece semantics:

- workpiece modeling remains available for verification, simulation, lineage, and diagnostics
- ST output does not preserve workpiece objects
- the ST backend now performs explicit semantic erasure with an annotated header instead of a silent loss path

See:

- `docs/workpiece_to_st_codegen_policy.md`

---

## What "AI for AI" Means Here

In RustPLC, `AI for AI` means the system is designed so that one AI can produce artifacts and another AI can reliably consume, verify, critique, transform, or ship them.

That requires outputs richer than plain code:

- IR JSON
- verification reports
- timing reports
- traces
- diagnostics
- release manifests

These outputs form the interface between cooperating agents.

---

## Near-Term Execution Priorities

The next useful steps are:

- make repo-local wiki and GitHub wiki tell one consistent story
- keep README focused on product positioning instead of only feature inventory
- continue hardening semantic gates so AI-generated programs fail early and precisely
- keep ST/codegen policy explicit whenever upper-layer semantics are erased
- strengthen release-bundle and evidence contracts for downstream AI review

---

## Bottom Line

If RustPLC succeeds, it will not be because it can generate PLC text.

It will be because it can turn AI-generated control intent into something that is:

- semantically explicit
- formally checked
- executable
- auditable
- reproducible

That is the standard required for software that aims to impress globally in the `AI for AI` category.
