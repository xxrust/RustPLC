pub const FAMILY: &str = "cylinder";
pub const COMMAND_PORT: &str = "cmd";
pub const EXTENDED_STATE_PORT: &str = "extended";
pub const RETRACTED_STATE_PORT: &str = "retracted";
pub const STROKE_ACTION: &str = "stroke";

pub const DEFAULT_FEEDBACK_DEBOUNCE_MS: u64 = 20;
pub const DEFAULT_STROKE_TIMEOUT_MS: u64 = 3000;
pub const DEFAULT_ALLOW_EXTEND: bool = true;
pub const DEFAULT_ALLOW_RETRACT: bool = true;
pub const DEFAULT_SIMULATION_MODE: bool = false;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CylinderActionDefaults {
    pub feedback_debounce_ms: u64,
    pub stroke_timeout_ms: u64,
    pub allow_extend: bool,
    pub allow_retract: bool,
    pub simulation_mode: bool,
}

pub const DEFAULT_ACTION_DEFAULTS: CylinderActionDefaults = CylinderActionDefaults {
    feedback_debounce_ms: DEFAULT_FEEDBACK_DEBOUNCE_MS,
    stroke_timeout_ms: DEFAULT_STROKE_TIMEOUT_MS,
    allow_extend: DEFAULT_ALLOW_EXTEND,
    allow_retract: DEFAULT_ALLOW_RETRACT,
    simulation_mode: DEFAULT_SIMULATION_MODE,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderTopologyContractKind {
    OpenLoopCommand,
    ClosedLoopDualEndFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderContractError {
    MissingComplementaryEndState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderStrokeVerb {
    Extend,
    Retract,
}

impl CylinderStrokeVerb {
    pub const fn expected_state_port(self) -> &'static str {
        match self {
            Self::Extend => EXTENDED_STATE_PORT,
            Self::Retract => RETRACTED_STATE_PORT,
        }
    }

    pub const fn action_keyword(self) -> &'static str {
        match self {
            Self::Extend => "extend",
            Self::Retract => "retract",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderStrokeFault {
    OppositeFeedbackReasserted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderSafetyFault {
    ContradictoryFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderFeedbackFault {
    OppositeFeedback,
    ContradictoryFeedback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CylinderActionOutcome {
    Done,
    Timeout,
    StrokeFault(CylinderStrokeFault),
    SafetyFault(CylinderSafetyFault),
}

pub fn is_end_state_port(port: &str) -> bool {
    matches!(port, EXTENDED_STATE_PORT | RETRACTED_STATE_PORT)
        || port.ends_with(".extended")
        || port.ends_with(".retracted")
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_ACTION_DEFAULTS, EXTENDED_STATE_PORT, RETRACTED_STATE_PORT, is_end_state_port,
    };

    #[test]
    fn exposes_cylinder_action_defaults() {
        assert_eq!(DEFAULT_ACTION_DEFAULTS.feedback_debounce_ms, 20);
        assert_eq!(DEFAULT_ACTION_DEFAULTS.stroke_timeout_ms, 3000);
        assert!(DEFAULT_ACTION_DEFAULTS.allow_extend);
        assert!(DEFAULT_ACTION_DEFAULTS.allow_retract);
        assert!(!DEFAULT_ACTION_DEFAULTS.simulation_mode);
    }

    #[test]
    fn detects_cylinder_terminal_feedback_ports() {
        assert!(is_end_state_port(EXTENDED_STATE_PORT));
        assert!(is_end_state_port(RETRACTED_STATE_PORT));
        assert!(is_end_state_port("rod_a.extended"));
        assert!(is_end_state_port("rod_a.retracted"));
        assert!(!is_end_state_port("rod_a.mid"));
    }
}
