# RustPLC Profile

Use this profile when the protected project is `E:\personal_project\rust_plc`.

## Default Public Surface

The bootstrap script exposes these paths by default for profile `rust-plc`:

- `README.md`
- `README_EN.md`
- `QUICKSTART.md`
- `AGENTS.md`
- `docs/`
- `examples/`
- `devices/`
- `scenarios/`
- `.codex/skills/plc-gen/`
- `.codex/skills/plc-system/`

This keeps the blind operator on public contracts, examples, and already-exposed skills while excluding private implementation directories such as `src/` and `crates/`.

## Typical Targets

Good target tasks for the blind pass:

- explain how to scaffold a RustPLC project from public docs
- use `plc-system` to draft a `main.system.md`
- use `plc-gen` to propose a verified `plc/main.plc` workflow
- identify the correct validation commands for a public example

## Typical Protected Paths

Do not include these in the blind bundle unless the user explicitly changes the boundary:

- `src/`
- `crates/`
- `target/`
- `.git/`
- `vendor/`
- `web-ui/`

## Preferred Fix Order

When the blind operator struggles on RustPLC:

1. Improve exported docs, command outputs, and examples.
2. Improve project skills such as `plc-gen` or `plc-system`.
3. Improve source-level outward contracts if the capability is still hidden.

Do not skip straight to stuffing internal architecture knowledge into the target skill.
