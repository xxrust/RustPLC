#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceToken<'a> {
    pub token_id: WorkpieceTokenId,
    pub workpiece_type: &'a str,
    pub current_location: &'a str,
    pub mounted_slot: Option<&'a str>,
    pub active: bool,
    pub terminal_status: Option<WorkpieceTerminalStatus<'a>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkpieceTokenStoreError {
    DuplicateTokenId { token_id: WorkpieceTokenId },
    TokenNotFound { token_id: WorkpieceTokenId },
    TokenInactive { token_id: WorkpieceTokenId },
    CapacityExceeded { max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceTokenStore<'a> {
    tokens: [Option<WorkpieceToken<'a>>; MAX_WORKPIECE_TOKENS],
    slots_used: usize,
    active_tokens: usize,
}

impl<'a> WorkpieceTokenStore<'a> {
    pub const fn new() -> Self {
        Self {
            tokens: [None; MAX_WORKPIECE_TOKENS],
            slots_used: 0,
            active_tokens: 0,
        }
    }

    pub fn slots_used(&self) -> usize {
        self.slots_used
    }

    pub fn active_tokens(&self) -> usize {
        self.active_tokens
    }

    pub fn token(&self, token_id: WorkpieceTokenId) -> Option<WorkpieceToken<'a>> {
        self.find_index(token_id)
            .and_then(|idx| self.tokens[idx].as_ref().copied())
    }

    pub fn active_tokens_at(&self, location: &str) -> usize {
        self.tokens
            .iter()
            .flatten()
            .filter(|token| token.active && token.current_location == location)
            .count()
    }

    pub fn create_token(
        &mut self,
        token_id: WorkpieceTokenId,
        workpiece_type: &'a str,
        current_location: &'a str,
    ) -> Result<WorkpieceToken<'a>, WorkpieceTokenStoreError> {
        if self.find_index(token_id).is_some() {
            return Err(WorkpieceTokenStoreError::DuplicateTokenId { token_id });
        }
        let Some(slot_idx) = self.tokens.iter().position(Option::is_none) else {
            return Err(WorkpieceTokenStoreError::CapacityExceeded {
                max: MAX_WORKPIECE_TOKENS,
            });
        };

        let token = WorkpieceToken {
            token_id,
            workpiece_type,
            current_location,
            mounted_slot: None,
            active: true,
            terminal_status: None,
        };
        self.tokens[slot_idx] = Some(token);
        self.slots_used += 1;
        self.active_tokens += 1;
        Ok(token)
    }

    pub fn move_token(
        &mut self,
        token_id: WorkpieceTokenId,
        new_location: &'a str,
    ) -> Result<WorkpieceToken<'a>, WorkpieceTokenStoreError> {
        let idx = self
            .find_index(token_id)
            .ok_or(WorkpieceTokenStoreError::TokenNotFound { token_id })?;
        let Some(token) = self.tokens[idx].as_mut() else {
            return Err(WorkpieceTokenStoreError::TokenNotFound { token_id });
        };
        if !token.active {
            return Err(WorkpieceTokenStoreError::TokenInactive { token_id });
        }
        token.current_location = new_location;
        Ok(*token)
    }

    pub fn finish_token(
        &mut self,
        token_id: WorkpieceTokenId,
        terminal_status: WorkpieceTerminalStatus<'a>,
    ) -> Result<WorkpieceToken<'a>, WorkpieceTokenStoreError> {
        let idx = self
            .find_index(token_id)
            .ok_or(WorkpieceTokenStoreError::TokenNotFound { token_id })?;
        let Some(token) = self.tokens[idx].as_mut() else {
            return Err(WorkpieceTokenStoreError::TokenNotFound { token_id });
        };
        if !token.active {
            return Err(WorkpieceTokenStoreError::TokenInactive { token_id });
        }
        token.active = false;
        token.mounted_slot = None;
        token.terminal_status = Some(terminal_status);
        self.active_tokens = self.active_tokens.saturating_sub(1);
        Ok(*token)
    }

    pub fn set_mounted_slot(
        &mut self,
        token_id: WorkpieceTokenId,
        mounted_slot: Option<&'a str>,
    ) -> Result<WorkpieceToken<'a>, WorkpieceTokenStoreError> {
        let idx = self
            .find_index(token_id)
            .ok_or(WorkpieceTokenStoreError::TokenNotFound { token_id })?;
        let Some(token) = self.tokens[idx].as_mut() else {
            return Err(WorkpieceTokenStoreError::TokenNotFound { token_id });
        };
        if !token.active {
            return Err(WorkpieceTokenStoreError::TokenInactive { token_id });
        }
        token.mounted_slot = mounted_slot;
        Ok(*token)
    }

    fn find_index(&self, token_id: WorkpieceTokenId) -> Option<usize> {
        self.tokens
            .iter()
            .position(|entry| entry.is_some_and(|token| token.token_id == token_id))
    }
}

