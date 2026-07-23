# Product

## Register

product

## Users

RustPLC is used by control engineers, commissioning engineers, safety reviewers, operators, and automation tool builders. They work with PLC programs, topology models, scenarios, verification reports, traces, alarms, and deployment artifacts.

Their job is to express industrial control intent, inspect the compiled semantics, verify safety and timing properties, run no-board or board-backed checks, and review evidence before a program is released.

## Product Purpose

RustPLC is an industrial control modeling and verification system. The product exists to turn PLC DSL source into a compiler-owned IR that can drive verification, runtime execution, simulation, and code generation.

The Web IDE exists to make that compiler pipeline inspectable and operable without requiring every user to work from a terminal. Success means a user can edit PLC source, see compiler and verification diagnostics, inspect topology and flow, run scenarios, review trace evidence, and understand why a gate passed or failed.

## Brand Personality

Precise, restrained, accountable.

The interface should feel like an engineering workstation: dense enough for repeated technical work, calm enough for fault investigation, and explicit about every safety or verification boundary.

## Anti-references

Do not make the Web IDE feel like a marketing landing page, portfolio site, or decorative dashboard. Avoid oversized hero sections, ornamental cards, gradient text, decorative animations, vague AI-assistant copy, and UI that hides compiler evidence behind optimistic status labels.

Do not make safety or verification results feel like lightweight notifications. Diagnostics, warnings, faults, overrides, and approvals must remain concrete, reviewable, and traceable.

Do not invent frontend-only PLC semantics. UI validation, hints, visual flow, and editing affordances must consume compiler-owned diagnostics or artifacts.

## Design Principles

1. Compiler evidence first: every editor marker, topology warning, verification status, and run result should map back to a compiler or runtime artifact.
2. Keep workflows operational: common actions such as edit, validate, run, replay, inspect, and acknowledge should stay close to the workspace.
3. Preserve safety boundaries: high-risk actions require clear status, permission context, confirmation, and audit evidence.
4. Prefer dense clarity over visual decoration: panels, tables, timelines, and canvases should be compact, scannable, and consistent.
5. Make failure actionable: diagnostics should expose stage, code, location, message, and suggested next action whenever the backend provides them.

## Accessibility & Inclusion

Target WCAG 2.1 AA for text contrast, focus visibility, keyboard operation, and status messaging. The interface should not rely on color alone for pass, warning, fail, selected, or disabled states.

Support reduced motion. Motion should communicate state changes only, and no workflow should depend on animation timing.

The UI should remain usable in engineering environments with mixed display quality, constrained screen sizes, and users switching between source, topology, traces, and reports.
