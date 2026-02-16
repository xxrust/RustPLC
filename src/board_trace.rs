use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceRow {
    pub tick: u64,
    pub task: usize,
    pub from_step: u16,
    pub to_step: u16,
    pub reason: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TraceParseError {
    #[error("line {line}: expected TRACE record, got: {text}")]
    NotTrace { line: usize, text: String },

    #[error("line {line}: invalid field {field:?} in token {token:?}")]
    InvalidField {
        line: usize,
        field: &'static str,
        token: String,
    },

    #[error("line {line}: missing required field {field:?}")]
    MissingField { line: usize, field: &'static str },
}

/// Parse board text logs into structured trace events.
///
/// Expected line format (order may vary):
/// `TRACE tick=<u64> task=<usize> from=<u16> to=<u16> reason=<str> ts_ms=<u64>`
pub fn parse_trace_text(input: &str) -> Result<Vec<TraceRow>, TraceParseError> {
    let mut out = Vec::new();

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some(trace_start) = line.find("TRACE ") else {
            continue;
        };
        let line = &line[trace_start..];
        let mut tick: Option<u64> = None;
        let mut task: Option<usize> = None;
        let mut from_step: Option<u16> = None;
        let mut to_step: Option<u16> = None;
        let mut reason: Option<String> = None;
        let mut ts_ms: Option<u64> = None;

        for token in line.split_whitespace().skip(1) {
            let (k, v) = token
                .split_once('=')
                .ok_or_else(|| TraceParseError::NotTrace {
                    line: line_no,
                    text: line.to_string(),
                })?;
            match k {
                "tick" => tick = Some(parse_u64(line_no, "tick", token, v)?),
                "task" => task = Some(parse_usize(line_no, "task", token, v)?),
                "from" => from_step = Some(parse_u16(line_no, "from", token, v)?),
                "to" => to_step = Some(parse_u16(line_no, "to", token, v)?),
                "reason" => reason = Some(v.to_string()),
                "ts_ms" => ts_ms = Some(parse_u64(line_no, "ts_ms", token, v)?),
                _ => {}
            }
        }

        let tick = tick.ok_or(TraceParseError::MissingField {
            line: line_no,
            field: "tick",
        })?;
        let task = task.ok_or(TraceParseError::MissingField {
            line: line_no,
            field: "task",
        })?;
        let from_step = from_step.ok_or(TraceParseError::MissingField {
            line: line_no,
            field: "from",
        })?;
        let to_step = to_step.ok_or(TraceParseError::MissingField {
            line: line_no,
            field: "to",
        })?;
        let reason = reason.ok_or(TraceParseError::MissingField {
            line: line_no,
            field: "reason",
        })?;
        let timestamp_ms = ts_ms.ok_or(TraceParseError::MissingField {
            line: line_no,
            field: "ts_ms",
        })?;

        out.push(TraceRow {
            tick,
            task,
            from_step,
            to_step,
            reason,
            timestamp_ms,
        });
    }

    Ok(out)
}

fn parse_u64(
    line: usize,
    field: &'static str,
    token: &str,
    v: &str,
) -> Result<u64, TraceParseError> {
    v.parse().map_err(|_| TraceParseError::InvalidField {
        line,
        field,
        token: token.to_string(),
    })
}

fn parse_u16(
    line: usize,
    field: &'static str,
    token: &str,
    v: &str,
) -> Result<u16, TraceParseError> {
    v.parse().map_err(|_| TraceParseError::InvalidField {
        line,
        field,
        token: token.to_string(),
    })
}

fn parse_usize(
    line: usize,
    field: &'static str,
    token: &str,
    v: &str,
) -> Result<usize, TraceParseError> {
    v.parse().map_err(|_| TraceParseError::InvalidField {
        line,
        field,
        token: token.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trace_lines_ignoring_other_output() {
        let input = r#"
boot ok
TICK tick=0 ts_ms=0
TRACE tick=0 task=0 from=0 to=1 reason=action ts_ms=0
TRACE task=0 tick=1 from=1 to=2 reason=goto ts_ms=1
"#;
        let rows = parse_trace_text(input).expect("parse ok");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            TraceRow {
                tick: 0,
                task: 0,
                from_step: 0,
                to_step: 1,
                reason: "action".to_string(),
                timestamp_ms: 0
            }
        );
        assert_eq!(rows[1].tick, 1);
        assert_eq!(rows[1].reason, "goto");
    }

    #[test]
    fn parses_trace_lines_with_log_prefix() {
        let input = r#"
18:10:49.8655 [INFO] Script: TRACE tick=0 task=0 from=0 to=1 reason=action ts_ms=0
18:10:49.8656 [INFO] Script: TRACE tick=1 task=0 from=1 to=2 reason=goto ts_ms=1
"#;
        let rows = parse_trace_text(input).expect("parse ok");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].tick, 0);
        assert_eq!(rows[1].tick, 1);
    }
}
