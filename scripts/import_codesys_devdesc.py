#!/usr/bin/env python3
"""
Import one or more CODESYS `.devdesc.xml` files and emit a RustPLC controller profile.

This importer intentionally targets `devices/controllers/*.toml`, because that is the
current RustPLC entry point that can expand a port inventory directly into topology.

Supported source patterns:
- digital bitfields (`BitfieldType` + channel=input/output) -> `X*` / `Y*`
- scalar BOOL channels -> `X*` / `Y*`
- scalar numeric channels -> `AI*` / `AO*`

Typical usage:
  python scripts/import_codesys_devdesc.py \
    --profile-id codesys_rpi_gpio_ab \
    --identity-name "CODESYS Raspberry Pi GPIOs A/B" \
    --out devices/controllers/codesys_rpi_gpio_ab.toml \
    "https://forge.codesys.com/drv/rpi-legacy/code/1/tree/trunk/legacy/Devices/GPIOs.devdesc.xml?format=raw"

  python scripts/import_codesys_devdesc.py \
    --profile-id codesys_rpi_gpio_mcp3008_stack \
    --identity-name "CODESYS Raspberry Pi GPIO + MCP3008 Stack" \
    --out devices/controllers/codesys_rpi_gpio_mcp3008_stack.toml \
    "https://forge.codesys.com/drv/rpi-legacy/code/1/tree/trunk/legacy/Devices/GPIOs.devdesc.xml?format=raw" \
    "https://forge.codesys.com/drv/mcp3008/code/HEAD/tree/trunk/mcp3008/Devices/MCP3008.devdesc.xml?format=raw"
"""

from __future__ import annotations

import argparse
import re
import sys
import urllib.request
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Optional, Sequence, Tuple


NS = {"dd": "http://www.3s-software.com/schemas/DeviceDescription-1.0.xsd"}
NUMERIC_STD_TYPES = {
    "BYTE",
    "USINT",
    "UINT",
    "UDINT",
    "ULINT",
    "SINT",
    "INT",
    "DINT",
    "LINT",
    "REAL",
    "LREAL",
    "WORD",
    "DWORD",
    "LWORD",
}


@dataclass(frozen=True)
class ImportedPort:
    name: str
    direction: str
    port_type: str
    states: Tuple[str, ...] = ()
    default_state: str = ""


@dataclass
class ImportedDeviceTree:
    source: str
    name: str
    vendor: str
    description: str
    ports: List[ImportedPort]


def fetch_text(source: str) -> str:
    if re.match(r"^https?://", source, flags=re.IGNORECASE):
        with urllib.request.urlopen(source) as response:
            data = response.read()
        return data.decode("utf-8-sig")
    return Path(source).read_text(encoding="utf-8-sig")


def local_type_name(raw: str) -> str:
    if ":" in raw:
        return raw.split(":", 1)[1]
    return raw


def parse_text(elem: Optional[ET.Element]) -> str:
    if elem is None or elem.text is None:
        return ""
    return elem.text.strip()


def numeric_suffix(text: str) -> Optional[int]:
    match = re.search(r"(\d+)$", text)
    if not match:
        return None
    return int(match.group(1))


def bool_states_for_direction(direction: str) -> Tuple[Tuple[str, ...], str]:
    if direction == "input":
        return ("on", "off"), "off"
    return ("on", "off"), "off"


def infer_scalar_channel_kind(param_name: str, param_type: str) -> str:
    upper_type = local_type_name(param_type).upper()
    if upper_type == "BOOL":
        return "digital"

    lower_name = param_name.lower()
    if any(token in lower_name for token in ("button", "switch", "fault", "ready", "enable")):
        return "digital"

    if upper_type in NUMERIC_STD_TYPES:
        return "analog"

    return "generic"


