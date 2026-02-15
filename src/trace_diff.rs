use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NormalizedTraceEvent {
    pub tick: u64,
    pub task: usize,
    pub from_step: u16,
    pub to_step: u16,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceDiffContextRow {
    pub index: usize,
    pub sil: Option<NormalizedTraceEvent>,
    pub board: Option<NormalizedTraceEvent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceMismatchType {
    Step,
    Reason,
    Edge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceDiffReport {
    pub is_match: bool,
    pub sil_events: usize,
    pub board_events: usize,
    pub first_mismatch_tick: Option<u64>,
    pub mismatch_type: Option<TraceMismatchType>,
    pub mismatch_index: Option<usize>,
    pub context_window: usize,
    pub context: Vec<TraceDiffContextRow>,
}

#[derive(Debug, thiserror::Error)]
pub enum TraceJsonlParseError {
    #[error("line {line}: invalid JSON: {message}")]
    InvalidJson { line: usize, message: String },

    #[error("line {line}: invalid trace row: {message}")]
    InvalidRow { line: usize, message: String },
}

#[derive(Debug, Deserialize)]
struct JsonlRowAny {
    tick: u64,
    task: usize,
    from_step: u16,
    to_step: u16,
    reason: String,

    // Present in board traces; absent in SIL traces.
    #[allow(dead_code)]
    #[serde(default)]
    timestamp_ms: Option<u64>,
}

pub fn parse_trace_jsonl(input: &str) -> Result<Vec<NormalizedTraceEvent>, TraceJsonlParseError> {
    let mut out = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let row: JsonlRowAny = serde_json::from_str(line).map_err(|err| TraceJsonlParseError::InvalidJson {
            line: line_no,
            message: err.to_string(),
        })?;
        out.push(NormalizedTraceEvent {
            tick: row.tick,
            task: row.task,
            from_step: row.from_step,
            to_step: row.to_step,
            reason: row.reason,
        });
    }
    Ok(out)
}

pub fn diff_traces(
    sil: &[NormalizedTraceEvent],
    board: &[NormalizedTraceEvent],
    context_window: usize,
) -> TraceDiffReport {
    let mut mismatch_index: Option<usize> = None;
    let mut mismatch_type: Option<TraceMismatchType> = None;
    let mut first_mismatch_tick: Option<u64> = None;

    let min_len = sil.len().min(board.len());
    for i in 0..min_len {
        if sil[i] == board[i] {
            continue;
        }

        mismatch_index = Some(i);
        mismatch_type = Some(classify_mismatch(&sil[i], &board[i]));
        first_mismatch_tick = Some(sil[i].tick.min(board[i].tick));
        break;
    }

    if mismatch_index.is_none() && sil.len() != board.len() {
        let i = min_len;
        mismatch_index = Some(i);
        mismatch_type = Some(TraceMismatchType::Edge);
        first_mismatch_tick = sil
            .get(i)
            .map(|e| e.tick)
            .or_else(|| board.get(i).map(|e| e.tick));
    }

    let is_match = mismatch_index.is_none();
    let context = if let Some(mi) = mismatch_index {
        build_context(sil, board, mi, context_window)
    } else {
        Vec::new()
    };

    TraceDiffReport {
        is_match,
        sil_events: sil.len(),
        board_events: board.len(),
        first_mismatch_tick,
        mismatch_type,
        mismatch_index,
        context_window,
        context,
    }
}

fn classify_mismatch(a: &NormalizedTraceEvent, b: &NormalizedTraceEvent) -> TraceMismatchType {
    if a.tick != b.tick || a.task != b.task {
        return TraceMismatchType::Edge;
    }
    if a.from_step != b.from_step || a.to_step != b.to_step {
        return TraceMismatchType::Step;
    }
    if a.reason != b.reason {
        return TraceMismatchType::Reason;
    }
    TraceMismatchType::Edge
}

fn build_context(
    sil: &[NormalizedTraceEvent],
    board: &[NormalizedTraceEvent],
    mismatch_index: usize,
    context_window: usize,
) -> Vec<TraceDiffContextRow> {
    let max_len = sil.len().max(board.len());
    if max_len == 0 {
        return Vec::new();
    }

    let start = mismatch_index.saturating_sub(context_window);
    let end_exclusive = (mismatch_index + context_window + 1).min(max_len);

    let mut out = Vec::new();
    for i in start..end_exclusive {
        out.push(TraceDiffContextRow {
            index: i,
            sil: sil.get(i).cloned(),
            board: board.get(i).cloned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_reports_match_when_sequences_equal() {
        let sil = vec![NormalizedTraceEvent {
            tick: 0,
            task: 0,
            from_step: 0,
            to_step: 1,
            reason: "action".to_string(),
        }];
        let board = sil.clone();

        let rep = diff_traces(&sil, &board, 2);
        assert!(rep.is_match);
        assert_eq!(rep.mismatch_index, None);
        assert!(rep.context.is_empty());
    }

    #[test]
    fn diff_reports_first_reason_mismatch_with_context() {
        let sil = vec![
            NormalizedTraceEvent {
                tick: 0,
                task: 0,
                from_step: 0,
                to_step: 1,
                reason: "action".to_string(),
            },
            NormalizedTraceEvent {
                tick: 1,
                task: 0,
                from_step: 1,
                to_step: 2,
                reason: "goto".to_string(),
            },
        ];
        let board = vec![
            sil[0].clone(),
            NormalizedTraceEvent {
                reason: "timeout".to_string(),
                ..sil[1].clone()
            },
        ];

        let rep = diff_traces(&sil, &board, 1);
        assert!(!rep.is_match);
        assert_eq!(rep.first_mismatch_tick, Some(1));
        assert_eq!(rep.mismatch_type, Some(TraceMismatchType::Reason));
        assert_eq!(rep.mismatch_index, Some(1));
        assert_eq!(rep.context.len(), 2);
        assert_eq!(rep.context[1].index, 1);
    }
}

