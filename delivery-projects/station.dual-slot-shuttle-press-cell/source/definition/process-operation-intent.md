# Process Operation Scheduling Intent

## Operation Classes

| Class | Source | Destination/result | Required resources |
| --- | --- | --- | --- |
| `feed` | external finite supply | `raw_infeed` | upstream availability |
| `acquire` | `raw_infeed` | `load_nest` or load-owned token | `load_station_access` |
| `mount` | load-owned token | `shuttle_tray.slot[*]` | `load_station_access` |
| `move_to_press` | tray at load | tray at press | `shuttle_envelope` |
| `press` | mounted unpressed part | mounted pressed part | `shuttle_envelope` |
| `move_to_load` | tray at press | tray at load | `shuttle_envelope` |
| `unmount` | `shuttle_tray.slot[*]` | `load_nest` | `load_station_access` |
| `transfer` | `load_nest` | `good_outfeed` | `load_station_access` |
| `finish` | `good_outfeed` | terminal finished | none |

## Admission Rules

- `acquire`: source has exactly one selectable active token for the operation and the load-side destination is free.
- `mount`: selected concrete carrier slot is free.
- `move_to_press`: at least one mounted part is present, press cylinder is retracted, axis is not faulted, and `shuttle_envelope` is free.
- `press`: press position is proven, mounted part is present, and `shuttle_envelope` is free.
- `move_to_load`: press operation for every occupied slot is complete, cylinder is retracted, and resource is free.
- `unmount`: load position is proven and the selected slot contains a mounted part.
- `transfer/finish`: downstream capacity exists and the exact token location is unambiguous.

## Scheduling Policy

Use opportunistic slot admission. Slot 0 and slot 1 are symmetric candidates; written order does not create a business predecessor. Serialization is justified only by shared endpoint occupancy, `load_station_access`, `shuttle_envelope`, or an explicit predecessor above.

The authored TOML must exist before task/step flow and must be validated with `process-model-check`. OP-002 is a hard failure. OP-003 is either a hard failure or an explicitly evidenced current tool limitation; it cannot be silently ignored.