def infer_named_port(
    param_name: str,
    channel: str,
    kind: str,
    next_digital: int,
    next_analog: int,
) -> Tuple[str, int, int]:
    if re.fullmatch(r"(X|Y|AI|AO|DI|DO)\d+", param_name):
        return param_name, next_digital, next_analog

    suffix = numeric_suffix(param_name)
    if kind == "digital":
        prefix = "X" if channel == "input" else "Y"
        if suffix is not None and re.fullmatch(r"(In|Out|Bit)\d+", param_name, flags=re.IGNORECASE):
            return f"{prefix}{suffix}", next_digital, next_analog
        return f"{prefix}{next_digital}", next_digital + 1, next_analog

    if kind == "analog":
        prefix = "AI" if channel == "input" else "AO"
        if suffix is not None and re.fullmatch(r"(In|Out|AI|AO)\d+", param_name, flags=re.IGNORECASE):
            return f"{prefix}{suffix}", next_digital, next_analog
        return f"{prefix}{next_analog}", next_digital, next_analog + 1

    prefix = "DI" if channel == "input" else "DO"
    return f"{prefix}{next_digital}", next_digital + 1, next_analog


def parse_bitfield_types(root: ET.Element) -> Dict[str, List[Tuple[str, str]]]:
    out: Dict[str, List[Tuple[str, str]]] = {}
    for bitfield in root.findall(".//dd:BitfieldType", NS):
        name = bitfield.attrib.get("name", "").strip()
        if not name:
            continue
        components = []
        for component in bitfield.findall("dd:Component", NS):
            identifier = component.attrib.get("identifier", "").strip()
            comp_type = component.attrib.get("type", "").strip()
            if not identifier:
                continue
            components.append((identifier, comp_type))
        out[name] = components
    return out


def parse_host_parameters(root: ET.Element) -> List[Tuple[str, str, str]]:
    params: List[Tuple[str, str, str]] = []
    for parameter in root.findall(".//dd:HostParameterSet/dd:Parameter", NS):
        attrs = parameter.find("dd:Attributes", NS)
        channel = attrs.attrib.get("channel", "none").strip() if attrs is not None else "none"
        if channel not in {"input", "output"}:
            continue

        param_type = parameter.attrib.get("type", "").strip()
        name = parse_text(parameter.find("dd:Name", NS))
        if not name:
            continue
        params.append((name, param_type, channel))
    return params


def parse_device_tree(source: str) -> ImportedDeviceTree:
    text = fetch_text(source)
    root = ET.fromstring(text)

    device = root.find("dd:Device", NS)
    if device is None:
        raise ValueError(f"{source}: missing <Device>")

    info = device.find("dd:DeviceInfo", NS)
    device_name = parse_text(info.find("dd:Name", NS) if info is not None else None) or "CODESYS Device"
    vendor = parse_text(info.find("dd:Vendor", NS) if info is not None else None)
    description = parse_text(info.find("dd:Description", NS) if info is not None else None)

    bitfield_types = parse_bitfield_types(root)
    ports: List[ImportedPort] = []
    used_names: Dict[str, ImportedPort] = {}
    next_digital = 0
    next_analog = 0

    for param_name, param_type, channel in parse_host_parameters(root):
        type_name = local_type_name(param_type)
        bitfield = bitfield_types.get(type_name)
        if bitfield:
            direction = "input" if channel == "input" else "output"
            prefix = "X" if direction == "input" else "Y"
            states, default_state = bool_states_for_direction(direction)

            ordered_components = sorted(
                bitfield,
                key=lambda item: (
                    numeric_suffix(item[0]) is None,
                    numeric_suffix(item[0]) or 0,
                    item[0],
                ),
            )
            for identifier, _component_type in ordered_components:
                bit_index = numeric_suffix(identifier)
                if bit_index is None:
                    bit_index = next_digital
                    next_digital += 1
                port = ImportedPort(
                    name=f"{prefix}{bit_index}",
                    direction=direction,
                    port_type="digital",
                    states=states,
                    default_state=default_state,
                )
                existing = used_names.get(port.name)
                if existing is None:
                    used_names[port.name] = port
                    ports.append(port)
                elif existing != port:
                    raise ValueError(
                        f"{source}: conflicting definitions for port {port.name}: {existing} vs {port}"
                    )
            continue

        kind = infer_scalar_channel_kind(param_name, param_type)
        direction = "input" if channel == "input" else "output"
        port_name, next_digital, next_analog = infer_named_port(
            param_name,
            channel,
            kind,
            next_digital=next_digital,
            next_analog=next_analog,
        )
        if kind == "digital":
            states, default_state = bool_states_for_direction(direction)
            port_type = "digital"
        elif kind == "analog":
            states, default_state = (), ""
            port_type = "analog"
        else:
            states, default_state = (), ""
            port_type = "generic"

        port = ImportedPort(
            name=port_name,
            direction=direction,
            port_type=port_type,
            states=states,
            default_state=default_state,
        )
        existing = used_names.get(port.name)
        if existing is None:
            used_names[port.name] = port
            ports.append(port)
        elif existing != port:
            raise ValueError(f"{source}: conflicting definitions for port {port.name}")

    ports.sort(key=port_sort_key)
    return ImportedDeviceTree(
        source=source,
        name=device_name,
        vendor=vendor,
        description=description,
        ports=ports,
    )


