use crate::optimization::OptimizationCandidate;

pub fn rank_candidates(candidates: &mut [OptimizationCandidate]) {
    candidates.sort_by(|left, right| {
        right
            .legality
            .is_legal
            .cmp(&left.legality.is_legal)
            .then_with(|| {
                left.timing
                    .global_nominal_ms
                    .cmp(&right.timing.global_nominal_ms)
            })
            .then_with(|| left.wait_points_after.cmp(&right.wait_points_after))
            .then_with(|| left.change_cost.cmp(&right.change_cost))
            .then_with(|| left.id.cmp(&right.id))
    });
}

#[cfg(test)]
mod tests {
    use super::rank_candidates;
    use crate::optimization::{
        CandidateLegality, CandidateRewrite, CandidateRewriteKind, CandidateTimingSummary,
        OptimizationCandidate,
    };

    #[test]
    fn ranks_by_legality_then_time_then_waits_then_change_cost() {
        let mut candidates = vec![
            candidate("slow-legal", true, 200, 2, 2),
            candidate("fast-illegal", false, 10, 0, 0),
            candidate("fast-legal-more-waits", true, 100, 3, 1),
            candidate("fast-legal-fewer-waits", true, 100, 1, 2),
            candidate("fast-legal-same-waits-smaller-change", true, 100, 1, 1),
        ];

        rank_candidates(&mut candidates);

        assert_eq!(candidates[0].id, "fast-legal-same-waits-smaller-change");
        assert_eq!(candidates[1].id, "fast-legal-fewer-waits");
        assert_eq!(candidates[2].id, "fast-legal-more-waits");
        assert_eq!(candidates[3].id, "slow-legal");
        assert_eq!(candidates[4].id, "fast-illegal");
    }

    fn candidate(
        id: &str,
        legal: bool,
        nominal_ms: u64,
        wait_points_after: usize,
        change_cost: usize,
    ) -> OptimizationCandidate {
        OptimizationCandidate {
            id: id.to_string(),
            rewrite: CandidateRewrite {
                kind: CandidateRewriteKind::SwapAdjacentSteps,
                task: "main".to_string(),
                summary: id.to_string(),
                affected_steps: vec!["a".to_string(), "b".to_string()],
            },
            timing: CandidateTimingSummary {
                global_nominal_ms: nominal_ms,
                global_worst_case_ms: nominal_ms,
                sequential_nominal_ms: nominal_ms,
                sequential_worst_case_ms: nominal_ms,
                active_tasks: vec![("main".to_string(), nominal_ms)],
            },
            legality: CandidateLegality {
                is_legal: legal,
                diagnostics: Vec::new(),
            },
            wait_points_after,
            change_cost,
            source: String::new(),
        }
    }
}
