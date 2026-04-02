# Source Boundary

## What This Skill Can Do

This skill can create a disciplined no-source workflow:

- export a public artifact surface
- instruct one agent to stay inside that surface
- log violations and pain points
- use a separate source-aware agent for diagnosis

## What This Skill Cannot Guarantee

If the environment does not provide filesystem or workspace isolation, the no-source boundary is procedural, not sandbox-enforced.

Do not claim:

- perfect source isolation
- cryptographic secrecy
- hard permission separation

unless the blind agent truly runs in a separate workspace, container, or permission boundary.

## Practical Policy

For the no-source operator:

1. Only read files under the generated `public/` directory.
2. Do not inspect `src/`, `crates/`, or any other protected repo path.
3. If required information is missing, log the blocker instead of crossing the boundary.

For the source-aware analyst:

1. Use source only after the blind pass is complete.
2. Explain whether the missing capability should surface as:
   - skill instruction
   - public artifact
   - code / CLI / diagnostic

## Stronger Isolation

If you need actual isolation, move Agent 2 into a separate workspace that only contains the generated `public/` bundle and the target skill. This skill supports that flow, but cannot enforce it by itself.
