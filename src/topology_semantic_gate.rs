use crate::ast::{
    DeviceDeclaration, DevicePort, DeviceType, PortRole, PortType, TopologyConnection,
    TopologyRelation, TopologySection,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fmt;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TopologySemanticCode {
    #[serde(rename = "SEM-101")]
    Sem101PortNotFound,
    #[serde(rename = "SEM-102")]
    Sem102DirectionInvalid,
    #[serde(rename = "SEM-103")]
    Sem103TypeIncompatible,
    #[serde(rename = "SEM-104")]
    Sem104SemanticRoleIncompatible,
    #[serde(rename = "SEM-105")]
    Sem105DanglingPort,
}

impl TopologySemanticCode {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Sem101PortNotFound => "SEM-101",
            Self::Sem102DirectionInvalid => "SEM-102",
            Self::Sem103TypeIncompatible => "SEM-103",
            Self::Sem104SemanticRoleIncompatible => "SEM-104",
            Self::Sem105DanglingPort => "SEM-105",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TopologySemanticIssue {
    pub code: TopologySemanticCode,
    pub line: usize,
    pub relation: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub from_port: Option<String>,
    pub to_port: Option<String>,
    pub message: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TopologySemanticGateError {
    pub code: String,
    pub issues: Vec<TopologySemanticIssue>,
}

impl fmt::Display for TopologySemanticGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ERROR [{}] 拓扑语义门禁失败", self.code)?;
        for issue in &self.issues {
            writeln!(
                f,
                "  - [{}] <input>:{}:{}",
                issue.code.as_str(),
                issue.line.max(1),
                1
            )?;
            writeln!(f, "    原因: {}", issue.message)?;
            writeln!(f, "    建议: {}", issue.suggestion)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatePort {
    id: String,
    port_type: PortType,
    role: PortRole,
    semantic_role: Option<&'static str>,
    explicit: bool,
}

#[derive(Debug, Clone)]
struct GateDevice {
    ports: Vec<GatePort>,
}

#[derive(Debug, Clone)]
struct ResolvedPort {
    id: String,
    port_type: PortType,
    role: PortRole,
    semantic_role: Option<&'static str>,
}

pub fn validate_topology_semantics(
    topology: &TopologySection,
) -> Result<(), TopologySemanticGateError> {
    let mut devices = HashMap::<String, GateDevice>::new();
    for device in &topology.devices {
        devices.insert(
            device.name.clone(),
            GateDevice {
                ports: resolved_device_ports(device),
            },
        );
    }

    let mut issues = Vec::<TopologySemanticIssue>::new();
    let mut used_explicit_ports = HashSet::<(String, String)>::new();

    for connection in semantic_connections(topology) {
        let line = topology_connection_line(topology, &connection);
        let relation_name = relation_name(&connection.relation).to_string();

        let Some(from_device) = devices.get(&connection.from) else {
            issues.push(issue_sem101(
                line,
                &connection,
                format!("连接起点设备 `{}` 未定义", connection.from),
            ));
            continue;
        };
        let Some(to_device) = devices.get(&connection.to) else {
            issues.push(issue_sem101(
                line,
                &connection,
                format!("连接终点设备 `{}` 未定义", connection.to),
            ));
            continue;
        };

        let from_port = match resolve_port(
            &connection,
            &from_device.ports,
            true,
            line,
            &mut issues,
            &mut used_explicit_ports,
        ) {
            Some(port) => port,
            None => continue,
        };

        let to_port = match resolve_port(
            &connection,
            &to_device.ports,
            false,
            line,
            &mut issues,
            &mut used_explicit_ports,
        ) {
            Some(port) => port,
            None => continue,
        };

        let source_can_produce =
            matches!(from_port.role, PortRole::Producer | PortRole::Bidirectional);
        let target_can_consume =
            matches!(to_port.role, PortRole::Consumer | PortRole::Bidirectional);
        if !(source_can_produce && target_can_consume) {
            issues.push(TopologySemanticIssue {
                code: TopologySemanticCode::Sem102DirectionInvalid,
                line,
                relation: Some(relation_name.clone()),
                from: Some(connection.from.clone()),
                to: Some(connection.to.clone()),
                from_port: Some(from_port.id.clone()),
                to_port: Some(to_port.id.clone()),
                message: format!(
                    "关系 `{}` 方向错误：{}.{}/{} -> {}.{}/{}",
                    relation_name,
                    connection.from,
                    from_port.id,
                    role_name(&from_port.role),
                    connection.to,
                    to_port.id,
                    role_name(&to_port.role)
                ),
                suggestion: "请确保连线方向为 output(producer) -> input(consumer)".to_string(),
            });
        }

        if !relation_type_compatible(
            &connection.relation,
            &from_port.port_type,
            &to_port.port_type,
        ) {
            issues.push(TopologySemanticIssue {
                code: TopologySemanticCode::Sem103TypeIncompatible,
                line,
                relation: Some(relation_name.clone()),
                from: Some(connection.from.clone()),
                to: Some(connection.to.clone()),
                from_port: Some(from_port.id.clone()),
                to_port: Some(to_port.id.clone()),
                message: format!(
                    "关系 `{}` 端口类型不兼容：{}.{}/{} -> {}.{}/{}",
                    relation_name,
                    connection.from,
                    from_port.id,
                    port_type_name(&from_port.port_type),
                    connection.to,
                    to_port.id,
                    port_type_name(&to_port.port_type)
                ),
                suggestion: "请调整端口类型或关系类型，确保上下游类型匹配".to_string(),
            });
        }

        if matches!(connection.relation, TopologyRelation::Detects) {
            if let (Some(from_sem), Some(to_sem)) = (from_port.semantic_role, to_port.semantic_role)
            {
                if !(from_sem == "state" && to_sem == "detector") {
                    issues.push(TopologySemanticIssue {
                        code: TopologySemanticCode::Sem104SemanticRoleIncompatible,
                        line,
                        relation: Some(relation_name),
                        from: Some(connection.from.clone()),
                        to: Some(connection.to.clone()),
                        from_port: Some(from_port.id.clone()),
                        to_port: Some(to_port.id.clone()),
                        message: format!(
                            "detects 语义角色不兼容：源端口为 `{}`，目标端口为 `{}`",
                            from_sem, to_sem
                        ),
                        suggestion: "detects 关系应满足 state -> detector".to_string(),
                    });
                }
            }
        }
    }

    for device in &topology.devices {
        for port in &device.attributes.ports {
            if !used_explicit_ports.contains(&(device.name.clone(), port.id.clone())) {
                issues.push(TopologySemanticIssue {
                    code: TopologySemanticCode::Sem105DanglingPort,
                    line: device.line.max(1),
                    relation: None,
                    from: Some(device.name.clone()),
                    to: None,
                    from_port: Some(port.id.clone()),
                    to_port: None,
                    message: format!("端口 `{}` 已声明但未参与任何关系", port.id),
                    suggestion: "请补充关系连接，或删除未使用端口声明".to_string(),
                });
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(TopologySemanticGateError {
            code: "semantic_topology_invalid".to_string(),
            issues,
        })
    }
}

fn semantic_connections(topology: &TopologySection) -> Vec<TopologyConnection> {
    if !topology.connections.is_empty() {
        return topology.connections.clone();
    }

    let mut connections = Vec::new();
    for device in &topology.devices {
        if let Some(upstream) = device.attributes.driven_by.as_ref() {
            connections.push(TopologyConnection {
                from: upstream.clone(),
                to: device.name.clone(),
                relation: TopologyRelation::DrivenBy,
                from_port: None,
                to_port: None,
                signal: None,
            });
        }
        if let Some(target) = device.attributes.reports_to.as_ref() {
            connections.push(TopologyConnection {
                from: device.name.clone(),
                to: target.clone(),
                relation: TopologyRelation::ReportsTo,
                from_port: None,
                to_port: None,
                signal: None,
            });
        }
        if let Some(detects) = device.attributes.detects.as_ref() {
            connections.push(TopologyConnection {
                from: detects.device.clone(),
                to: device.name.clone(),
                relation: TopologyRelation::Detects,
                from_port: Some(detects.state.clone()),
                to_port: None,
                signal: Some(detects.state.clone()),
            });
        }
    }
    connections
}

fn resolved_device_ports(device: &DeviceDeclaration) -> Vec<GatePort> {
    if !device.attributes.ports.is_empty() {
        return device
            .attributes
            .ports
            .iter()
            .map(|port| GatePort {
                id: port.id.clone(),
                port_type: port.port_type.clone(),
                role: port.role.clone(),
                semantic_role: infer_semantic_role(port),
                explicit: true,
            })
            .collect();
    }

    implicit_ports_for_type(&device.device_type)
}

fn implicit_ports_for_type(device_type: &DeviceType) -> Vec<GatePort> {
    match device_type {
        DeviceType::DigitalOutput => vec![gate_port(
            "out",
            PortType::Digital,
            PortRole::Producer,
            Some("actuator_cmd"),
        )],
        DeviceType::DigitalInput => vec![gate_port(
            "in",
            PortType::Digital,
            PortRole::Consumer,
            Some("detector"),
        )],
        DeviceType::SolenoidValve => vec![
            gate_port(
                "coil",
                PortType::Digital,
                PortRole::Consumer,
                Some("actuator_cmd"),
            ),
            gate_port(
                "out",
                PortType::Pneumatic,
                PortRole::Producer,
                Some("state"),
            ),
        ],
        DeviceType::Cylinder => vec![
            gate_port(
                "cmd",
                PortType::Pneumatic,
                PortRole::Consumer,
                Some("actuator_cmd"),
            ),
            gate_port(
                "extended",
                PortType::Logical,
                PortRole::Producer,
                Some("state"),
            ),
            gate_port(
                "retracted",
                PortType::Logical,
                PortRole::Producer,
                Some("state"),
            ),
        ],
        DeviceType::Sensor => vec![
            gate_port(
                "sense",
                PortType::Logical,
                PortRole::Consumer,
                Some("detector"),
            ),
            gate_port("out", PortType::Digital, PortRole::Producer, Some("state")),
        ],
        DeviceType::Motor => vec![
            gate_port(
                "cmd",
                PortType::Digital,
                PortRole::Consumer,
                Some("actuator_cmd"),
            ),
            gate_port("on", PortType::Logical, PortRole::Producer, Some("state")),
        ],
        DeviceType::AnalogInput => vec![gate_port(
            "in",
            PortType::Analog,
            PortRole::Consumer,
            Some("detector"),
        )],
        DeviceType::AnalogOutput => vec![gate_port(
            "out",
            PortType::Analog,
            PortRole::Producer,
            Some("actuator_cmd"),
        )],
        DeviceType::Pid => vec![
            gate_port("in", PortType::Analog, PortRole::Consumer, None),
            gate_port("out", PortType::Analog, PortRole::Producer, None),
        ],
    }
}

fn gate_port(
    id: &'static str,
    port_type: PortType,
    role: PortRole,
    semantic_role: Option<&'static str>,
) -> GatePort {
    GatePort {
        id: id.to_string(),
        port_type,
        role,
        semantic_role,
        explicit: false,
    }
}

fn infer_semantic_role(port: &DevicePort) -> Option<&'static str> {
    let id = port.id.to_lowercase();
    match port.role {
        PortRole::Consumer => {
            if id.contains("sense") || id.contains("detector") || id == "in" {
                Some("detector")
            } else if id.contains("cmd") || id.contains("coil") || id.contains("power") {
                Some("actuator_cmd")
            } else {
                None
            }
        }
        PortRole::Producer => {
            if id.contains("state")
                || id.contains("feedback")
                || id.contains("extended")
                || id.contains("retracted")
                || id.contains("on")
            {
                Some("state")
            } else {
                None
            }
        }
        PortRole::Bidirectional => None,
    }
}

fn resolve_port(
    connection: &TopologyConnection,
    ports: &[GatePort],
    source_side: bool,
    line: usize,
    issues: &mut Vec<TopologySemanticIssue>,
    used_explicit_ports: &mut HashSet<(String, String)>,
) -> Option<ResolvedPort> {
    let requested = if source_side {
        connection.from_port.as_deref()
    } else {
        connection.to_port.as_deref()
    };

    if let Some(requested_id) = requested {
        if let Some(port) = ports.iter().find(|port| port.id == requested_id) {
            if port.explicit {
                let device = if source_side {
                    connection.from.clone()
                } else {
                    connection.to.clone()
                };
                used_explicit_ports.insert((device, port.id.clone()));
            }
            return Some(ResolvedPort {
                id: port.id.clone(),
                port_type: port.port_type.clone(),
                role: port.role.clone(),
                semantic_role: port.semantic_role,
            });
        }

        if source_side && matches!(connection.relation, TopologyRelation::Detects) {
            return Some(ResolvedPort {
                id: requested_id.to_string(),
                port_type: PortType::Logical,
                role: PortRole::Producer,
                semantic_role: Some("state"),
            });
        }

        issues.push(issue_sem101(
            line,
            connection,
            format!(
                "端口 `{}` 在设备 `{}` 上不存在",
                requested_id,
                if source_side {
                    &connection.from
                } else {
                    &connection.to
                }
            ),
        ));
        return None;
    }

    let candidates = ports
        .iter()
        .filter(|port| {
            if source_side {
                matches!(port.role, PortRole::Producer | PortRole::Bidirectional)
            } else {
                matches!(port.role, PortRole::Consumer | PortRole::Bidirectional)
            }
        })
        .collect::<Vec<_>>();

    if candidates.len() == 1 {
        let port = candidates[0];
        if port.explicit {
            let device = if source_side {
                connection.from.clone()
            } else {
                connection.to.clone()
            };
            used_explicit_ports.insert((device, port.id.clone()));
        }
        return Some(ResolvedPort {
            id: port.id.clone(),
            port_type: port.port_type.clone(),
            role: port.role.clone(),
            semantic_role: port.semantic_role,
        });
    }

    // Preserve direction diagnostics: when a node has exactly one declared port but it is on the
    // wrong side, bind it and let SEM-102 report the direction mismatch.
    if candidates.is_empty() && ports.len() == 1 {
        let port = &ports[0];
        if port.explicit {
            let device = if source_side {
                connection.from.clone()
            } else {
                connection.to.clone()
            };
            used_explicit_ports.insert((device, port.id.clone()));
        }
        return Some(ResolvedPort {
            id: port.id.clone(),
            port_type: port.port_type.clone(),
            role: port.role.clone(),
            semantic_role: port.semantic_role,
        });
    }

    issues.push(issue_sem101(
        line,
        connection,
        if candidates.is_empty() {
            format!(
                "设备 `{}` 缺少可用的 {}端口",
                if source_side {
                    &connection.from
                } else {
                    &connection.to
                },
                if source_side { "输出" } else { "输入" }
            )
        } else {
            format!(
                "设备 `{}` 的 {}端口不唯一，请显式指定 from_port/to_port",
                if source_side {
                    &connection.from
                } else {
                    &connection.to
                },
                if source_side { "输出" } else { "输入" }
            )
        },
    ));
    None
}

fn issue_sem101(
    line: usize,
    connection: &TopologyConnection,
    message: String,
) -> TopologySemanticIssue {
    TopologySemanticIssue {
        code: TopologySemanticCode::Sem101PortNotFound,
        line: line.max(1),
        relation: Some(relation_name(&connection.relation).to_string()),
        from: Some(connection.from.clone()),
        to: Some(connection.to.clone()),
        from_port: connection.from_port.clone(),
        to_port: connection.to_port.clone(),
        message,
        suggestion: "请补充并修正端口声明，确保关系端点都能解析到唯一端口".to_string(),
    }
}

fn relation_type_compatible(
    relation: &TopologyRelation,
    from_type: &PortType,
    to_type: &PortType,
) -> bool {
    match relation {
        TopologyRelation::DrivenBy => {
            from_type == to_type
                && matches!(
                    from_type,
                    PortType::Digital | PortType::Analog | PortType::Pneumatic
                )
        }
        TopologyRelation::ReportsTo => {
            from_type == to_type && matches!(from_type, PortType::Digital | PortType::Analog)
        }
        TopologyRelation::Detects => {
            from_type == to_type
                || matches!((from_type, to_type), (PortType::Logical, PortType::Digital))
        }
    }
}

fn relation_name(relation: &TopologyRelation) -> &'static str {
    match relation {
        TopologyRelation::DrivenBy => "driven_by",
        TopologyRelation::ReportsTo => "reports_to",
        TopologyRelation::Detects => "detects",
    }
}

fn role_name(role: &PortRole) -> &'static str {
    match role {
        PortRole::Producer => "producer",
        PortRole::Consumer => "consumer",
        PortRole::Bidirectional => "bidirectional",
    }
}

fn port_type_name(port_type: &PortType) -> &'static str {
    match port_type {
        PortType::Digital => "digital",
        PortType::Analog => "analog",
        PortType::Pneumatic => "pneumatic",
        PortType::Logical => "logical",
        PortType::Generic => "generic",
    }
}

fn topology_connection_line(topology: &TopologySection, connection: &TopologyConnection) -> usize {
    for device in &topology.devices {
        let matches_line = match connection.relation {
            TopologyRelation::DrivenBy => {
                device.name == connection.to
                    && device
                        .attributes
                        .driven_by
                        .as_deref()
                        .is_some_and(|from| from == connection.from)
            }
            TopologyRelation::ReportsTo => {
                device.name == connection.from
                    && device
                        .attributes
                        .reports_to
                        .as_deref()
                        .is_some_and(|to| to == connection.to)
            }
            TopologyRelation::Detects => {
                device.name == connection.to
                    && device
                        .attributes
                        .detects
                        .as_ref()
                        .is_some_and(|detects| detects.device == connection.from)
            }
        };
        if matches_line {
            return device.line.max(1);
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::validate_topology_semantics;
    use crate::parser::parse_plc;

    #[test]
    fn gate_rejects_two_cylinder_wrong_input_wiring() {
        let input = r#"
[topology]
device Y0: digital_output
device X4: digital_input
device start_button: digital_input { driven_by: X4 }

[constraints]

[tasks]
task main:
    step idle:
"#;
        let program = parse_plc(input).expect("parse");
        let err = validate_topology_semantics(&program.topology).expect_err("gate should fail");
        let codes = err
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"SEM-102"), "should report direction error");
    }

    #[test]
    fn gate_accepts_basic_digital_to_valve_to_cylinder_chain() {
        let input = r#"
[topology]
device Y0: digital_output
device valve_A: solenoid_valve { driven_by: Y0 }
device cyl_A: cylinder { driven_by: valve_A, stroke_time: 200ms, retract_time: 180ms }

[constraints]

[tasks]
task main:
    step idle:
"#;
        let program = parse_plc(input).expect("parse");
        validate_topology_semantics(&program.topology).expect("gate should pass");
    }
}
