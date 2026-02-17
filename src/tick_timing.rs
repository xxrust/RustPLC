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

#[cfg(test)]
mod tests {
    use super::{TickTimingSample, parse_tick_timing_jsonl, to_tick_timing_jsonl};

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
}