impl<'a> Default for WorkpieceTokenStore<'a> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkpieceLineageKind {
    SplitParentToChild,
    MergeInputToOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceLineageRecord {
    pub record_id: WorkpieceLineageRecordId,
    pub relation: WorkpieceLineageKind,
    pub source_token_id: WorkpieceTokenId,
    pub target_token_id: WorkpieceTokenId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkpieceLineageStoreError {
    DuplicateRelation {
        relation: WorkpieceLineageKind,
        source_token_id: WorkpieceTokenId,
        target_token_id: WorkpieceTokenId,
    },
    CapacityExceeded {
        max: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkpieceLineageStore {
    records: [Option<WorkpieceLineageRecord>; MAX_WORKPIECE_LINEAGE_RECORDS],
    records_used: usize,
    next_record_id: WorkpieceLineageRecordId,
}

impl WorkpieceLineageStore {
    pub const fn new() -> Self {
        Self {
            records: [None; MAX_WORKPIECE_LINEAGE_RECORDS],
            records_used: 0,
            next_record_id: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.records_used
    }

    pub fn is_empty(&self) -> bool {
        self.records_used == 0
    }

    pub fn record_split_child(
        &mut self,
        parent_token_id: WorkpieceTokenId,
        child_token_id: WorkpieceTokenId,
    ) -> Result<WorkpieceLineageRecord, WorkpieceLineageStoreError> {
        self.record_relation(
            WorkpieceLineageKind::SplitParentToChild,
            parent_token_id,
            child_token_id,
        )
    }

    pub fn record_merge_input(
        &mut self,
        input_token_id: WorkpieceTokenId,
        output_token_id: WorkpieceTokenId,
    ) -> Result<WorkpieceLineageRecord, WorkpieceLineageStoreError> {
        self.record_relation(
            WorkpieceLineageKind::MergeInputToOutput,
            input_token_id,
            output_token_id,
        )
    }

    pub fn iter(&self) -> impl Iterator<Item = WorkpieceLineageRecord> + '_ {
        self.records.iter().flatten().copied()
    }

    pub fn split_children_of(
        &self,
        parent_token_id: WorkpieceTokenId,
    ) -> impl Iterator<Item = WorkpieceLineageRecord> + '_ {
        self.iter().filter(move |record| {
            record.relation == WorkpieceLineageKind::SplitParentToChild
                && record.source_token_id == parent_token_id
        })
    }

    pub fn merge_inputs_of(
        &self,
        output_token_id: WorkpieceTokenId,
    ) -> impl Iterator<Item = WorkpieceLineageRecord> + '_ {
        self.iter().filter(move |record| {
            record.relation == WorkpieceLineageKind::MergeInputToOutput
                && record.target_token_id == output_token_id
        })
    }

    fn record_relation(
        &mut self,
        relation: WorkpieceLineageKind,
        source_token_id: WorkpieceTokenId,
        target_token_id: WorkpieceTokenId,
    ) -> Result<WorkpieceLineageRecord, WorkpieceLineageStoreError> {
        if self.iter().any(|record| {
            record.relation == relation
                && record.source_token_id == source_token_id
                && record.target_token_id == target_token_id
        }) {
            return Err(WorkpieceLineageStoreError::DuplicateRelation {
                relation,
                source_token_id,
                target_token_id,
            });
        }

        let Some(slot_idx) = self.records.iter().position(Option::is_none) else {
            return Err(WorkpieceLineageStoreError::CapacityExceeded {
                max: MAX_WORKPIECE_LINEAGE_RECORDS,
            });
        };

        let record = WorkpieceLineageRecord {
            record_id: self.next_record_id,
            relation,
            source_token_id,
            target_token_id,
        };
        self.records[slot_idx] = Some(record);
        self.records_used += 1;
        self.next_record_id = self.next_record_id.saturating_add(1);
        Ok(record)
    }
}

impl Default for WorkpieceLineageStore {
    fn default() -> Self {
        Self::new()
    }
}