def port_sort_key(port: ImportedPort) -> Tuple[int, int, str]:
    match = re.fullmatch(r"([A-Z]+)(\d+)", port.name)
    if not match:
        return (99, 0, port.name)

    prefix, index_text = match.groups()
    order = {
        "X": 0,
        "Y": 1,
        "AI": 2,
        "AO": 3,
        "DI": 4,
        "DO": 5,
    }.get(prefix, 99)
    return (order, int(index_text), port.name)


def build_profile_text(
    profile_id: str,
    identity_name: str,
    imported: Sequence[ImportedDeviceTree],
) -> str:
    merged_ports: Dict[str, ImportedPort] = {}
    merged_list: List[ImportedPort] = []

    for item in imported:
        for port in item.ports:
            existing = merged_ports.get(port.name)
            if existing is None:
                merged_ports[port.name] = port
                merged_list.append(port)
                continue
            if existing != port:
                raise ValueError(f"conflicting merged port definition for {port.name}")

    merged_list.sort(key=port_sort_key)

    lines: List[str] = []
    lines.append(f"# generated by scripts/import_codesys_devdesc.py")
    lines.append(f"# profile_id = {profile_id}")
    for item in imported:
        summary = item.description or item.name
        vendor = f" [{item.vendor}]" if item.vendor else ""
        lines.append(f"# source: {item.source} :: {item.name}{vendor} :: {summary}")
    lines.append("")
    lines.append("[identity]")
    lines.append(f'name = "{identity_name}"')
    lines.append('type = "plc"')

    for port in merged_list:
        lines.append("")
        lines.append("[[interfaces.ports]]")
        lines.append(f'name = "{port.name}"')
        lines.append(f'direction = "{port.direction}"')
        lines.append(f'port_type = "{port.port_type}"')
        if port.states:
            rendered_states = ", ".join(f'"{value}"' for value in port.states)
            lines.append(f"states = [{rendered_states}]")
        if port.default_state:
            lines.append(f'default_state = "{port.default_state}"')

    lines.append("")
    return "\n".join(lines)


def default_identity_name(imported: Sequence[ImportedDeviceTree]) -> str:
    if len(imported) == 1:
        return f"CODESYS Imported {imported[0].name}"
    return "CODESYS Imported Composite Controller"


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Import CODESYS .devdesc.xml into a RustPLC controller profile.")
    parser.add_argument("sources", nargs="+", help="input .devdesc.xml path(s) or raw URL(s)")
    parser.add_argument("--profile-id", required=True, help="output profile id, e.g. codesys_mcp3008_adc8")
    parser.add_argument("--identity-name", help="human-readable controller profile name")
    parser.add_argument("--out", type=Path, help="write TOML to file instead of stdout")
    args = parser.parse_args(argv)

    imported = [parse_device_tree(source) for source in args.sources]
    identity_name = args.identity_name or default_identity_name(imported)
    profile_text = build_profile_text(args.profile_id, identity_name, imported)

    if args.out is not None:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(profile_text, encoding="utf-8")
    else:
        sys.stdout.write(profile_text)

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
