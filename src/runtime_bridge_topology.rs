
struct TopologyResolver<'a> {
    topology: &'a TopologyGraph,
    by_name: HashMap<&'a str, NodeIndex>,
}

struct CylinderMotionResolution {
    target: &'static str,
    output: DigitalOutputId,
    confirm_inputs: &'static [DigitalInputId],
    opposing_inputs: &'static [DigitalInputId],
}

impl<'a> TopologyResolver<'a> {
    fn new(topology: &'a TopologyGraph) -> Self {
        let mut by_name = HashMap::new();
        for idx in topology.graph.node_indices() {
            let device = &topology.graph[idx];
            by_name.insert(device.name.as_str(), idx);
        }
        Self { topology, by_name }
    }

    fn resolve_digital_input_id(
        &self,
        state_name: &str,
        device: &str,
    ) -> Result<DigitalInputId, BridgeError> {
        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_input_physical_ids(start, DeviceKind::DigitalInput, parse_x_id);
        unique_physical_id(ids).map(DigitalInputId).map_err(|_| {
            BridgeError::UnresolvableDigitalInput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_digital_output_id(
        &self,
        state_name: &str,
        device: &str,
        port: &str,
    ) -> Result<DigitalOutputId, BridgeError> {
        if let Some(id) = self.resolve_digital_output_id_by_port(device, port) {
            return Ok(DigitalOutputId(id));
        }

        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_physical_ids(start, DeviceKind::DigitalOutput, parse_y_id);
        unique_physical_id(ids).map(DigitalOutputId).map_err(|_| {
            BridgeError::UnresolvableDigitalOutput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_analog_output_id(
        &self,
        state_name: &str,
        device: &str,
        port: &str,
    ) -> Result<AnalogOutputId, BridgeError> {
        if let Some(id) = self.resolve_analog_output_id_by_port(device, port) {
            return Ok(AnalogOutputId(id));
        }

        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_physical_ids(start, DeviceKind::AnalogOutput, parse_ao_id);
        unique_physical_id(ids).map(AnalogOutputId).map_err(|_| {
            BridgeError::UnresolvableAnalogOutput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_analog_input_id(
        &self,
        state_name: &str,
        device: &str,
    ) -> Result<AnalogInputId, BridgeError> {
        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;

        let ids = self.collect_input_physical_ids(start, DeviceKind::AnalogInput, parse_ai_id);
        unique_physical_id(ids).map(AnalogInputId).map_err(|_| {
            BridgeError::UnresolvableAnalogInput {
                state: state_name.to_string(),
                device: device.to_string(),
            }
        })
    }

    fn resolve_cylinder_motion(
        &self,
        state_name: &str,
        device: &str,
        port: &str,
        expect_extended: bool,
    ) -> Result<Option<CylinderMotionResolution>, BridgeError> {
        let start =
            self.by_name
                .get(device)
                .copied()
                .ok_or_else(|| BridgeError::UnknownDevice {
                    state: state_name.to_string(),
                    device: device.to_string(),
                })?;
        if self.topology.graph[start].kind != DeviceKind::Cylinder {
            return Ok(None);
        }

        let requested_port = state_port_key(
            port,
            if expect_extended {
                CylinderStrokeVerb::Extend.expected_state_port()
            } else {
                CylinderStrokeVerb::Retract.expected_state_port()
            },
        );
        let defined_state_ports = self.cylinder_detect_state_ports(device);
        if defined_state_ports.is_empty() {
            return Ok(None);
        }
        let confirm_ids = self.resolve_detect_state_input_ids(device, &requested_port);
        let opposing_port = cylinder_complementary_state_port(&requested_port).ok_or_else(|| {
            BridgeError::UnsupportedGuardExpression {
                state: state_name.to_string(),
                expression: format!(
                    "closed-loop cylinder action requires complementary end-state for {device}.{requested_port}"
                ),
            }
        })?;
        let opposing_ids = self.resolve_detect_state_input_ids(device, &opposing_port);
        if confirm_ids.is_empty() || opposing_ids.is_empty() {
            return Err(BridgeError::IncompleteClosedLoopCylinderMotion {
                state: state_name.to_string(),
                device: device.to_string(),
                requested_state: requested_port,
            });
        }

        Ok(Some(CylinderMotionResolution {
            target: Box::leak(device.to_string().into_boxed_str()),
            output: self.resolve_digital_output_id(state_name, device, port)?,
            confirm_inputs: leak_digital_input_ids(confirm_ids),
            opposing_inputs: leak_digital_input_ids(opposing_ids),
        }))
    }

    fn resolve_state_guard_instr(
        &self,
        state_name: &str,
        state_ref: &StateGuardRef,
        next: StepId,
        timeout: Option<Timeout>,
    ) -> Result<Instr<'static>, BridgeError> {
        let start = self
            .by_name
            .get(state_ref.device.as_str())
            .copied()
            .ok_or_else(|| BridgeError::UnknownDevice {
                state: state_name.to_string(),
                device: state_ref.device.clone(),
            })?;
        let device_kind = &self.topology.graph[start].kind;
        if *device_kind == DeviceKind::Cylinder {
            return Err(BridgeError::UnsupportedGuardExpression {
                state: state_name.to_string(),
                expression: format!("{}.{} == true", state_ref.device, state_ref.state),
            });
        }
        let requested_port = state_port_key(&state_ref.port, &state_ref.state);
        let target_ids = self.resolve_detect_state_input_ids(&state_ref.device, &requested_port);
        if target_ids.is_empty() {
            return Err(BridgeError::UnresolvableDigitalInput {
                state: state_name.to_string(),
                device: format!("{}.{}", state_ref.device, state_ref.state),
            });
        }

        let mut conditions = Vec::new();
        conditions.extend(target_ids.iter().copied().map(|id| DigitalCondition {
            id: DigitalInputId(id),
            equals: true,
        }));
        Ok(Instr::WaitAllDigital {
            conditions: leak_digital_conditions(conditions),
            next,
            timeout,
        })
    }

    fn axis_profile(&self, device: &str) -> Option<&crate::ir::AxisProfile> {
        self.topology.axis_profiles.get(device)
    }

    fn collect_physical_ids(
        &self,
        start: NodeIndex,
        kind: DeviceKind,
        parse: fn(&str) -> Option<u16>,
    ) -> Vec<u16> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut out = Vec::new();

        queue.push_back(start);
        visited.insert(start);

        while let Some(n) = queue.pop_front() {
            let device = &self.topology.graph[n];
            if device.kind == kind {
                if let Some(id) = parse(&device.name) {
                    out.push(id);
                }
            }
            for link in self.topology.links.iter().filter(|link| {
                link.to == device.name && matches_physical_output_kind(&kind, &link.kind)
            }) {
                if let Some(id) = parse_link_source_physical_id(link, parse) {
                    out.push(id);
                }
            }

            for pred in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Incoming)
            {
                if visited.insert(pred) {
                    queue.push_back(pred);
                }
            }
        }

        out
    }

    fn resolve_digital_output_id_by_port(&self, device: &str, port: &str) -> Option<u16> {
        let mut ids = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.to != device || link.kind != crate::ir::ConnectionType::Electrical {
                    return None;
                }
                if port != "self" && link.to_port.as_deref() != Some(port) {
                    return None;
                }
                parse_link_source_physical_id(link, parse_y_id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        match ids.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    fn resolve_analog_output_id_by_port(&self, device: &str, port: &str) -> Option<u16> {
        let mut ids = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.to != device || link.kind != crate::ir::ConnectionType::Analog {
                    return None;
                }
                if port != "self" && link.to_port.as_deref() != Some(port) {
                    return None;
                }
                parse_link_source_physical_id(link, parse_ao_id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        match ids.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    fn collect_input_physical_ids(
        &self,
        start: NodeIndex,
        kind: DeviceKind,
        parse: fn(&str) -> Option<u16>,
    ) -> Vec<u16> {
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut out = Vec::new();

        queue.push_back(start);
        visited.insert(start);

        while let Some(n) = queue.pop_front() {
            let device = &self.topology.graph[n];
            if device.kind == kind {
                if let Some(id) = parse(&device.name) {
                    out.push(id);
                }
            }
            for link in self.topology.links.iter().filter(|link| {
                link.from == device.name && matches_physical_input_kind(&kind, &link.kind)
            }) {
                if let Some(id) = parse_link_target_physical_id(link, parse) {
                    out.push(id);
                }
            }

            for pred in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Incoming)
            {
                let pred_kind = &self.topology.graph[pred].kind;
                if (*pred_kind == DeviceKind::Sensor || *pred_kind == kind) && visited.insert(pred)
                {
                    queue.push_back(pred);
                }
            }
            for succ in self
                .topology
                .graph
                .neighbors_directed(n, Direction::Outgoing)
            {
                let succ_kind = &self.topology.graph[succ].kind;
                if (*succ_kind == DeviceKind::Sensor || *succ_kind == kind) && visited.insert(succ)
                {
                    queue.push_back(succ);
                }
            }
        }

        out
    }

    fn resolve_detect_state_input_ids(&self, device: &str, state_port: &str) -> Vec<u16> {
        let mut ids = Vec::new();
        for sensor in self.detect_sensors_for_state_port(device, state_port) {
            ids.extend(self.sensor_reported_digital_input_ids(sensor));
        }
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn cylinder_detect_state_ports(&self, device: &str) -> Vec<String> {
        let mut state_ports = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.from != device || link.kind != crate::ir::ConnectionType::Logical {
                    return None;
                }
                let port = link.from_port.as_deref()?;
                is_cylinder_end_state_port(port).then(|| port.to_string())
            })
            .collect::<Vec<_>>();
        state_ports.sort();
        state_ports.dedup();
        state_ports
    }

    fn detect_sensors_for_state_port(&self, device: &str, state_port: &str) -> Vec<&str> {
        let mut sensors = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.from != device || link.kind != crate::ir::ConnectionType::Logical {
                    return None;
                }
                if !state_port_matches(link.from_port.as_deref(), state_port) {
                    return None;
                }
                Some(link.to.as_str())
            })
            .collect::<Vec<_>>();
        sensors.sort_unstable();
        sensors.dedup();
        sensors
    }

    fn sensor_reported_digital_input_ids(&self, sensor: &str) -> Vec<u16> {
        let mut ids = self
            .topology
            .links
            .iter()
            .filter_map(|link| {
                if link.from != sensor || link.kind != crate::ir::ConnectionType::Logical {
                    return None;
                }
                parse_link_target_physical_id(link, parse_x_id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        ids
    }

    fn sensor_is_cylinder_end_feedback(&self, sensor: &str) -> bool {
        self.topology.links.iter().any(|link| {
            link.to == sensor
                && link.kind == crate::ir::ConnectionType::Logical
                && link
                    .from_port
                    .as_deref()
                    .is_some_and(is_cylinder_end_state_port)
                && self
                    .by_name
                    .get(link.from.as_str())
                    .is_some_and(|idx| self.topology.graph[*idx].kind == DeviceKind::Cylinder)
        })
    }

    fn digital_input_is_cylinder_end_feedback(&self, id: DigitalInputId) -> bool {
        self.topology
            .graph
            .node_indices()
            .filter(|idx| self.topology.graph[*idx].kind == DeviceKind::Sensor)
            .map(|idx| self.topology.graph[idx].name.as_str())
            .filter(|sensor| self.sensor_is_cylinder_end_feedback(sensor))
            .any(|sensor| {
                self.sensor_reported_digital_input_ids(sensor)
                    .into_iter()
                    .any(|candidate| candidate == id.0)
            })
    }
}

fn leak_digital_input_ids(ids: Vec<u16>) -> &'static [DigitalInputId] {
    let leaked = ids
        .into_iter()
        .map(DigitalInputId)
        .collect::<Vec<DigitalInputId>>();
    Box::leak(leaked.into_boxed_slice())
}

fn leak_digital_conditions(conditions: Vec<DigitalCondition>) -> &'static [DigitalCondition] {
    Box::leak(conditions.into_boxed_slice())
}

fn state_port_matches(actual: Option<&str>, requested: &str) -> bool {
    matches!(actual, Some(port) if port == requested)
}

fn matches_physical_output_kind(kind: &DeviceKind, link_kind: &crate::ir::ConnectionType) -> bool {
    matches!(
        (kind, link_kind),
        (
            &DeviceKind::DigitalOutput,
            crate::ir::ConnectionType::Electrical
        ) | (&DeviceKind::AnalogOutput, crate::ir::ConnectionType::Analog)
    )
}

fn matches_physical_input_kind(kind: &DeviceKind, link_kind: &crate::ir::ConnectionType) -> bool {
    matches!(
        (kind, link_kind),
        (
            &DeviceKind::DigitalInput,
            crate::ir::ConnectionType::Logical
        ) | (&DeviceKind::AnalogInput, crate::ir::ConnectionType::Analog)
    )
}

fn parse_link_source_physical_id(
    link: &crate::ir::TopologyLink,
    parse: fn(&str) -> Option<u16>,
) -> Option<u16> {
    link.from_port
        .as_deref()
        .and_then(parse)
        .or_else(|| parse(&link.from))
}

fn parse_link_target_physical_id(
    link: &crate::ir::TopologyLink,
    parse: fn(&str) -> Option<u16>,
) -> Option<u16> {
    link.to_port
        .as_deref()
        .and_then(parse)
        .or_else(|| parse(&link.to))
}

fn unique_physical_id(mut ids: Vec<u16>) -> Result<u16, ()> {
    ids.sort_unstable();
    ids.dedup();
    match ids.len() {
        1 => Ok(ids[0]),
        _ => Err(()),
    }
}

fn parse_x_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::DigitalInput) => Some(port.id),
        _ => None,
    }
}

fn parse_y_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::DigitalOutput) => Some(port.id),
        _ => None,
    }
}

fn parse_ao_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::AnalogOutput) => Some(port.id),
        _ => None,
    }
}

fn parse_ai_id(name: &str) -> Option<u16> {
    match parse_physical_plc_port_ref(name) {
        Some(port) if matches!(port.kind, PlcPortKind::AnalogInput) => Some(port.id),
        _ => None,
    }
}
