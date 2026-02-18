use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TickTimingSample {
    pub tick: u64,
    pub ts_start_us: u64,
    pub ts_end_us: u64,
    pub exec_us: u64,
    pub slack_us: u64,
    pub overrun: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum TickTimingParseError {
    #[error("line {line}: invalid JSON: {message}")]
    InvalidJson { line: usize, message: String },
}

pub fn parse_tick_timing_jsonl(input: &str) -> Result<Vec<TickTimingSample>, TickTimingParseError> {
    let mut out = Vec::new();
    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let sample: TickTimingSample =
            serde_json::from_str(line).map_err(|err| TickTimingParseError::InvalidJson {
                line: line_no,
                message: err.to_string(),
            })?;
        out.push(sample);
    }
    Ok(out)
}

pub fn to_tick_timing_jsonl(samples: &[TickTimingSample]) -> Result<String, serde_json::Error> {
    let mut out = String::new();
    for sample in samples {
        let mut line = serde_json::to_string(sample)?;
        line.push('\n');
        out.push_str(&line);
    }
    Ok(out)
}

#[derive(Debug, thiserror::Error)]
pub enum BoardTimingParseError {
    #[error("line {line}: expected TIMING record, got: {text}")]
    NotTiming { line: usize, text: String },

    #[error("line {line}: invalid field {field:?} in token {token:?}")]
    InvalidField {
        line: usize,
        field: &'static str,
        token: String,
    },

    #[error("line {line}: missing required field {field:?}")]
    MissingField { line: usize, field: &'static str },
}

/// Parse board text logs into structured tick timing samples.
///
/// Expected line format (order may vary, extra fields are ignored):
/// `TIMING tick=<u64> ts_start_us=<u64> ts_end_us=<u64> exec_us=<u64> slack_us=<u64> overrun=<bool>`
pub fn parse_tick_timing_text(input: &str) -> Result<Vec<TickTimingSample>, BoardTimingParseError> {
    let mut out = Vec::new();

    for (idx, raw) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let Some(timing_start) = line.find("TIMING ") else {
            continue;
        };
        let line = &line[timing_start..];

        let mut tick: Option<u64> = None;
        let mut ts_start_us: Option<u64> = None;
        let mut ts_end_us: Option<u64> = None;
        let mut exec_us: Option<u64> = None;
        let mut slack_us: Option<u64> = None;
        let mut overrun: Option<bool> = None;

        for token in line.split_whitespace().skip(1) {
            let (k, v) = token
                .split_once('=')
                .ok_or_else(|| BoardTimingParseError::NotTiming {
                    line: line_no,
                    text: line.to_string(),
                })?;
            match k {
                "tick" => tick = Some(parse_u64(line_no, "tick", token, v)?),
                "ts_start_us" => ts_start_us = Some(parse_u64(line_no, "ts_start_us", token, v)?),
                "ts_end_us" => ts_end_us = Some(parse_u64(line_no, "ts_end_us", token, v)?),
                "exec_us" => exec_us = Some(parse_u64(line_no, "exec_us", token, v)?),
                "slack_us" => slack_us = Some(parse_u64(line_no, "slack_us", token, v)?),
                "overrun" => overrun = Some(parse_bool(line_no, "overrun", token, v)?),
                _ => {}
            }
        }

        let tick = tick.ok_or(BoardTimingParseError::MissingField {
            line: line_no,
            field: "tick",
        })?;
        let ts_start_us = ts_start_us.ok_or(BoardTimingParseError::MissingField {
            line: line_no,
            field: "ts_start_us",
        })?;
        let ts_end_us = ts_end_us.ok_or(BoardTimingParseError::MissingField {
            line: line_no,
            field: "ts_end_us",
        })?;
        let exec_us = exec_us.ok_or(BoardTimingParseError::MissingField {
            line: line_no,
            field: "exec_us",
        })?;
        let slack_us = slack_us.ok_or(BoardTimingParseError::MissingField {
            line: line_no,
            field: "slack_us",
        })?;
        let overrun = overrun.ok_or(BoardTimingParseError::MissingField {
            line: line_no,
            field: "overrun",
        })?;

        out.push(TickTimingSample {
            tick,
            ts_start_us,
            ts_end_us,
            exec_us,
            slack_us,
            overrun,
        });
    }

    Ok(out)
}

fn parse_u64(
    line: usize,
    field: &'static str,
    token: &str,
    v: &str,
) -> Result<u64, BoardTimingParseError> {
    v.parse().map_err(|_| BoardTimingParseError::InvalidField {
        line,
        field,
        token: token.to_string(),
    })
}

fn parse_bool(
    line: usize,
    field: &'static str,
    token: &str,
    v: &str,
) -> Result<bool, BoardTimingParseError> {
    v.parse().map_err(|_| BoardTimingParseError::InvalidField {
        line,
        field,
        token: token.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        BoardTimingParseError, TickTimingSample, parse_tick_timing_jsonl, parse_tick_timing_text,
        to_tick_timing_jsonl,
    };

    #[test]
    fn roundtrip_jsonl_keeps_fields_order_and_values() {
        let samples = vec![TickTimingSample {
            tick: 0,
            ts_start_us: 0,
            ts_end_us: 30,
            exec_us: 30,
            slack_us: 970,
            overrun: false,
        }];

        let jsonl = to_tick_timing_jsonl(&samples).expect("serialize tick timing");
        assert!(
            jsonl.contains(
                "\"tick\":0,\"ts_start_us\":0,\"ts_end_us\":30,\"exec_us\":30,\"slack_us\":970,\"overrun\":false"
            ),
            "serialized row should keep stable field order: {jsonl}"
        );

        let parsed = parse_tick_timing_jsonl(&jsonl).expect("parse tick timing");
        assert_eq!(parsed, samples);
    }

    #[test]
    fn parses_timing_lines_out_of_order_ignoring_other_output() {
        let input = r#"
boot ok
TICK tick=7 ts_ms=7
TIMING exec_us=120 tick=7 slack_us=880 ts_start_us=7000 ts_end_us=7120 overrun=false overrun_count=0
"#;
        let rows = parse_tick_timing_text(input).expect("parse ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            TickTimingSample {
                tick: 7,
                ts_start_us: 7000,
                ts_end_us: 7120,
                exec_us: 120,
                slack_us: 880,
                overrun: false,
            }
        );
    }

    #[test]
    fn parses_timing_lines_with_log_prefix_and_extra_fields() {
        let input = r#"
18:10:49.8655 [INFO] Script: TIMING tick=7 ts_start_us=7000 foo=bar ts_end_us=7120 exec_us=120 slack_us=880 overrun=true overrun_count=3
"#;
        let rows = parse_tick_timing_text(input).expect("parse ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tick, 7);
        assert_eq!(rows[0].overrun, true);
    }

    #[test]
    fn timing_missing_field_returns_locatable_error() {
        let input = "TIMING tick=1 ts_start_us=0 ts_end_us=10 exec_us=10 overrun=false\n";
        let err = parse_tick_timing_text(input).expect_err("should fail");
        match err {
            BoardTimingParseError::MissingField { line, field } => {
                assert_eq!(line, 1);
                assert_eq!(field, "slack_us");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
