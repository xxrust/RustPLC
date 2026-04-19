use rust_plc::parser::parse_plc;
use rust_plc::semantic::preprocess_program;

#[test]
fn codesys_rpi_gpio_profile_expands_digital_ports() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: codesys_rpi_gpio_ab }
device valve_a: solenoid_valve { ports: [coil:digital:consumer] }
device start_button: sensor { ports: [out:digital:producer] }

relation { from: plc_main.Y7, to: valve_a.coil, via: driven_by }
relation { from: start_button.out, to: plc_main.X5, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    let expanded = preprocess_program(&program).expect("preprocess");

    assert!(
        expanded.topology.devices.iter().any(|d| d.name == "X5"),
        "expected imported digital input X5 to expand into an internal IO node"
    );
    assert!(
        expanded.topology.devices.iter().any(|d| d.name == "Y7"),
        "expected imported digital output Y7 to expand into an internal IO node"
    );
}

#[test]
fn codesys_mcp3008_profile_expands_analog_ports() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: codesys_mcp3008_adc8 }
device pressure_sensor: analog_input { range: 0..1023 }

relation { from: pressure_sensor, to: plc_main.AI3, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    let expanded = preprocess_program(&program).expect("preprocess");

    assert!(
        expanded.topology.devices.iter().any(|d| d.name == "AI3"),
        "expected imported analog input AI3 to expand into an internal IO node"
    );
    assert!(expanded.topology.connections.iter().any(|c| {
        c.from == "pressure_sensor" && c.to == "AI3"
    }));
}

#[test]
fn codesys_composite_profile_merges_digital_and_analog_ports() {
    let input = r#"
[topology]
device plc_main: plc { model_ref: codesys_rpi_gpio_mcp3008_stack }
device alarm_light: solenoid_valve { ports: [coil:digital:consumer] }
device level_sensor: analog_input { range: 0..1023 }

relation { from: plc_main.Y1, to: alarm_light.coil, via: driven_by }
relation { from: level_sensor, to: plc_main.AI6, via: reports_to }

[constraints]

[tasks]
task main:
    step idle:
"#;

    let program = parse_plc(input).expect("parse");
    let expanded = preprocess_program(&program).expect("preprocess");

    assert!(expanded.topology.devices.iter().any(|d| d.name == "Y1"));
    assert!(expanded.topology.devices.iter().any(|d| d.name == "AI6"));
}
