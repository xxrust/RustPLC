# Battery Module Pack Line Architecture

## Delivery Layers
- `line`: owns top layout, station handoff, global interlocks, and release evidence.
- `station`: owns local workpiece flow, module composition, and local fault boundaries.
- `module`: remains available for repeated mechanisms inside a station, but the first delivery closure here is the station layer.

## Station Map
- S01 `s01_tray_infeed_buffer`: positions `s01_mag_pick`, `s01_tray_scan`, `s01_buffer_align`, `s01_transfer_out`; modules: magazine elevator, tray shuttle, buffer align clamp, transfer shuttle.
- S02 `s02_cell_loading_alignment`: positions `s02_cell_pick`, `s02_orientation`, `s02_press_load`, `s02_transfer_out`; modules: cell picker, orientation spindle, load press nest, outfeed shuttle.
- S03 `s03_busbar_tab_prep`: positions `s03_busbar_pick`, `s03_tab_form`, `s03_prepress`, `s03_transfer_out`; modules: busbar picker, tab former, pre-press clamp, handoff shuttle.
- S04 `s04_laser_weld_cooling`: positions `s04_entry_clamp`, `s04_left_weld`, `s04_right_weld`, `s04_cool_out`; modules: entry clamp, dual weld head, cooling tunnel, outfeed shuttle.
- S05 `s05_leak_hipot_vision`: positions `s05_seal_load`, `s05_vacuum_test`, `s05_hipot_test`, `s05_vision_out`; modules: seal clamp pod, vacuum test manifold, hipot fixture, vision indexer.
- S06 `s06_label_packout_sort`: positions `s06_label_print`, `s06_qr_verify`, `s06_pack_insert`, `line_packout`; modules: label head, qr verify index, box pusher, sort diverter.

## Ownership Rules
- The line consumes station contracts and never binds intent directly to module-internal step names.
- The flattened compile surface keeps station ownership in fragment names such as `station_s01_*`.
- The first executable line path uses representative actuators from each station but preserves the full declared actuator inventory.

## Validation Strategy
- Validate the line asset bundle from `plc/deliveries/line/plc_gen_megapipeline/plc/main.bundle.toml`.
- Validate each station asset independently once the worker-authored station bundles are integrated.
