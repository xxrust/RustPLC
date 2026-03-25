---
name: skill-flywheel
description: Evolve a project-specific skill through a three-agent loop that separates source-aware skill editing, no-source task execution, and source-aware root-cause analysis. Use when a repository such as RustPLC needs to improve an outward-facing skill without exposing private source code, and you need to iterate by packaging a public artifact surface, running a blind operator, collecting pain points, and deciding whether the fix belongs in the skill, the public surface, or the codebase.
---

# skill-flywheel

Run a tight iteration loop around a protected codebase without stuffing private implementation details into the skill.

Load only the references you need:

- `references/workflow.md`
  End-to-end loop, sequencing, and review checkpoints.
- `references/boundary.md`
  What "no source access" means here, and what it does not mean.
- `references/classification.md`
  How to classify each pain point as a skill, public-surface, code, or task issue.
- `references/prompt-templates.md`
  Prompt shapes for the three required agents.
- `references/rust-plc-profile.md`
  Default public artifact surface for `E:\personal_project\rust_plc`.

## Core Rules

1. Always use at least three agents:
   - Agent 1: source-aware skill editor
   - Agent 2: no-source operator
   - Agent 3: source-aware root-cause analyst
2. Treat source protection as procedural unless the environment gives you a real sandbox. Never claim hard isolation if you only used instructions.
3. Build a public artifact surface before asking the no-source operator to do real work.
4. Keep Agent 2 on the public surface only. If Agent 2 needs source facts to succeed, record that as a finding instead of silently reading source.
5. Prefer stable exported artifacts, commands, manifests, diagnostics, and examples over embedding private reasoning into the skill.
6. Keep the target skill lean. Repeated missing facts belong in the public surface or the codebase if they can be exported mechanically.
7. Record each loop in a cycle directory so later iterations can reuse the evidence without re-reading the whole repo.
8. Test the real target skill directly. Do not create or rely on a copied skill under `public/`.

## Default Workflow

1. Read `references/workflow.md`.
2. If the target project is RustPLC, read `references/rust-plc-profile.md`.
3. Run `scripts/init_public_surface.py` to create a cycle directory with:
   - `public/`
   - `logs/`
   - `prompts/`
4. Use the generated prompts as the starting point for the three-agent loop.
5. Read `logs/pain-points.md` and `logs/root-cause.md`.
6. Apply the smallest fix at the right layer:
   - target skill
   - public artifact surface
   - codebase contract / tool / diagnostic
7. Re-run the cycle when the change is material.

## Minimum Completion Standard

Do not stop at "the skill should be better". Produce at least:

- a public artifact bundle or an updated public-surface recipe
- a pain-point log from Agent 2
- a root-cause decision from Agent 3
- a concrete minimal patch plan for Agent 1 when the issue is a skill gap
