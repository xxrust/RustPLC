    #[test]
    fn workpiece_token_store_creates_tokens_with_active_occupancy() {
        let mut store = WorkpieceTokenStore::new();

        let created = store
            .create_token(1, "part", "infeed")
            .expect("create token should succeed");

        assert_eq!(created.token_id, 1);
        assert_eq!(created.workpiece_type, "part");
        assert_eq!(created.current_location, "infeed");
        assert_eq!(created.mounted_slot, None);
        assert!(created.active);
        assert_eq!(created.terminal_status, None);
        assert_eq!(store.slots_used(), 1);
        assert_eq!(store.active_tokens(), 1);
        assert_eq!(store.active_tokens_at("infeed"), 1);
        assert_eq!(store.token(1), Some(created));
    }

    #[test]
    fn workpiece_token_store_moves_tokens_between_locations() {
        let mut store = WorkpieceTokenStore::new();
        store
            .create_token(7, "part", "infeed")
            .expect("create token should succeed");

        let moved = store
            .move_token(7, "arm")
            .expect("move token should succeed");

        assert_eq!(moved.current_location, "arm");
        assert_eq!(moved.mounted_slot, None);
        assert_eq!(store.active_tokens_at("infeed"), 0);
        assert_eq!(store.active_tokens_at("arm"), 1);
    }

    #[test]
    fn workpiece_token_store_finishes_tokens_and_retains_terminal_status() {
        let mut store = WorkpieceTokenStore::new();
        store
            .create_token(9, "part", "outfeed")
            .expect("create token should succeed");

        let finished = store
            .finish_token(
                9,
                WorkpieceTerminalStatus::TerminalState { state: "finished" },
            )
            .expect("finish token should succeed");

        assert!(!finished.active);
        assert_eq!(finished.mounted_slot, None);
        assert_eq!(
            finished.terminal_status,
            Some(WorkpieceTerminalStatus::TerminalState { state: "finished" })
        );
        assert_eq!(store.active_tokens(), 0);
        assert_eq!(store.active_tokens_at("outfeed"), 0);
        assert_eq!(store.token(9), Some(finished));
        assert_eq!(
            store.move_token(9, "reject_bin"),
            Err(WorkpieceTokenStoreError::TokenInactive { token_id: 9 })
        );
    }

    #[test]
    fn workpiece_token_store_rejects_capacity_overflow() {
        let mut store = WorkpieceTokenStore::new();
        for token_id in 0..MAX_WORKPIECE_TOKENS as WorkpieceTokenId {
            store
                .create_token(token_id, "part", "buffer")
                .expect("capacity fill should succeed");
        }

        assert_eq!(store.slots_used(), MAX_WORKPIECE_TOKENS);
        assert_eq!(store.active_tokens(), MAX_WORKPIECE_TOKENS);
        assert_eq!(
            store.create_token(MAX_WORKPIECE_TOKENS as WorkpieceTokenId, "part", "buffer"),
            Err(WorkpieceTokenStoreError::CapacityExceeded {
                max: MAX_WORKPIECE_TOKENS,
            })
        );
    }

    #[test]
    fn workpiece_lineage_store_tracks_split_children_after_source_is_consumed() {
        let mut tokens = WorkpieceTokenStore::new();
        tokens
            .create_token(1, "rod", "cut_station")
            .expect("source token should exist");
        tokens
            .create_token(2, "slice", "tray_a")
            .expect("child token a should exist");
        tokens
            .create_token(3, "slice", "tray_b")
            .expect("child token b should exist");

        let mut lineage = WorkpieceLineageStore::new();
        let child_a = lineage
            .record_split_child(1, 2)
            .expect("first split child should record");
        let child_b = lineage
            .record_split_child(1, 3)
            .expect("second split child should record");

        tokens
            .finish_token(1, WorkpieceTerminalStatus::Consumed)
            .expect("split source should be consumable");

        assert_eq!(lineage.len(), 2);
        assert_eq!(
            lineage.split_children_of(1).collect::<Vec<_>>(),
            vec![child_a, child_b]
        );
        assert_eq!(
            tokens
                .token(1)
                .expect("source token should remain traceable"),
            WorkpieceToken {
                token_id: 1,
                workpiece_type: "rod",
                current_location: "cut_station",
                mounted_slot: None,
                active: false,
                terminal_status: Some(WorkpieceTerminalStatus::Consumed),
            }
        );
    }

    #[test]
    fn workpiece_lineage_store_tracks_merge_inputs_after_inputs_are_consumed() {
        let mut tokens = WorkpieceTokenStore::new();
        tokens
            .create_token(10, "cell", "buffer_a")
            .expect("merge input a should exist");
        tokens
            .create_token(11, "cell", "buffer_b")
            .expect("merge input b should exist");
        tokens
            .create_token(12, "module", "assembly")
            .expect("merge output should exist");

        let mut lineage = WorkpieceLineageStore::new();
        let input_a = lineage
            .record_merge_input(10, 12)
            .expect("first merge input should record");
        let input_b = lineage
            .record_merge_input(11, 12)
            .expect("second merge input should record");

        tokens
            .finish_token(10, WorkpieceTerminalStatus::Consumed)
            .expect("merge input a should be consumable");
        tokens
            .finish_token(11, WorkpieceTerminalStatus::Consumed)
            .expect("merge input b should be consumable");

        assert_eq!(lineage.len(), 2);
        assert_eq!(
            lineage.merge_inputs_of(12).collect::<Vec<_>>(),
            vec![input_a, input_b]
        );
        assert_eq!(
            tokens
                .token(10)
                .expect("consumed merge input should stay inspectable")
                .terminal_status,
            Some(WorkpieceTerminalStatus::Consumed)
        );
        assert_eq!(
            tokens
                .token(11)
                .expect("consumed merge input should stay inspectable")
                .terminal_status,
            Some(WorkpieceTerminalStatus::Consumed)
        );
    }

    #[test]
    fn workpiece_lineage_store_rejects_capacity_overflow() {
        let mut lineage = WorkpieceLineageStore::new();
        for relation_id in 0..MAX_WORKPIECE_LINEAGE_RECORDS as WorkpieceTokenId {
            lineage
                .record_split_child(relation_id, relation_id.saturating_add(10_000))
                .expect("capacity fill should succeed");
        }

        assert_eq!(lineage.len(), MAX_WORKPIECE_LINEAGE_RECORDS);
        assert_eq!(
            lineage.record_merge_input(99_001, 99_002),
            Err(WorkpieceLineageStoreError::CapacityExceeded {
                max: MAX_WORKPIECE_LINEAGE_RECORDS,
            })
        );
    }

    #[test]
    fn runtime_executes_phase1_workpiece_flow_across_site_holder_site_and_finish() {
        static PICK: [Action; 1] = [Action::WorkpieceAcquire {
            workpiece_type: "part",
            holder: "arm",
            from: "infeed",
        }];
        static PLACE: [Action; 1] = [Action::WorkpieceTransfer {
            from: "arm",
            to: "outfeed",
        }];
        static FINISH: [Action; 1] = [Action::WorkpieceFinish {
            at: "outfeed",
            terminal_state: "finished",
        }];
        static STEPS: [Step<'static>; 6] = [
            Step {
                name: "pick",
                instr: Instr::Action {
                    actions: &PICK,
                    next: StepId(1),
                },
            },
            Step {
                name: "after_pick",
                instr: Instr::Delay {
                    ticks: 1,
                    next: StepId(2),
                },
            },
            Step {
                name: "place",
                instr: Instr::Action {
                    actions: &PLACE,
                    next: StepId(3),
                },
            },
            Step {
                name: "after_place",
                instr: Instr::Delay {
                    ticks: 1,
                    next: StepId(4),
                },
            },
            Step {
                name: "finish",
                instr: Instr::Action {
                    actions: &FINISH,
                    next: StepId(5),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 1] = [WorkpieceTypeDef {
            name: "part",
            normal_terminal_states: &["finished"],
            abnormal_terminal_states: &["rejected"],
            ingress_sites: &["infeed"],
            normal_egress_sites: &["outfeed"],
            abnormal_egress_sites: &["reject_bin"],
        }];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 3] = [
            WorkpieceSiteDef {
                name: "infeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "reject_bin",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static WORKPIECE_HOLDERS: [WorkpieceHolderDef<'static>; 1] = [WorkpieceHolderDef {
            name: "arm",
            capacity: 1,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &WORKPIECE_HOLDERS,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should seed ingress token");

        assert_eq!(rt.workpiece_tokens().active_tokens_at("infeed"), 1);
        assert_eq!(rt.workpiece_tokens().active_tokens_at("arm"), 0);
        assert_eq!(rt.workpiece_tokens().active_tokens_at("outfeed"), 0);

        rt.tick(&mut io).expect("pick tick should succeed");
        assert_eq!(
            rt.task_context(0).expect("task context").current_step,
            StepId(1)
        );
        assert_eq!(rt.workpiece_tokens().active_tokens_at("infeed"), 0);
        assert_eq!(rt.workpiece_tokens().active_tokens_at("arm"), 1);
        assert_eq!(rt.workpiece_tokens().active_tokens_at("outfeed"), 0);

        rt.tick(&mut io).expect("place tick should succeed");
        assert_eq!(
            rt.task_context(0).expect("task context").current_step,
            StepId(3)
        );
        assert_eq!(rt.workpiece_tokens().active_tokens_at("arm"), 0);
        assert_eq!(rt.workpiece_tokens().active_tokens_at("outfeed"), 1);

        rt.tick(&mut io).expect("finish tick should succeed");
        assert_eq!(
            rt.task_context(0).expect("task context").current_step,
            StepId(5)
        );
        assert_eq!(rt.workpiece_tokens().active_tokens(), 0);
        let finished = rt
            .workpiece_tokens()
            .token(0)
            .expect("seeded token should remain traceable");
        assert_eq!(finished.current_location, "outfeed");
        assert!(!finished.active);
        assert_eq!(
            finished.terminal_status,
            Some(WorkpieceTerminalStatus::TerminalState { state: "finished" })
        );
    }

    #[test]
    fn runtime_rejects_workpiece_source_underflow() {
        static ACTIONS: [Action; 1] = [Action::WorkpieceTransfer {
            from: "arm",
            to: "outfeed",
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "transfer_without_part",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 1] = [WorkpieceTypeDef {
            name: "part",
            normal_terminal_states: &["finished"],
            abnormal_terminal_states: &[],
            ingress_sites: &["infeed"],
            normal_egress_sites: &["outfeed"],
            abnormal_egress_sites: &[],
        }];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "infeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static WORKPIECE_HOLDERS: [WorkpieceHolderDef<'static>; 1] = [WorkpieceHolderDef {
            name: "arm",
            capacity: 1,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &WORKPIECE_HOLDERS,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt.tick(&mut io).expect_err("empty source should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceSourceUnderflow { endpoint: "arm" }
        );
    }

    #[test]
    fn runtime_rejects_workpiece_duplicate_source_occupancy() {
        static ACTIONS: [Action; 1] = [Action::WorkpieceAcquire {
            workpiece_type: "part_a",
            holder: "arm",
            from: "infeed",
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "pick_ambiguous_source",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "part_a",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &["infeed"],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "part_b",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &["infeed"],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "infeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 2,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static WORKPIECE_HOLDERS: [WorkpieceHolderDef<'static>; 1] = [WorkpieceHolderDef {
            name: "arm",
            capacity: 1,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &WORKPIECE_HOLDERS,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should seed both ingress parts");
        let err = rt.tick(&mut io).expect_err("ambiguous source should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceDuplicateOccupancy {
                endpoint: "infeed",
                count: 2,
            }
        );
    }

    #[test]
    fn runtime_rejects_workpiece_destination_overflow() {
        static ACTIONS_PICK: [Action; 1] = [Action::WorkpieceAcquire {
            workpiece_type: "incoming_part",
            holder: "arm",
            from: "infeed",
        }];
        static ACTIONS_SEED_HOLDER: [Action; 1] = [Action::WorkpieceTransfer {
            from: "arm",
            to: "arm",
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "pick_into_full_holder",
                instr: Instr::Action {
                    actions: &ACTIONS_PICK,
                    next: StepId(0),
                },
            },
            Step {
                name: "seed_holder_source",
                instr: Instr::Action {
                    actions: &ACTIONS_SEED_HOLDER,
                    next: StepId(1),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "incoming_part",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &["infeed"],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "holder_part",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &["arm"],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "infeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static WORKPIECE_HOLDERS: [WorkpieceHolderDef<'static>; 1] = [WorkpieceHolderDef {
            name: "arm",
            capacity: 1,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &WORKPIECE_HOLDERS,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should seed source and holder");
        let err = rt.tick(&mut io).expect_err("full destination should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceOverflow {
                endpoint: "arm",
                capacity: 1,
                occupancy: 1,
            }
        );
    }

    #[test]
    fn runtime_executes_phase2_mount_transform_unmount_flow() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "steel_plate.slot[0]",
        }];
        static ACTIONS_TRANSFORM: [Action; 1] = [Action::WorkpieceTransformCarrier {
            carrier: "steel_plate",
            frame: "cut_height",
        }];
        static ACTIONS_UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "steel_plate.slot[0]",
            to: "outfeed",
        }];
        static ACTIONS_FINISH: [Action; 1] = [Action::WorkpieceFinish {
            at: "outfeed",
            terminal_state: "finished",
        }];
        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "mount",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "raise",
                instr: Instr::Action {
                    actions: &ACTIONS_TRANSFORM,
                    next: StepId(2),
                },
            },
            Step {
                name: "unmount",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT,
                    next: StepId(3),
                },
            },
            Step {
                name: "finish",
                instr: Instr::Action {
                    actions: &ACTIONS_FINISH,
                    next: StepId(4),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 1] = [WorkpieceTypeDef {
            name: "rod",
            normal_terminal_states: &["finished"],
            abnormal_terminal_states: &[],
            ingress_sites: &[],
            normal_egress_sites: &["outfeed"],
            abnormal_egress_sites: &[],
        }];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "steel_plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");

        rt.tick(&mut io).expect("phase2 flow should succeed");
        let finished = rt
            .workpiece_tokens()
            .token(0)
            .expect("finished token should remain traceable");
        assert_eq!(finished.current_location, "outfeed");
        assert_eq!(finished.mounted_slot, None);
        assert!(!finished.active);
    }

    #[test]
    fn runtime_rejects_unmount_from_empty_phase2_slot() {
        static ACTIONS: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "steel_plate.slot[0]",
            to: "outfeed",
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "unmount",
            instr: Instr::Action {
                actions: &ACTIONS,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 1] = [WorkpieceTypeDef {
            name: "rod",
            normal_terminal_states: &["finished"],
            abnormal_terminal_states: &[],
            ingress_sites: &[],
            normal_egress_sites: &["outfeed"],
            abnormal_egress_sites: &[],
        }];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "steel_plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt.tick(&mut io).expect_err("empty slot should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceSourceUnderflow {
                endpoint: "steel_plate.slot[0]",
            }
        );
    }

    #[test]
    fn runtime_rejects_duplicate_phase2_mount() {
        static ACTIONS: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "steel_plate.slot[0]",
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "mount_a",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
            },
            Step {
                name: "mount_b",
                instr: Instr::Action {
                    actions: &ACTIONS,
                    next: StepId(1),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 1] = [WorkpieceTypeDef {
            name: "rod",
            normal_terminal_states: &["finished"],
            abnormal_terminal_states: &[],
            ingress_sites: &[],
            normal_egress_sites: &["outfeed"],
            abnormal_egress_sites: &[],
        }];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "steel_plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt.tick(&mut io).expect_err("duplicate mount should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceDuplicateMount {
                slot: "steel_plate.slot[0]",
                token_id: 0,
            }
        );
    }

    #[test]
    fn runtime_rejects_phase2_slot_overflow() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "steel_plate.slot[0]",
        }];
        static ACTIONS_SEED_SLOT: [Action; 1] = [Action::WorkpieceAcquire {
            workpiece_type: "seeded",
            holder: "arm",
            from: "steel_plate.slot[0]",
        }];
        static STEPS: [Step<'static>; 2] = [
            Step {
                name: "mount_into_full_slot",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(0),
                },
            },
            Step {
                name: "seed_slot_source",
                instr: Instr::Action {
                    actions: &ACTIONS_SEED_SLOT,
                    next: StepId(1),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "rod",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "seeded",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &["steel_plate.slot[0]"],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "steel_plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static WORKPIECE_HOLDERS: [WorkpieceHolderDef<'static>; 1] = [WorkpieceHolderDef {
            name: "arm",
            capacity: 1,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &WORKPIECE_HOLDERS,
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt.tick(&mut io).expect_err("full slot should fail mount");
        assert_eq!(
            err,
            RuntimeError::WorkpieceOverflow {
                endpoint: "steel_plate.slot[0]",
                capacity: 1,
                occupancy: 1,
            }
        );
    }

    #[test]
    fn runtime_executes_split_and_records_lineage() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
        }];
        static ACTIONS_UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
            to: "cut_zone",
        }];
        static ACTIONS_SPLIT: [Action; 1] = [Action::WorkpieceSplit {
            source_type: "rod",
            target_type: "slice",
            count: 4,
            consumed: true,
        }];
        static STEPS: [Step<'static>; 4] = [
            Step {
                name: "load",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "unload",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT,
                    next: StepId(2),
                },
            },
            Step {
                name: "cut",
                instr: Instr::Action {
                    actions: &ACTIONS_SPLIT,
                    next: StepId(3),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "rod",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 3] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_zone",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 4,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 4,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        rt.tick(&mut io).expect("split flow should succeed");

        assert_eq!(rt.workpiece_tokens().active_tokens(), 4);
        let consumed = rt
            .workpiece_tokens()
            .token(0)
            .expect("source token should remain traceable");
        assert_eq!(consumed.workpiece_type, "rod");
        assert_eq!(consumed.current_location, "cut_zone");
        assert_eq!(
            consumed.terminal_status,
            Some(WorkpieceTerminalStatus::Consumed)
        );
        assert!(!consumed.active);

        for child_id in 1..=4 {
            let child = rt
                .workpiece_tokens()
                .token(child_id)
                .expect("split child should be stored");
            assert_eq!(child.workpiece_type, "slice");
            assert_eq!(child.current_location, "cut_zone");
            assert_eq!(child.mounted_slot, None);
            assert!(child.active);
            assert_eq!(child.terminal_status, None);
        }

        assert_eq!(rt.workpiece_lineage().len(), 4);
        assert_eq!(
            rt.workpiece_lineage()
                .split_children_of(0)
                .map(|record| record.target_token_id)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
    }

    #[test]
    fn runtime_rejects_split_without_available_source_type() {
        static ACTIONS_SPLIT: [Action; 1] = [Action::WorkpieceSplit {
            source_type: "rod",
            target_type: "slice",
            count: 2,
            consumed: true,
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "cut",
            instr: Instr::Action {
                actions: &ACTIONS_SPLIT,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "rod",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &[],
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt
            .tick(&mut io)
            .expect_err("missing split source should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceTypeSourceUnderflow {
                workpiece_type: "rod",
            }
        );
    }

    #[test]
    fn runtime_rejects_split_when_source_type_is_ambiguous() {
        static ACTIONS_MOUNT_A: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
        }];
        static ACTIONS_UNMOUNT_A: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
            to: "cut_a",
        }];
        static ACTIONS_MOUNT_B: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "plate.slot[1]",
        }];
        static ACTIONS_UNMOUNT_B: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "plate.slot[1]",
            to: "cut_b",
        }];
        static ACTIONS_SPLIT: [Action; 1] = [Action::WorkpieceSplit {
            source_type: "rod",
            target_type: "slice",
            count: 2,
            consumed: true,
        }];
        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "load_a",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT_A,
                    next: StepId(1),
                },
            },
            Step {
                name: "unload_a",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT_A,
                    next: StepId(2),
                },
            },
            Step {
                name: "load_b",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT_B,
                    next: StepId(3),
                },
            },
            Step {
                name: "unload_b",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT_B,
                    next: StepId(4),
                },
            },
            Step {
                name: "cut",
                instr: Instr::Action {
                    actions: &ACTIONS_SPLIT,
                    next: StepId(4),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "rod",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 4] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "plate.slot[1]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_a",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_b",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt
            .tick(&mut io)
            .expect_err("ambiguous split source should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceTypeSourceAmbiguity {
                workpiece_type: "rod",
                count: 2,
            }
        );
    }

    #[test]
    fn runtime_rejects_split_when_outputs_overflow_destination() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
        }];
        static ACTIONS_UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
            to: "cut_zone",
        }];
        static ACTIONS_SPLIT: [Action; 1] = [Action::WorkpieceSplit {
            source_type: "rod",
            target_type: "slice",
            count: 4,
            consumed: true,
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "load",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "unload",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT,
                    next: StepId(2),
                },
            },
            Step {
                name: "cut",
                instr: Instr::Action {
                    actions: &ACTIONS_SPLIT,
                    next: StepId(2),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "rod",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_zone",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 3,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt.tick(&mut io).expect_err("split overflow should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceSplitOverflow {
                workpiece_type: "rod",
                capacity: 3,
                occupancy: 1,
            }
        );
    }

    #[test]
    fn runtime_executes_merge_and_records_lineage() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
        }];
        static ACTIONS_UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
            to: "cut_zone",
        }];
        static ACTIONS_SPLIT: [Action; 1] = [Action::WorkpieceSplit {
            source_type: "rod",
            target_type: "slice",
            count: 4,
            consumed: true,
        }];
        static MERGE_INPUT_REFS: [&str; 2] = ["slice_a", "slice_b"];
        static MERGE_INPUT_TYPES: [&str; 2] = ["slice", "slice"];
        static ACTIONS_MERGE: [Action; 1] = [Action::WorkpieceMerge {
            input_refs: &MERGE_INPUT_REFS,
            input_types: &MERGE_INPUT_TYPES,
            target_type: "module",
            consumed_inputs: true,
        }];
        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "load",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "unload",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT,
                    next: StepId(2),
                },
            },
            Step {
                name: "cut",
                instr: Instr::Action {
                    actions: &ACTIONS_SPLIT,
                    next: StepId(3),
                },
            },
            Step {
                name: "assemble",
                instr: Instr::Action {
                    actions: &ACTIONS_MERGE,
                    next: StepId(4),
                },
            },
            Step {
                name: "halt",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 3] = [
            WorkpieceTypeDef {
                name: "rod",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "module",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_zone",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 4,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        rt.tick(&mut io).expect("merge flow should succeed");

        assert_eq!(rt.workpiece_tokens().active_tokens(), 3);
        assert_eq!(rt.workpiece_lineage().len(), 6);

        let module = rt
            .workpiece_tokens()
            .token(5)
            .expect("merge output should be stored");
        assert_eq!(module.workpiece_type, "module");
        assert_eq!(module.current_location, "cut_zone");
        assert!(module.active);

        assert_eq!(
            rt.workpiece_lineage()
                .merge_inputs_of(5)
                .map(|record| record.source_token_id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        for consumed_id in [1, 2] {
            let consumed = rt
                .workpiece_tokens()
                .token(consumed_id)
                .expect("consumed merge input should stay traceable");
            assert_eq!(
                consumed.terminal_status,
                Some(WorkpieceTerminalStatus::Consumed)
            );
            assert!(!consumed.active);
        }
    }

    #[test]
    fn runtime_rejects_merge_without_required_inputs() {
        static MERGE_INPUT_REFS: [&str; 2] = ["slice_a", "slice_b"];
        static MERGE_INPUT_TYPES: [&str; 2] = ["slice", "slice"];
        static ACTIONS_MERGE: [Action; 1] = [Action::WorkpieceMerge {
            input_refs: &MERGE_INPUT_REFS,
            input_types: &MERGE_INPUT_TYPES,
            target_type: "module",
            consumed_inputs: true,
        }];
        static STEPS: [Step<'static>; 1] = [Step {
            name: "assemble",
            instr: Instr::Action {
                actions: &ACTIONS_MERGE,
                next: StepId(0),
            },
        }];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "module",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 1] = [WorkpieceSiteDef {
            name: "cut_zone",
            kind: WorkpieceSiteKind::WorkpieceLocation,
            capacity: 4,
        }];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt
            .tick(&mut io)
            .expect_err("missing merge input should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceMergeInputUnderflow {
                target_type: "module",
                input_ref: "slice_a",
                required_type: "slice",
            }
        );
    }

    #[test]
    fn runtime_rejects_merge_with_duplicate_consumed_input_refs() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "slice",
            slot: "plate.slot[0]",
        }];
        static ACTIONS_UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "slice",
            slot: "plate.slot[0]",
            to: "cut_zone",
        }];
        static ACTIONS_MOUNT_B: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "slice",
            slot: "plate.slot[1]",
        }];
        static ACTIONS_UNMOUNT_B: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "slice",
            slot: "plate.slot[1]",
            to: "cut_zone_b",
        }];
        static MERGE_INPUT_REFS: [&str; 2] = ["slice_a", "slice_a"];
        static MERGE_INPUT_TYPES: [&str; 2] = ["slice", "slice"];
        static ACTIONS_MERGE: [Action; 1] = [Action::WorkpieceMerge {
            input_refs: &MERGE_INPUT_REFS,
            input_types: &MERGE_INPUT_TYPES,
            target_type: "module",
            consumed_inputs: true,
        }];
        static STEPS: [Step<'static>; 5] = [
            Step {
                name: "load_a",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "unload_a",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT,
                    next: StepId(2),
                },
            },
            Step {
                name: "load_b",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT_B,
                    next: StepId(3),
                },
            },
            Step {
                name: "unload_b",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT_B,
                    next: StepId(4),
                },
            },
            Step {
                name: "assemble",
                instr: Instr::Action {
                    actions: &ACTIONS_MERGE,
                    next: StepId(4),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "module",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 4] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "plate.slot[1]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_zone",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 2,
            },
            WorkpieceSiteDef {
                name: "cut_zone_b",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 2,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt
            .tick(&mut io)
            .expect_err("duplicate merge input ref should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceDuplicateConsumedMergeInput {
                input_ref: "slice_a",
            }
        );
    }

    #[test]
    fn runtime_rejects_merge_with_input_arity_mismatch() {
        static ACTIONS_MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "slice",
            slot: "plate.slot[0]",
        }];
        static ACTIONS_UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "slice",
            slot: "plate.slot[0]",
            to: "cut_zone",
        }];
        static MERGE_INPUT_REFS: [&str; 2] = ["slice_a", "slice_b"];
        static MERGE_INPUT_TYPES: [&str; 1] = ["slice"];
        static ACTIONS_MERGE: [Action; 1] = [Action::WorkpieceMerge {
            input_refs: &MERGE_INPUT_REFS,
            input_types: &MERGE_INPUT_TYPES,
            target_type: "module",
            consumed_inputs: true,
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "load",
                instr: Instr::Action {
                    actions: &ACTIONS_MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "unload",
                instr: Instr::Action {
                    actions: &ACTIONS_UNMOUNT,
                    next: StepId(2),
                },
            },
            Step {
                name: "assemble",
                instr: Instr::Action {
                    actions: &ACTIONS_MERGE,
                    next: StepId(2),
                },
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static WORKPIECE_TYPES: [WorkpieceTypeDef<'static>; 2] = [
            WorkpieceTypeDef {
                name: "slice",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
            WorkpieceTypeDef {
                name: "module",
                normal_terminal_states: &["finished"],
                abnormal_terminal_states: &[],
                ingress_sites: &[],
                normal_egress_sites: &["outfeed"],
                abnormal_egress_sites: &[],
            },
        ];
        static WORKPIECE_SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "cut_zone",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 2,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &WORKPIECE_TYPES,
            workpiece_sites: &WORKPIECE_SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init should succeed");
        let err = rt
            .tick(&mut io)
            .expect_err("merge arity mismatch should fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceMergeArityMismatch {
                target_type: "module",
                input_refs: 2,
                input_types: 1,
            }
        );
    }

    #[test]
    fn runtime_rejects_unmount_when_declared_type_differs_from_mounted_token() {
        static MOUNT: [Action; 1] = [Action::WorkpieceMount {
            workpiece_type: "rod",
            slot: "plate.slot[0]",
        }];
        static UNMOUNT: [Action; 1] = [Action::WorkpieceUnmount {
            workpiece_type: "cell",
            slot: "plate.slot[0]",
            to: "outfeed",
        }];
        static STEPS: [Step<'static>; 3] = [
            Step {
                name: "mount",
                instr: Instr::Action {
                    actions: &MOUNT,
                    next: StepId(1),
                },
            },
            Step {
                name: "unmount",
                instr: Instr::Action {
                    actions: &UNMOUNT,
                    next: StepId(2),
                },
            },
            Step {
                name: "done",
                instr: Instr::Halt,
            },
        ];
        static TASKS: [Task<'static>; 1] = [Task {
            name: "main",
            steps: &STEPS,
            entry: StepId(0),
        }];
        static SITES: [WorkpieceSiteDef<'static>; 2] = [
            WorkpieceSiteDef {
                name: "plate.slot[0]",
                kind: WorkpieceSiteKind::CarrierLocation,
                capacity: 1,
            },
            WorkpieceSiteDef {
                name: "outfeed",
                kind: WorkpieceSiteKind::WorkpieceLocation,
                capacity: 1,
            },
        ];
        static PROGRAM: Program<'static> = Program {
            tasks: &TASKS,
            pid_loops: &[],
            var_init: &[],
            cam_configs: &[],
            cam_tables: &[],
            axis_fault_policies: &[],
            semantic_resources: &[],
            resource_claims: &[],
            workpiece_types: &[],
            workpiece_sites: &SITES,
            workpiece_holders: &[],
        };

        let mut io = MemIo::new();
        let mut rt = Runtime::new(&PROGRAM).expect("runtime init");
        let err = rt
            .tick(&mut io)
            .expect_err("unmount type mismatch must fail");
        assert_eq!(
            err,
            RuntimeError::WorkpieceTypeMismatch {
                endpoint: "plate.slot[0]",
                expected: "cell",
                token_id: 0,
            }
        );
    }
