use crate::board_trace::{TraceParseError, TraceRow, parse_trace_text};
use crate::tick_timing::{BoardTimingParseError, TickTimingSample, parse_tick_timing_text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardLogParseOutput {
    pub trace_rows: Vec<TraceRow>,
    pub timing_rows: Vec<TickTimingSample>,
}

#[derive(Debug, thiserror::Error)]
pub enum BoardLogParseError {
    #[error("TRACE parse error: {0}")]
    Trace(#[from] TraceParseError),

    #[error("TIMING parse error: {0}")]
    Timing(#[from] BoardTimingParseError),
}

/// Parse a board text log that may contain interleaved `TRACE ...` and `TIMING ...` records.
///
/// This is the single entrypoint for board-log parsing so CLI subcommands don't drift.
pub fn parse_board_log_text(input: &str) -> Result<BoardLogParseOutput, BoardLogParseError> {
    let trace_rows = parse_trace_text(input)?;
    let timing_rows = parse_tick_timing_text(input)?;
    Ok(BoardLogParseOutput {
        trace_rows,
        timing_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mixed_trace_and_timing_rows() {
        let input = r#"
boot ok
18:10:49.8655 [INFO] Script: TRACE tick=0 task=0 from=0 to=1 reason=action ts_ms=0
18:10:49.8656 [INFO] Script: TIMING tick=0 ts_start_us=0 ts_end_us=30 exec_us=30 slack_us=970 overrun=false
TRACE tick=1 task=0 from=1 to=2 reason=goto ts_ms=1
TIMING exec_us=120 tick=1 slack_us=880 ts_start_us=1000 ts_end_us=1120 overrun=true
"#;

        let parsed = parse_board_log_text(input).expect("parse ok");
        assert_eq!(parsed.trace_rows.len(), 2);
        assert_eq!(parsed.timing_rows.len(), 2);

        assert_eq!(parsed.trace_rows[0].tick, 0);
        assert_eq!(parsed.trace_rows[1].reason, "goto");

        assert_eq!(parsed.timing_rows[0].tick, 0);
        assert_eq!(parsed.timing_rows[1].overrun, true);
    }
}
