use crate::component_faults::{ComponentFaultEvent, ComponentFaultKind};
use crate::component_library::ComponentType;
use crate::component_scenario::{ComponentScenario, ComponentSensorEvent, ComponentSwitchEvent};
use crate::component_topology::ComponentTopology;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentFaultAuditEntry {
    pub schema_version: u32,
    pub tick: u64,
    pub at_ms: u64,
    pub action: String,
    pub event_index: usize,
    pub target_component_id: String,
    pub fault_kind: ComponentFaultKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentTraceRow {
    pub schema_version: u32,
    pub tick: u64,
    pub at_ms: u64,
    pub components: BTreeMap<String, ComponentTraceComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentTraceComponent {
    pub component_type: ComponentType,
    pub state: String,
    pub outputs: BTreeMap<String, Value>,
    pub inputs: BTreeMap<String, bool>,
    pub active_faults: Vec<ComponentFaultKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComponentSimReport {
    pub schema_version: u32,
    pub tick_ms: u64,
    pub duration_ms: u64,
    pub ticks: Vec<ComponentTraceRow>,
    pub fault_audit: Vec<ComponentFaultAuditEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComponentSimIssue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("component simulation failed")]
pub struct ComponentSimError {
    pub issues: Vec<ComponentSimIssue>,
}

pub fn run_component_simulation(
    topology: &ComponentTopology,
    scenario: &ComponentScenario,
) -> Result<ComponentSimReport, ComponentSimError> {
    let mut issues = Vec::new();
    let model = ComponentModel::from_topology(topology, &mut issues);
    validate_scenario_targets(&model, scenario, &mut issues);
    if !issues.is_empty() {
        return Err(ComponentSimError { issues });
    }

    let mut runtime =
        ComponentRuntime::new(model, scenario.tick_ms, scenario.component_faults.clone());
    let switch_by_tick = index_switch_events(&scenario.switch_events, scenario.tick_ms);
    let sensor_by_tick = index_sensor_events(&scenario.sensor_events, scenario.tick_ms);
    let total_ticks = if scenario.tick_ms == 0 {
        0
    } else {
        scenario.duration_ms / scenario.tick_ms
    };

    let mut rows = Vec::new();
    for tick in 0..total_ticks {
        let at_ms = tick.saturating_mul(scenario.tick_ms);
        runtime.apply_tick(tick, switch_by_tick.get(&tick), sensor_by_tick.get(&tick));
        rows.push(runtime.snapshot_row(tick, at_ms));
    }

    Ok(ComponentSimReport {
        schema_version: 1,
        tick_ms: scenario.tick_ms,
        duration_ms: scenario.duration_ms,
        ticks: rows,
        fault_audit: runtime.fault_audit,
    })
}

fn validate_scenario_targets(
    model: &ComponentModel,
    scenario: &ComponentScenario,
    issues: &mut Vec<ComponentSimIssue>,
) {
    for (idx, event) in scenario.switch_events.iter().enumerate() {
        match model.instances.get(&event.target) {
            None => issues.push(issue(
                "CSIM-TGT-001",
                format!("$.switch_events[{idx}].target"),
                format!("unknown switch target `{}`", event.target),
            )),
            Some(instance) if instance.component_type != ComponentType::Switch => {
                issues.push(issue(
                    "CSIM-TGT-002",
                    format!("$.switch_events[{idx}].target"),
                    format!(
                        "target `{}` is `{}`; switch_events require a `switch` instance",
                        event.target,
                        component_type_label(instance.component_type)
                    ),
                ))
            }
            Some(_) => {}
        }
    }
    for (idx, event) in scenario.sensor_events.iter().enumerate() {
        match model.instances.get(&event.target) {
            None => issues.push(issue(
                "CSIM-TGT-003",
                format!("$.sensor_events[{idx}].target"),
                format!("unknown sensor target `{}`", event.target),
            )),
            Some(instance) if instance.component_type != ComponentType::Sensor => {
                issues.push(issue(
                    "CSIM-TGT-004",
                    format!("$.sensor_events[{idx}].target"),
                    format!(
                        "target `{}` is `{}`; sensor_events require a `sensor` instance",
                        event.target,
                        component_type_label(instance.component_type)
                    ),
                ))
            }
            Some(_) => {}
        }
    }
    for (idx, fault) in scenario.component_faults.iter().enumerate() {
        let Some(instance) = model.instances.get(&fault.target_component_id) else {
            issues.push(issue(
                "CSIM-TGT-005",
                format!("$.component_faults[{idx}].target_component_id"),
                format!("unknown component target `{}`", fault.target_component_id),
            ));
            continue;
        };
        if !fault_allowed_on_type(instance.component_type, fault.fault_kind) {
            issues.push(issue(
                "CSIM-TGT-006",
                format!("$.component_faults[{idx}].fault_kind"),
                format!(
                    "fault_kind `{}` is not supported on `{}` instance `{}`",
                    fault_kind_label(fault.fault_kind),
                    component_type_label(instance.component_type),
                    fault.target_component_id
                ),
            ));
        }
    }
}

fn fault_allowed_on_type(component_type: ComponentType, fault_kind: ComponentFaultKind) -> bool {
    match component_type {
        ComponentType::Cylinder => {
            matches!(
                fault_kind,
                ComponentFaultKind::Jammed | ComponentFaultKind::MotionTimeout
            )
        }
        ComponentType::Sensor => matches!(
            fault_kind,
            ComponentFaultKind::StuckOn
                | ComponentFaultKind::StuckOff
                | ComponentFaultKind::Chatter
        ),
        ComponentType::Switch => {
            matches!(
                fault_kind,
                ComponentFaultKind::StuckOn | ComponentFaultKind::StuckOff
            )
        }
        ComponentType::StepperPd => matches!(
            fault_kind,
            ComponentFaultKind::LostStep
                | ComponentFaultKind::Stall
                | ComponentFaultKind::DirectionReversed
        ),
    }
}

#[derive(Debug, Clone)]
struct InstanceModel {
    component_type: ComponentType,
    params: BTreeMap<String, Value>,
}

#[derive(Debug, Clone)]
struct ConnectionModel {
    from_instance: String,
    from_port: String,
    to_instance: String,
    to_port: String,
}

#[derive(Debug, Clone)]
struct ComponentModel {
    instances: BTreeMap<String, InstanceModel>,
    connections: Vec<ConnectionModel>,
}

impl ComponentModel {
    fn from_topology(topology: &ComponentTopology, issues: &mut Vec<ComponentSimIssue>) -> Self {
        let mut definitions = BTreeMap::<String, (ComponentType, BTreeMap<String, Value>)>::new();
        for definition in &topology.component_library.components {
            definitions.insert(
                definition.id.clone(),
                (definition.component_type, definition.params.clone()),
            );
        }

        let mut instances = BTreeMap::<String, InstanceModel>::new();
        for (idx, instance) in topology.components.iter().enumerate() {
            let Some((component_type, base_params)) =
                definitions.get(&instance.component_id).cloned()
            else {
                issues.push(issue(
                    "CSIM-MODEL-001",
                    format!("$.components[{idx}].component_id"),
                    format!(
                        "component_id `{}` is not defined in component_library",
                        instance.component_id
                    ),
                ));
                continue;
            };
            let mut merged_params = base_params;
            for (key, value) in &instance.params {
                merged_params.insert(key.clone(), value.clone());
            }
            instances.insert(
                instance.id.clone(),
                InstanceModel {
                    component_type,
                    params: merged_params,
                },
            );
        }

        let mut connections = Vec::new();
        for (idx, connection) in topology.connections.iter().enumerate() {
            let Some((from_instance, from_port)) = connection.from.split_once('.') else {
                issues.push(issue(
                    "CSIM-MODEL-002",
                    format!("$.connections[{idx}].from"),
                    "from endpoint must use `<instance>.<port>` format",
                ));
                continue;
            };
            let Some((to_instance, to_port)) = connection.to.split_once('.') else {
                issues.push(issue(
                    "CSIM-MODEL-003",
                    format!("$.connections[{idx}].to"),
                    "to endpoint must use `<instance>.<port>` format",
                ));
                continue;
            };
            connections.push(ConnectionModel {
                from_instance: from_instance.to_string(),
                from_port: from_port.to_string(),
                to_instance: to_instance.to_string(),
                to_port: to_port.to_string(),
            });
        }

        Self {
            instances,
            connections,
        }
    }
}

#[derive(Debug, Clone)]
struct ComponentRuntime {
    model: ComponentModel,
    instances: BTreeMap<String, RuntimeInstance>,
    outputs: BTreeMap<(String, String), bool>,
    position_outputs: BTreeMap<(String, String), i64>,
    fault_events: Vec<ScheduledFaultEvent>,
    active_fault_indices: BTreeSet<usize>,
    fault_audit: Vec<ComponentFaultAuditEntry>,
    tick_ms: u64,
}

impl ComponentRuntime {
    fn new(model: ComponentModel, tick_ms: u64, faults: Vec<ComponentFaultEvent>) -> Self {
        let instances = model
            .instances
            .iter()
            .map(|(id, instance)| {
                (
                    id.clone(),
                    RuntimeInstance::new(instance.component_type, &instance.params),
                )
            })
            .collect::<BTreeMap<_, _>>();

        let mut runtime = Self {
            model,
            instances,
            outputs: BTreeMap::new(),
            position_outputs: BTreeMap::new(),
            fault_events: to_scheduled_faults(faults, tick_ms),
            active_fault_indices: BTreeSet::new(),
            fault_audit: Vec::new(),
            tick_ms,
        };
        runtime.refresh_outputs();
        runtime
    }

    fn apply_tick(
        &mut self,
        tick: u64,
        switch_events: Option<&Vec<ComponentSwitchEvent>>,
        sensor_events: Option<&Vec<ComponentSensorEvent>>,
    ) {
        let at_ms = tick.saturating_mul(self.tick_ms);
        if let Some(events) = switch_events {
            for event in events {
                if let Some(instance) = self.instances.get_mut(&event.target) {
                    instance.set_external_bool(event.value);
                }
            }
        }
        if let Some(events) = sensor_events {
            for event in events {
                if let Some(instance) = self.instances.get_mut(&event.target) {
                    instance.set_external_bool(event.value);
                }
            }
        }

        let active_now = self.active_faults_for_tick(tick);
        for index in active_now.difference(&self.active_fault_indices) {
            let event = &self.fault_events[*index];
            self.fault_audit.push(ComponentFaultAuditEntry {
                schema_version: 1,
                tick,
                at_ms,
                action: "activated".to_string(),
                event_index: event.index,
                target_component_id: event.event.target_component_id.clone(),
                fault_kind: event.event.fault_kind,
            });
        }
        for index in self.active_fault_indices.difference(&active_now) {
            let event = &self.fault_events[*index];
            self.fault_audit.push(ComponentFaultAuditEntry {
                schema_version: 1,
                tick,
                at_ms,
                action: "expired".to_string(),
                event_index: event.index,
                target_component_id: event.event.target_component_id.clone(),
                fault_kind: event.event.fault_kind,
            });
        }
        self.active_fault_indices = active_now;

        let input_signals = self.resolve_inputs();
        let active_by_component = self.active_faults_grouped_by_component(tick);

        for (id, runtime) in &mut self.instances {
            let effect = active_by_component.get(id).cloned().unwrap_or_default();
            let inputs = input_signals.get(id).cloned().unwrap_or_default();
            runtime.apply_tick(inputs, effect, tick, self.tick_ms);
        }

        self.refresh_outputs();
    }

    fn snapshot_row(&self, tick: u64, at_ms: u64) -> ComponentTraceRow {
        let active_by_component = self.active_faults_grouped_by_component(tick);
        let mut components = BTreeMap::new();
        let input_signals = self.resolve_inputs();
        for (id, runtime) in &self.instances {
            let active_faults = active_by_component
                .get(id)
                .map(|profile| profile.kinds.clone())
                .unwrap_or_default();
            let inputs = input_signals.get(id).cloned().unwrap_or_default();
            components.insert(id.clone(), runtime.snapshot(active_faults, inputs));
        }

        ComponentTraceRow {
            schema_version: 1,
            tick,
            at_ms,
            components,
        }
    }

    fn active_faults_for_tick(&self, tick: u64) -> BTreeSet<usize> {
        self.fault_events
            .iter()
            .enumerate()
            .filter_map(|(idx, event)| {
                if tick < event.start_tick {
                    return None;
                }
                if let Some(end_tick_exclusive) = event.end_tick_exclusive {
                    if tick >= end_tick_exclusive {
                        return None;
                    }
                }
                Some(idx)
            })
            .collect()
    }

    fn active_faults_grouped_by_component(&self, tick: u64) -> HashMap<String, FaultProfile> {
        let mut by_component = HashMap::<String, Vec<&ScheduledFaultEvent>>::new();
        for event in &self.fault_events {
            if tick < event.start_tick {
                continue;
            }
            if let Some(end_tick_exclusive) = event.end_tick_exclusive {
                if tick >= end_tick_exclusive {
                    continue;
                }
            }
            by_component
                .entry(event.event.target_component_id.clone())
                .or_default()
                .push(event);
        }

        let mut out = HashMap::new();
        for (component, active) in by_component {
            out.insert(
                component,
                FaultProfile::from_active_events(active, tick, self.tick_ms),
            );
        }
        out
    }

    fn resolve_inputs(&self) -> BTreeMap<String, BTreeMap<String, bool>> {
        let mut inputs = BTreeMap::<String, BTreeMap<String, bool>>::new();
        for connection in &self.model.connections {
            let value = self
                .outputs
                .get(&(
                    connection.from_instance.clone(),
                    connection.from_port.clone(),
                ))
                .copied()
                .unwrap_or(false);
            inputs
                .entry(connection.to_instance.clone())
                .or_default()
                .insert(connection.to_port.clone(), value);
        }
        inputs
    }

    fn refresh_outputs(&mut self) {
        self.outputs.clear();
        self.position_outputs.clear();
        for (id, runtime) in &self.instances {
            for (port, value) in runtime.bool_outputs() {
                self.outputs.insert((id.clone(), port), value);
            }
            if let Some(position_steps) = runtime.stepper_position_output() {
                self.position_outputs
                    .insert((id.clone(), "position_steps".to_string()), position_steps);
            }
        }
    }
}

fn to_scheduled_faults(faults: Vec<ComponentFaultEvent>, tick_ms: u64) -> Vec<ScheduledFaultEvent> {
    faults
        .into_iter()
        .enumerate()
        .map(|(idx, event)| {
            let start_tick = if tick_ms == 0 {
                0
            } else {
                event.at_ms / tick_ms
            };
            let end_tick_exclusive = event.duration_ms.map(|duration_ms| {
                if tick_ms == 0 {
                    0
                } else {
                    start_tick.saturating_add(duration_ms.div_ceil(tick_ms))
                }
            });
            ScheduledFaultEvent {
                index: idx,
                start_tick,
                end_tick_exclusive,
                event,
            }
        })
        .collect()
}

fn index_switch_events(
    events: &[ComponentSwitchEvent],
    tick_ms: u64,
) -> HashMap<u64, Vec<ComponentSwitchEvent>> {
    let mut out = HashMap::<u64, Vec<ComponentSwitchEvent>>::new();
    for event in events {
        let tick = if tick_ms == 0 {
            0
        } else {
            event.at_ms / tick_ms
        };
        out.entry(tick).or_default().push(event.clone());
    }
    out
}

fn index_sensor_events(
    events: &[ComponentSensorEvent],
    tick_ms: u64,
) -> HashMap<u64, Vec<ComponentSensorEvent>> {
    let mut out = HashMap::<u64, Vec<ComponentSensorEvent>>::new();
    for event in events {
        let tick = if tick_ms == 0 {
            0
        } else {
            event.at_ms / tick_ms
        };
        out.entry(tick).or_default().push(event.clone());
    }
    out
}

#[derive(Debug, Clone)]
struct ScheduledFaultEvent {
    index: usize,
    start_tick: u64,
    end_tick_exclusive: Option<u64>,
    event: ComponentFaultEvent,
}

#[derive(Debug, Clone, Default)]
struct FaultProfile {
    kinds: Vec<ComponentFaultKind>,
    stuck_value: Option<bool>,
    chatter: Option<ChatterFault>,
    jammed: bool,
    motion_timeout_ticks: Option<u64>,
    stall: bool,
    direction_reversed: bool,
    lost_step_ratio: Option<f64>,
}

#[derive(Debug, Clone)]
struct ChatterFault {
    start_tick: u64,
    period_ticks: u64,
    active_ticks: u64,
}

impl FaultProfile {
    fn from_active_events(events: Vec<&ScheduledFaultEvent>, tick: u64, tick_ms: u64) -> Self {
        let mut profile = Self::default();
        profile.kinds = events.iter().map(|event| event.event.fault_kind).collect();
        profile.kinds.sort_by_key(|kind| fault_kind_priority(*kind));

        let mut bool_faults = events
            .iter()
            .filter_map(|event| match event.event.fault_kind {
                ComponentFaultKind::StuckOff => Some((3_u8, Some(false), None)),
                ComponentFaultKind::StuckOn => Some((2_u8, Some(true), None)),
                ComponentFaultKind::Chatter => {
                    let period_ms = event
                        .event
                        .params
                        .get("period_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(1);
                    let duty = event
                        .event
                        .params
                        .get("duty_percent")
                        .and_then(Value::as_u64)
                        .unwrap_or(50)
                        .clamp(1, 99);
                    let period_ticks = period_ms.div_ceil(tick_ms.max(1)).max(1);
                    let active_ticks = (period_ticks * duty).div_ceil(100).max(1);
                    Some((
                        1_u8,
                        None,
                        Some(ChatterFault {
                            start_tick: event.start_tick,
                            period_ticks,
                            active_ticks,
                        }),
                    ))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        bool_faults.sort_by(|a, b| b.0.cmp(&a.0));
        if let Some((_, stuck, chatter)) = bool_faults.into_iter().next() {
            profile.stuck_value = stuck;
            profile.chatter = chatter;
        }

        for event in &events {
            match event.event.fault_kind {
                ComponentFaultKind::Jammed => profile.jammed = true,
                ComponentFaultKind::MotionTimeout => {
                    let timeout_ms = event
                        .event
                        .params
                        .get("timeout_ms")
                        .and_then(Value::as_u64)
                        .unwrap_or(1);
                    let timeout_ticks = timeout_ms.max(1);
                    profile.motion_timeout_ticks = match profile.motion_timeout_ticks {
                        Some(existing) => Some(existing.min(timeout_ticks)),
                        None => Some(timeout_ticks),
                    };
                }
                ComponentFaultKind::Stall => profile.stall = true,
                ComponentFaultKind::DirectionReversed => profile.direction_reversed = true,
                ComponentFaultKind::LostStep => {
                    let ratio = event
                        .event
                        .params
                        .get("ratio")
                        .and_then(Value::as_f64)
                        .unwrap_or(1.0)
                        .clamp(0.0, 1.0);
                    profile.lost_step_ratio = match profile.lost_step_ratio {
                        Some(existing) => Some(existing.min(ratio)),
                        None => Some(ratio),
                    };
                }
                ComponentFaultKind::StuckOn
                | ComponentFaultKind::StuckOff
                | ComponentFaultKind::Chatter => {}
            }
        }

        if let Some(chatter) = profile.chatter.as_mut() {
            if chatter.period_ticks == 0 {
                chatter.period_ticks = 1;
            }
            if chatter.active_ticks == 0 {
                chatter.active_ticks = 1;
            }
            if chatter.active_ticks > chatter.period_ticks {
                chatter.active_ticks = chatter.period_ticks;
            }
            chatter.start_tick = chatter.start_tick.min(tick);
        }

        profile
    }
}

fn fault_kind_priority(kind: ComponentFaultKind) -> u8 {
    match kind {
        ComponentFaultKind::Jammed => 90,
        ComponentFaultKind::MotionTimeout => 80,
        ComponentFaultKind::Stall => 70,
        ComponentFaultKind::DirectionReversed => 60,
        ComponentFaultKind::LostStep => 50,
        ComponentFaultKind::StuckOff => 40,
        ComponentFaultKind::StuckOn => 30,
        ComponentFaultKind::Chatter => 20,
    }
}

#[derive(Debug, Clone)]
enum RuntimeInstance {
    Switch(SwitchState),
    Sensor(SensorState),
    Cylinder(CylinderState),
    Stepper(StepperState),
}

impl RuntimeInstance {
    fn new(component_type: ComponentType, params: &BTreeMap<String, Value>) -> Self {
        match component_type {
            ComponentType::Switch => Self::Switch(SwitchState {
                external_state: false,
                state: false,
            }),
            ComponentType::Sensor => Self::Sensor(SensorState {
                external_state: false,
                state: false,
            }),
            ComponentType::Cylinder => Self::Cylinder(CylinderState::new(params)),
            ComponentType::StepperPd => Self::Stepper(StepperState::new()),
        }
    }

    fn set_external_bool(&mut self, value: bool) {
        match self {
            Self::Switch(state) => state.external_state = value,
            Self::Sensor(state) => state.external_state = value,
            Self::Cylinder(_) | Self::Stepper(_) => {}
        }
    }

    fn apply_tick(
        &mut self,
        inputs: BTreeMap<String, bool>,
        fault: FaultProfile,
        tick: u64,
        tick_ms: u64,
    ) {
        match self {
            Self::Switch(state) => state.apply(inputs, fault, tick),
            Self::Sensor(state) => state.apply(inputs, fault, tick),
            Self::Cylinder(state) => state.apply(inputs, fault, tick_ms),
            Self::Stepper(state) => state.apply(inputs, fault),
        }
    }

    fn bool_outputs(&self) -> Vec<(String, bool)> {
        match self {
            Self::Switch(state) => vec![("state".to_string(), state.state)],
            Self::Sensor(state) => vec![("state".to_string(), state.state)],
            Self::Cylinder(state) => vec![
                (
                    "sensor_extended".to_string(),
                    state.position_ticks >= state.stroke_ticks,
                ),
                ("sensor_retracted".to_string(), state.position_ticks == 0),
                ("state".to_string(), state.position_ticks > 0),
            ],
            Self::Stepper(state) => vec![("alarm".to_string(), state.alarm)],
        }
    }

    fn stepper_position_output(&self) -> Option<i64> {
        match self {
            Self::Stepper(state) => Some(state.position_steps),
            _ => None,
        }
    }

    fn snapshot(
        &self,
        active_faults: Vec<ComponentFaultKind>,
        inputs: BTreeMap<String, bool>,
    ) -> ComponentTraceComponent {
        match self {
            Self::Switch(state) => {
                let outputs = bool_output_map(vec![("state".to_string(), state.state)]);
                ComponentTraceComponent {
                    component_type: ComponentType::Switch,
                    state: if state.state { "on" } else { "off" }.to_string(),
                    outputs,
                    inputs,
                    active_faults,
                }
            }
            Self::Sensor(state) => {
                let outputs = bool_output_map(vec![("state".to_string(), state.state)]);
                ComponentTraceComponent {
                    component_type: ComponentType::Sensor,
                    state: if state.state { "on" } else { "off" }.to_string(),
                    outputs,
                    inputs,
                    active_faults,
                }
            }
            Self::Cylinder(state) => {
                let mut outputs = bool_output_map(vec![
                    (
                        "sensor_extended".to_string(),
                        state.position_ticks >= state.stroke_ticks,
                    ),
                    ("sensor_retracted".to_string(), state.position_ticks == 0),
                ]);
                outputs.insert(
                    "position_ticks".to_string(),
                    Value::from(state.position_ticks),
                );
                outputs.insert("timed_out".to_string(), Value::from(state.timed_out));
                ComponentTraceComponent {
                    component_type: ComponentType::Cylinder,
                    state: state.motion_state_label().to_string(),
                    outputs,
                    inputs,
                    active_faults,
                }
            }
            Self::Stepper(state) => {
                let mut outputs = Map::new();
                outputs.insert(
                    "position_steps".to_string(),
                    Value::from(state.position_steps),
                );
                outputs.insert("direction".to_string(), Value::from(state.direction));
                outputs.insert("alarm".to_string(), Value::from(state.alarm));
                ComponentTraceComponent {
                    component_type: ComponentType::StepperPd,
                    state: if state.enable { "enabled" } else { "disabled" }.to_string(),
                    outputs: outputs.into_iter().collect(),
                    inputs,
                    active_faults,
                }
            }
        }
    }
}

fn bool_output_map(entries: Vec<(String, bool)>) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(name, value)| (name, Value::from(value)))
        .collect()
}

#[derive(Debug, Clone)]
struct SwitchState {
    external_state: bool,
    state: bool,
}

impl SwitchState {
    fn apply(&mut self, _inputs: BTreeMap<String, bool>, fault: FaultProfile, tick: u64) {
        self.state = apply_bool_fault(self.external_state, &fault, tick);
    }
}

#[derive(Debug, Clone)]
struct SensorState {
    external_state: bool,
    state: bool,
}

impl SensorState {
    fn apply(&mut self, _inputs: BTreeMap<String, bool>, fault: FaultProfile, tick: u64) {
        self.state = apply_bool_fault(self.external_state, &fault, tick);
    }
}

#[derive(Debug, Clone)]
struct CylinderState {
    stroke_ticks: u64,
    position_ticks: u64,
    in_motion_ticks: u64,
    timed_out: bool,
    motion_state: CylinderMotionState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CylinderMotionState {
    Retracted,
    Extended,
    MovingExtend,
    MovingRetract,
    StoppedMid,
}

impl CylinderState {
    fn new(params: &BTreeMap<String, Value>) -> Self {
        let stroke_ticks = params
            .get("stroke_ticks")
            .and_then(Value::as_u64)
            .unwrap_or(4)
            .max(1);
        Self {
            stroke_ticks,
            position_ticks: 0,
            in_motion_ticks: 0,
            timed_out: false,
            motion_state: CylinderMotionState::Retracted,
        }
    }

    fn apply(&mut self, inputs: BTreeMap<String, bool>, fault: FaultProfile, _tick_ms: u64) {
        let cmd_extend = *inputs.get("cmd_extend").unwrap_or(&false);
        let cmd_retract = *inputs.get("cmd_retract").unwrap_or(&false);
        let mut moving = false;
        self.timed_out = false;

        if !fault.jammed {
            if cmd_extend && !cmd_retract {
                if self.position_ticks < self.stroke_ticks {
                    self.position_ticks = self.position_ticks.saturating_add(1);
                    moving = true;
                }
            } else if cmd_retract && !cmd_extend {
                if self.position_ticks > 0 {
                    self.position_ticks = self.position_ticks.saturating_sub(1);
                    moving = true;
                }
            }
        }

        if moving {
            self.in_motion_ticks = self.in_motion_ticks.saturating_add(1);
            if let Some(timeout_ticks) = fault.motion_timeout_ticks {
                if self.in_motion_ticks > timeout_ticks {
                    self.timed_out = true;
                    moving = false;
                }
            }
        } else {
            self.in_motion_ticks = 0;
        }

        self.motion_state = if moving {
            if cmd_retract && !cmd_extend {
                CylinderMotionState::MovingRetract
            } else {
                CylinderMotionState::MovingExtend
            }
        } else if self.position_ticks == 0 {
            CylinderMotionState::Retracted
        } else if self.position_ticks >= self.stroke_ticks {
            CylinderMotionState::Extended
        } else {
            CylinderMotionState::StoppedMid
        };
    }

    fn motion_state_label(&self) -> &'static str {
        match self.motion_state {
            CylinderMotionState::Retracted => "retracted",
            CylinderMotionState::Extended => "extended",
            CylinderMotionState::MovingExtend => "moving_extend",
            CylinderMotionState::MovingRetract => "moving_retract",
            CylinderMotionState::StoppedMid => "stopped_mid",
        }
    }
}

#[derive(Debug, Clone)]
struct StepperState {
    position_steps: i64,
    last_pulse: bool,
    enable: bool,
    direction: bool,
    alarm: bool,
    lost_step_remainder: f64,
}

impl StepperState {
    fn new() -> Self {
        Self {
            position_steps: 0,
            last_pulse: false,
            enable: false,
            direction: true,
            alarm: false,
            lost_step_remainder: 0.0,
        }
    }

    fn apply(&mut self, inputs: BTreeMap<String, bool>, fault: FaultProfile) {
        let pulse = *inputs.get("pulse").unwrap_or(&false);
        let dir = *inputs.get("direction").unwrap_or(&true);
        let enable = *inputs.get("enable").unwrap_or(&true);
        self.enable = enable;

        let mut effective_direction = dir;
        if fault.direction_reversed {
            effective_direction = !effective_direction;
        }
        self.direction = effective_direction;

        let rising = pulse && !self.last_pulse;
        self.alarm = fault.stall;
        if rising && enable && !fault.stall {
            let ratio = fault.lost_step_ratio.unwrap_or(1.0).clamp(0.0, 1.0);
            let planned = ratio + self.lost_step_remainder;
            let applied = planned.floor() as i64;
            self.lost_step_remainder = planned - (applied as f64);
            let sign: i64 = if effective_direction { 1 } else { -1 };
            self.position_steps = self
                .position_steps
                .saturating_add(sign.saturating_mul(applied.max(0)));
        } else if !enable || fault.stall {
            self.lost_step_remainder = 0.0;
        }

        self.last_pulse = pulse;
    }
}

fn apply_bool_fault(base: bool, fault: &FaultProfile, tick: u64) -> bool {
    if let Some(stuck) = fault.stuck_value {
        return stuck;
    }
    if let Some(chatter) = &fault.chatter {
        let phase = tick.saturating_sub(chatter.start_tick) % chatter.period_ticks.max(1);
        return phase < chatter.active_ticks.max(1);
    }
    base
}

fn component_type_label(component_type: ComponentType) -> &'static str {
    match component_type {
        ComponentType::Cylinder => "cylinder",
        ComponentType::Sensor => "sensor",
        ComponentType::Switch => "switch",
        ComponentType::StepperPd => "stepper_pd",
    }
}

fn fault_kind_label(fault_kind: ComponentFaultKind) -> &'static str {
    match fault_kind {
        ComponentFaultKind::Jammed => "jammed",
        ComponentFaultKind::MotionTimeout => "motion_timeout",
        ComponentFaultKind::StuckOn => "stuck_on",
        ComponentFaultKind::StuckOff => "stuck_off",
        ComponentFaultKind::Chatter => "chatter",
        ComponentFaultKind::LostStep => "lost_step",
        ComponentFaultKind::Stall => "stall",
        ComponentFaultKind::DirectionReversed => "direction_reversed",
    }
}

fn issue(
    code: impl Into<String>,
    path: impl Into<String>,
    message: impl Into<String>,
) -> ComponentSimIssue {
    ComponentSimIssue {
        code: code.into(),
        path: path.into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_scenario::parse_component_scenario_json;
    use crate::component_topology::parse_component_topology_json;

    fn sample_topology() -> ComponentTopology {
        parse_component_topology_json(
            r#"{
  "schema_version": 1,
  "component_library": {
    "schema_version": 1,
    "components": [
      { "id": "sw", "name": "Start", "type": "switch", "params": {} },
      { "id": "sn", "name": "Sensor", "type": "sensor", "params": {} },
      { "id": "cyl", "name": "Cylinder", "type": "cylinder", "params": { "stroke_ticks": 2 } },
      { "id": "stp", "name": "Stepper", "type": "stepper_pd", "params": {} }
    ]
  },
  "components": [
    { "id": "s0", "component_id": "sw", "params": {} },
    { "id": "x0", "component_id": "sn", "params": {} },
    { "id": "c0", "component_id": "cyl", "params": {} },
    { "id": "m0", "component_id": "stp", "params": {} }
  ],
  "connections": [
    { "from": "s0.state", "to": "c0.cmd_extend" },
    { "from": "x0.state", "to": "c0.cmd_retract" },
    { "from": "s0.state", "to": "m0.pulse" },
    { "from": "x0.state", "to": "m0.direction" },
    { "from": "s0.state", "to": "m0.enable" }
  ]
}"#,
        )
        .expect("valid topology")
    }

    #[test]
    fn cylinder_and_stepper_state_machine_is_deterministic() {
        let topology = sample_topology();
        let scenario = parse_component_scenario_json(
            r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 60,
  "switch_events": [
    { "at_ms": 0, "target": "s0", "value": true }
  ],
  "sensor_events": [
    { "at_ms": 30, "target": "x0", "value": false }
  ],
  "component_faults": []
}"#,
        )
        .expect("valid scenario");

        let first = run_component_simulation(&topology, &scenario).expect("sim should pass");
        let second = run_component_simulation(&topology, &scenario).expect("sim should pass");
        assert_eq!(first, second, "simulation should be deterministic");
        assert!(first.ticks.iter().any(|row| {
            row.components
                .get("c0")
                .is_some_and(|comp| comp.state.starts_with("moving"))
        }));
    }

    #[test]
    fn stepper_faults_change_position_evolution() {
        let topology = sample_topology();
        let scenario = parse_component_scenario_json(
            r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 80,
  "switch_events": [
    { "at_ms": 0, "target": "s0", "value": true },
    { "at_ms": 20, "target": "s0", "value": false },
    { "at_ms": 40, "target": "s0", "value": true },
    { "at_ms": 60, "target": "s0", "value": false }
  ],
  "sensor_events": [
    { "at_ms": 0, "target": "x0", "value": true }
  ],
  "component_faults": [
    { "at_ms": 20, "duration_ms": 20, "target_component_id": "m0", "fault_kind": "stall" },
    { "at_ms": 40, "duration_ms": 20, "target_component_id": "m0", "fault_kind": "direction_reversed" }
  ]
}"#,
        )
        .expect("valid scenario");

        let report = run_component_simulation(&topology, &scenario).expect("sim should pass");
        let final_stepper = report
            .ticks
            .last()
            .and_then(|row| row.components.get("m0"))
            .expect("stepper snapshot");
        let final_position = final_stepper
            .outputs
            .get("position_steps")
            .and_then(Value::as_i64)
            .expect("position output");
        assert!(
            final_position <= 0,
            "direction_reversed should affect position direction"
        );
        assert!(
            report
                .fault_audit
                .iter()
                .any(|entry| entry.action == "activated"
                    && entry.fault_kind == ComponentFaultKind::Stall)
        );
        assert!(report.fault_audit.iter().any(
            |entry| entry.action == "expired" && entry.fault_kind == ComponentFaultKind::Stall
        ));
    }

    #[test]
    fn rejects_fault_kind_not_supported_for_target_component() {
        let topology = sample_topology();
        let scenario = parse_component_scenario_json(
            r#"{
  "schema_version": 1,
  "tick_ms": 10,
  "duration_ms": 50,
  "component_faults": [
    { "at_ms": 10, "target_component_id": "s0", "fault_kind": "jammed" }
  ]
}"#,
        )
        .expect("scenario parse");

        let err = run_component_simulation(&topology, &scenario).expect_err("should fail");
        assert!(err.issues.iter().any(|issue| issue.code == "CSIM-TGT-006"));
    }
}
