# CODESYS Device Description Import

This import path is intentionally scoped to the part of CODESYS device trees that RustPLC can consume today with the least semantic distortion:

- controller-side port inventories
- digital input/output channels
- analog input/output channels

It does **not** try to import:

- visualization assets
- proprietary compiled libraries
- runtime driver code
- vendor-specific online configuration behavior

## Why controller profiles

RustPLC already has a stable expansion path for:

```plc
device plc_main: plc { model_ref: <profile_id> }
```

Those profiles live under `devices/controllers/*.toml` and are expanded by preprocess into internal IO nodes before semantic lowering.

That makes controller profiles the highest-value landing zone for imported CODESYS `.devdesc.xml` assets.

## Import script

Use:

```bash
python scripts/import_codesys_devdesc.py \
  --profile-id codesys_rpi_gpio_ab \
  --identity-name "CODESYS Raspberry Pi GPIOs A/B" \
  --out devices/controllers/codesys_rpi_gpio_ab.toml \
  "https://forge.codesys.com/drv/rpi-legacy/code/1/tree/trunk/legacy/Devices/GPIOs.devdesc.xml?format=raw"
```

The importer currently maps:

- scalar `channel="input"` + numeric types -> `AI*`
- scalar `channel="output"` + numeric types -> `AO*`
- scalar `BOOL` channels -> `X*` / `Y*`
- `BitfieldType` input/output channels -> expanded `X*` / `Y*`

## Imported profiles

Current checked-in profiles:

- `codesys_rpi_gpio_ab`
  - source: `GPIOs.devdesc.xml`
  - imported as `X0..X31` + `Y0..Y31`
- `codesys_mcp3008_adc8`
  - source: `MCP3008.devdesc.xml`
  - imported as `AI0..AI7`
- `codesys_rpi_gpio_mcp3008_stack`
  - composed from both source trees
  - imported as `X0..X31` + `Y0..Y31` + `AI0..AI7`

## Example

```plc
[topology]
device plc_main: plc { model_ref: codesys_rpi_gpio_mcp3008_stack }
device start_button: sensor { ports: [out:digital:producer] }
device lamp: solenoid_valve { ports: [coil:digital:consumer] }
device pressure_sensor: analog_input { range: 0..1023 }

relation { from: start_button.out, to: plc_main.X0, via: reports_to }
relation { from: plc_main.Y0, to: lamp.coil, via: driven_by }
relation { from: pressure_sensor, to: plc_main.AI0, via: reports_to }
```

## Current boundary

This is a port-tree import, not a full CODESYS runtime compatibility layer.

If we later want to import richer device trees as first-class topology modules instead of controller inventories, the next abstraction to add is a generic profile-backed IO/module device rather than overloading `plc`.
