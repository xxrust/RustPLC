#[cfg(test)]
mod tests {
    use crate::device_semantics::cylinder::{
        complementary_end_state_port as cylinder_complementary_state_port,
        is_end_state_port as is_cylinder_end_state_port,
    };

    #[test]
    fn cylinder_complementary_state_port_maps_default_end_states() {
        assert_eq!(
            cylinder_complementary_state_port("extended").as_deref(),
            Some("retracted")
        );
        assert_eq!(
            cylinder_complementary_state_port("retracted").as_deref(),
            Some("extended")
        );
    }

    #[test]
    fn cylinder_complementary_state_port_preserves_port_scope() {
        assert_eq!(
            cylinder_complementary_state_port("rod_a.extended").as_deref(),
            Some("rod_a.retracted")
        );
        assert_eq!(
            cylinder_complementary_state_port("rod_a.retracted").as_deref(),
            Some("rod_a.extended")
        );
        assert_eq!(cylinder_complementary_state_port("mid"), None);
    }

    #[test]
    fn cylinder_end_state_port_detection_matches_only_terminal_feedback() {
        assert!(is_cylinder_end_state_port("extended"));
        assert!(is_cylinder_end_state_port("retracted"));
        assert!(is_cylinder_end_state_port("rod_a.extended"));
        assert!(is_cylinder_end_state_port("rod_a.retracted"));
        assert!(!is_cylinder_end_state_port("sense"));
        assert!(!is_cylinder_end_state_port("mid"));
    }

    #[test]
    fn state_port_match_requires_exact_port_scope() {
        assert!(super::state_port_matches(Some("extended"), "extended"));
        assert!(super::state_port_matches(
            Some("rod_a.extended"),
            "rod_a.extended"
        ));
        assert!(!super::state_port_matches(
            Some("extended"),
            "rod_a.extended"
        ));
        assert!(!super::state_port_matches(
            Some("rod_a.extended"),
            "extended"
        ));
    }

    #[test]
    fn runtime_branch_lowering_recognizes_if_else_complement_guards() {
        assert!(super::is_not_of("NOT(loaded_count >= 2)", "loaded_count >= 2"));
        assert!(super::is_not_of(
            "NOT( pickup_buffer_ready == true )",
            "pickup_buffer_ready == true"
        ));
        assert!(!super::is_not_of(
            "NOT(loaded_count < 1)",
            "loaded_count >= 1"
        ));
    }
}
