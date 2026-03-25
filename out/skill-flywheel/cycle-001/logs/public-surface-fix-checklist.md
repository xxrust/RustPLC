# Public Surface Fix Checklist

Task baseline:
Explain how to scaffold a RustPLC project named `demo_line` and list the first three validation commands using only public artifacts.

## Canonical public answer to align around

### Scaffold creation

- Installed binary:
  - `rust_plc new demo_line`
- Source workspace:
  - `cargo run --release --bin rust_plc -- new out/demo_line`

### First three validation commands

- Installed binary:
  - `rust_plc scenario-validate plc/main.plc --scenario scenarios/nominal/normal.yaml --output human`
  - `rust_plc scenario-doctor plc/main.plc --scenario scenarios/nominal/normal.yaml --output human`
  - `rust_plc no-board-gate plc/main.plc --scenario scenarios/nominal/normal.yaml --out-dir out/gate/no_board/normal --output human`
- Source workspace:
  - `cargo run --release --bin rust_plc -- scenario-validate out/demo_line/plc/main.plc --scenario out/demo_line/scenarios/nominal/normal.yaml --output human`
  - `cargo run --release --bin rust_plc -- scenario-doctor out/demo_line/plc/main.plc --scenario out/demo_line/scenarios/nominal/normal.yaml --output human`
  - `cargo run --release --bin rust_plc -- no-board-gate out/demo_line/plc/main.plc --scenario out/demo_line/scenarios/nominal/normal.yaml --out-dir out/demo_line/out/gate/no_board/normal --output human`

Source of truth for this baseline:

- `.codex/skills/plc-gen/references/commands.md`

## P0

1. Unify launcher wording in top-level README.
   Target:
   - `README.md`
   Current drift:
   - Uses `cargo run --release -- ...` for scaffold creation.
   Required fix:
   - Replace short-form cargo launcher with `cargo run --release --bin rust_plc -- ...`
   - Add one explicit note: scaffold projects are not Cargo projects; source-workspace commands must run from the RustPLC repo root.

2. Unify the first three validation commands in top-level README.
   Target:
   - `README.md`
   Current drift:
   - Recommends `scenario-validate`, `no-board-gate`, `gen-st`
   Required fix:
   - Change the “day-1” command sequence to:
     - `scenario-validate`
     - `scenario-doctor`
     - `no-board-gate`
   - Move `gen-st` to “optional next step” instead of “first three checks”.

3. Fix scaffold quickstart execution context.
   Targets:
   - `examples/project_scaffold_demo/README.md`
   - generated help bundle mirror under `--help/README.md` if that file is derived from the same source
   Current drift:
   - Commands look scaffold-local even though cargo launcher must run from repo root.
   Required fix:
   - Either switch examples to installed-binary form inside the scaffold, or keep source-workspace form but prefix them with repo-root/full-path usage.
   Preferred fix:
   - Show both modes explicitly:
     - installed binary inside scaffold dir
     - source workspace from repo root with full scaffold-relative paths

## P1

4. Unify scaffold layout doc with the same first-three-checks sequence.
   Targets:
   - `examples/project_scaffold_demo/docs/project-layout.md`
   - generated help bundle mirror under `--help/docs/project-layout.md` if derived
   Current drift:
   - Uses `sim-plc` as the second command instead of `scenario-doctor`
   Required fix:
   - Make the first three checks match the canonical sequence.
   - Keep `sim-plc` as an optional follow-up, not part of the first-three baseline.

5. Add one canonical scaffold quickstart artifact.
   Targets:
   - either extend `examples/project_scaffold_demo/README.md`
   - or add one generated/public doc referenced from `README.md`
   Required contents:
   - create project
   - explain binary vs source-workspace launcher
   - explain execution directory
   - list first three validation commands
   - list optional next steps like `sim-plc`, `gen-st`, `build-rp2040`
   Goal:
   - blind operators should not need to merge several docs mentally.

## P2

6. Audit generated help/public mirrors for drift.
   Targets:
   - `--help/README.md`
   - `--help/docs/project-layout.md`
   - any generator or source doc that feeds them
   Required fix:
   - Ensure mirrors are regenerated from the same canonical content instead of hand-maintained variants.

7. Add a tiny command matrix table to one public doc.
   Good location:
   - `README.md` scaffold section
   Table columns:
   - task
   - installed binary
   - source workspace
   - run from
   Goal:
   - remove ambiguity without bloating the skill.

## Non-goals for this cycle

- Do not patch `plc-gen` first.
- Do not move source-internal architecture notes into public docs.
- Do not add new CLI subcommands just to paper over docs drift.

## Re-run condition

After P0 and P1 are done, rerun `skill-flywheel` on the same task and compare:

- whether the blind operator still synthesizes from multiple docs
- whether any command-context ambiguity remains
- whether any true `skill-gap` appears after the public surface is aligned
