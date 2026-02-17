use crate::tick_timing::TickTimingSample;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingReport {
    pub schema_version: u32,
    pub count: u64,
    pub overrun_count: u64,
    pub exec_us_min: u64,
    pub exec_us_max: u64,
    pub exec_us_p50: u64,
    pub exec_us_p95: u64,
    pub exec_us_p99: u64,
    pub exec_us_mean: f64,
}

pub fn build_timing_report(samples: &[TickTimingSample]) -> Option<TimingReport> {
    if samples.is_empty() {
        return None;
    }

    let mut exec_values: Vec<u64> = samples.iter().map(|s| s.exec_us).collect();
    exec_values.sort_unstable();

    let count = exec_values.len() as u64;
    let overrun_count = samples.iter().filter(|s| s.overrun).count() as u64;
    let exec_us_min = *exec_values.first().unwrap_or(&0);
    let exec_us_max = *exec_values.last().unwrap_or(&0);
    let total_exec: u128 = exec_values.iter().map(|v| *v as u128).sum();
    let exec_us_mean = (total_exec as f64) / (count as f64);

    Some(TimingReport {
        schema_version: 1,
        count,
        overrun_count,
        exec_us_min,
        exec_us_max,
        exec_us_p50: percentile_nearest_rank(&exec_values, 50),
        exec_us_p95: percentile_nearest_rank(&exec_values, 95),
        exec_us_p99: percentile_nearest_rank(&exec_values, 99),
        exec_us_mean,
    })
}

fn percentile_nearest_rank(sorted_values: &[u64], percentile: usize) -> u64 {
    debug_assert!(!sorted_values.is_empty());
    debug_assert!(percentile >= 1 && percentile <= 100);
    let n = sorted_values.len();
    let rank = ((n * percentile) + 99) / 100;
    let index = rank.saturating_sub(1).min(n.saturating_sub(1));
    sorted_values[index]
}

#[cfg(test)]
mod tests {
    use super::build_timing_report;
    use crate::tick_timing::TickTimingSample;

    #[test]
    fn build_timing_report_computes_expected_percentiles() {
        let mut samples = Vec::new();
        for (tick, exec_us) in [90_u64, 10, 60, 20, 100, 70, 30, 80, 40, 50]
            .into_iter()
            .enumerate()
        {
            samples.push(TickTimingSample {
                tick: tick as u64,
                ts_start_us: (tick as u64) * 1_000,
                ts_end_us: (tick as u64) * 1_000 + exec_us,
                exec_us,
                slack_us: 1_000_u64.saturating_sub(exec_us),
                overrun: exec_us >= 95,
            });
        }

        let report = build_timing_report(&samples).expect("report should exist");
        assert_eq!(report.count, 10);
        assert_eq!(report.overrun_count, 1);
        assert_eq!(report.exec_us_min, 10);
        assert_eq!(report.exec_us_max, 100);
        assert_eq!(report.exec_us_p50, 50);
        assert_eq!(report.exec_us_p95, 100);
        assert_eq!(report.exec_us_p99, 100);
        assert!((report.exec_us_mean - 55.0).abs() < 1e-9);
    }

    #[test]
    fn build_timing_report_returns_none_for_empty_input() {
        assert!(build_timing_report(&[]).is_none());
    }
}
