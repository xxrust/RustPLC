use rust_plc::diagnostics::EvidenceSource;

pub(crate) fn evidence_source_label(source: EvidenceSource) -> &'static str {
    match source {
        EvidenceSource::NoBoard => "no_board",
        EvidenceSource::HilBoard => "hil_board",
        EvidenceSource::RuntimeLive => "runtime_live",
        EvidenceSource::Mixed => "mixed",
    }
}
