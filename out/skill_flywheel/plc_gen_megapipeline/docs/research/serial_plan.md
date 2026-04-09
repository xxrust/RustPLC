# Serial Process Plan

## Product
- Workpiece: `battery_module_pack`
- Delivery asset: line `plc_gen_megapipeline`
- Planning principle: freeze one serial manufacturing route first, then allocate ownership to independently testable stations.

## Frozen Serial Route
1. `S01` receives trays and buffers incoming module packs.
2. `S02` loads cells into the tray and aligns the module body.
3. `S03` prepares busbars and tabs and completes the pre-press handoff.
4. `S04` clamps, welds, and cools the module.
5. `S05` performs leak, hipot, and vision inspection.
6. `S06` labels, packs, sorts, and finishes the workpiece.

## Why Start Serial
- It freezes one authoritative workpiece route before parallel station authoring begins.
- It gives every station a single upstream source and downstream sink.
- It prevents station teams from inventing incompatible handoff semantics.

## Parallel Authoring Rule
- Station delivery docs may be authored in parallel once the serial route, workpiece names, and actuator budgets are frozen.
- The line compile surface preserves ownership with `station_*` fragment naming.
- Station contracts must not depend on another station's internal step names.
