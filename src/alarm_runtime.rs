use crate::diagnostics::{DiagnosisCandidate, DiagnosisReport, EvidenceSource};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::thread;
use tungstenite::Message;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AlarmSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlarmEvent {
    pub alarm_id: String,
    pub severity: AlarmSeverity,
    pub first_seen_ms: u64,
    pub top_candidates: Vec<DiagnosisCandidate>,
    pub evidence_ref: String,
    pub evidence_source: EvidenceSource,
    pub scenario_or_recipe_id: String,
}

#[derive(Debug, Clone)]
pub struct AlarmBuildInput<'a> {
    pub diagnosis: &'a DiagnosisReport,
    pub severity: AlarmSeverity,
    pub first_seen_ms: u64,
    pub top_n: usize,
    pub evidence_ref: &'a str,
    pub evidence_source: EvidenceSource,
    pub scenario_or_recipe_id: &'a str,
}

pub fn build_alarm_event(input: AlarmBuildInput<'_>) -> AlarmEvent {
    let top_n = input.top_n.max(1);
    let top_candidates = input
        .diagnosis
        .candidates
        .iter()
        .take(top_n)
        .cloned()
        .collect::<Vec<_>>();
    let primary_issue_code = top_candidates
        .first()
        .map(|candidate| candidate.issue_code.as_str())
        .unwrap_or("DIAG-UNKNOWN");

    AlarmEvent {
        alarm_id: build_alarm_id(
            input.scenario_or_recipe_id,
            input.evidence_source,
            primary_issue_code,
        ),
        severity: input.severity,
        first_seen_ms: input.first_seen_ms,
        top_candidates,
        evidence_ref: input.evidence_ref.to_string(),
        evidence_source: input.evidence_source,
        scenario_or_recipe_id: input.scenario_or_recipe_id.to_string(),
    }
}

#[derive(Debug, Clone)]
pub struct AlarmDispatchConfig {
    pub audit_path: PathBuf,
    pub websocket_url: Option<String>,
    pub dedup_window_ms: u64,
    pub min_emit_interval_ms: u64,
    pub queue_capacity: usize,
}

impl AlarmDispatchConfig {
    pub fn with_audit_path(audit_path: PathBuf) -> Self {
        Self {
            audit_path,
            websocket_url: None,
            dedup_window_ms: 1_000,
            min_emit_interval_ms: 200,
            queue_capacity: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmPublishStatus {
    Enqueued,
    Deduplicated,
    RateLimited,
    QueueFullAuditFallback,
    ChannelClosedAuditFallback,
}

#[derive(Debug, thiserror::Error)]
pub enum AlarmDispatchError {
    #[error("failed to create alarm audit directory {path}: {message}")]
    CreateAuditDir { path: String, message: String },

    #[error("failed to open alarm audit file {path}: {message}")]
    OpenAuditFile { path: String, message: String },

    #[error("failed to serialize alarm event: {0}")]
    Serialize(String),

    #[error("failed to write alarm audit file {path}: {message}")]
    WriteAudit { path: String, message: String },

    #[error("alarm worker thread panicked")]
    WorkerPanicked,
}

pub struct AlarmDispatcher {
    sender: SyncSender<AlarmEvent>,
    worker: Option<thread::JoinHandle<()>>,
    limiter: Mutex<AlarmRateLimiter>,
    fallback_audit_path: PathBuf,
}

impl AlarmDispatcher {
    pub fn new(config: AlarmDispatchConfig) -> Result<Self, AlarmDispatchError> {
        let capacity = config.queue_capacity.max(1);
        ensure_parent_dir(&config.audit_path)?;
        // Truncate old audit output for deterministic runs in CLI/test usage.
        fs::File::create(&config.audit_path).map_err(|err| AlarmDispatchError::OpenAuditFile {
            path: config.audit_path.display().to_string(),
            message: err.to_string(),
        })?;

        let (tx, rx) = sync_channel::<AlarmEvent>(capacity);
        let worker_audit_path = config.audit_path.clone();
        let worker_ws = config.websocket_url.clone();

        let worker = thread::spawn(move || {
            run_alarm_worker(rx, &worker_audit_path, worker_ws.as_deref());
        });

        Ok(Self {
            sender: tx,
            worker: Some(worker),
            limiter: Mutex::new(AlarmRateLimiter::new(
                config.dedup_window_ms,
                config.min_emit_interval_ms,
            )),
            fallback_audit_path: config.audit_path,
        })
    }

    pub fn publish(&self, mut event: AlarmEvent) -> Result<AlarmPublishStatus, AlarmDispatchError> {
        let decision = {
            let mut limiter = self
                .limiter
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            limiter.should_emit(&event.alarm_id, event.first_seen_ms)
        };

        match decision {
            AlarmRateDecision::Deduplicated => Ok(AlarmPublishStatus::Deduplicated),
            AlarmRateDecision::RateLimited => Ok(AlarmPublishStatus::RateLimited),
            AlarmRateDecision::Emit { first_seen_ms } => {
                event.first_seen_ms = first_seen_ms;
                match self.sender.try_send(event) {
                    Ok(()) => Ok(AlarmPublishStatus::Enqueued),
                    Err(TrySendError::Full(event)) => {
                        append_alarm_event(&self.fallback_audit_path, &event)?;
                        Ok(AlarmPublishStatus::QueueFullAuditFallback)
                    }
                    Err(TrySendError::Disconnected(event)) => {
                        append_alarm_event(&self.fallback_audit_path, &event)?;
                        Ok(AlarmPublishStatus::ChannelClosedAuditFallback)
                    }
                }
            }
        }
    }

    pub fn close(mut self) -> Result<(), AlarmDispatchError> {
        drop(self.sender);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| AlarmDispatchError::WorkerPanicked)?;
        }
        Ok(())
    }
}

fn build_alarm_id(
    scenario_or_recipe_id: &str,
    evidence_source: EvidenceSource,
    primary_issue_code: &str,
) -> String {
    format!(
        "ALM-{}-{}-{}",
        sanitize_alarm_token(scenario_or_recipe_id),
        sanitize_alarm_token(evidence_source_label(evidence_source)),
        sanitize_alarm_token(primary_issue_code)
    )
}

fn sanitize_alarm_token(raw: &str) -> String {
    let out: String = raw
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "UNKNOWN".to_string()
    } else {
        out
    }
}

fn evidence_source_label(source: EvidenceSource) -> &'static str {
    match source {
        EvidenceSource::NoBoard => "no_board",
        EvidenceSource::HilBoard => "hil_board",
        EvidenceSource::RuntimeLive => "runtime_live",
        EvidenceSource::Mixed => "mixed",
    }
}

fn ensure_parent_dir(path: &Path) -> Result<(), AlarmDispatchError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|err| AlarmDispatchError::CreateAuditDir {
                path: parent.display().to_string(),
                message: err.to_string(),
            })?;
        }
    }
    Ok(())
}

fn append_alarm_event(path: &Path, event: &AlarmEvent) -> Result<(), AlarmDispatchError> {
    ensure_parent_dir(path)?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| AlarmDispatchError::OpenAuditFile {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;

    let mut writer = BufWriter::new(file);
    write_alarm_line(path, &mut writer, event)?;
    writer
        .flush()
        .map_err(|err| AlarmDispatchError::WriteAudit {
            path: path.display().to_string(),
            message: err.to_string(),
        })
}

fn write_alarm_line(
    path: &Path,
    writer: &mut BufWriter<fs::File>,
    event: &AlarmEvent,
) -> Result<(), AlarmDispatchError> {
    let line =
        serde_json::to_string(event).map_err(|err| AlarmDispatchError::Serialize(err.to_string()))?;
    writer
        .write_all(line.as_bytes())
        .map_err(|err| AlarmDispatchError::WriteAudit {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
    writer
        .write_all(b"\n")
        .map_err(|err| AlarmDispatchError::WriteAudit {
            path: path.display().to_string(),
            message: err.to_string(),
        })
}

fn run_alarm_worker(rx: Receiver<AlarmEvent>, audit_path: &Path, websocket_url: Option<&str>) {
    while let Ok(event) = rx.recv() {
        if append_alarm_event(audit_path, &event).is_err() {
            continue;
        }
        if let Some(url) = websocket_url {
            let _ = publish_to_websocket(url, &event);
        }
    }
}

fn publish_to_websocket(url: &str, event: &AlarmEvent) -> Result<(), String> {
    let payload = serde_json::to_string(event)
        .map_err(|err| format!("serialize alarm event for websocket failed: {err}"))?;
    let (mut socket, _) =
        tungstenite::connect(url).map_err(|err| format!("websocket connect failed for {url}: {err}"))?;
    socket
        .send(Message::Text(payload.into()))
        .map_err(|err| format!("websocket send failed for {url}: {err}"))?;
    let _ = socket.close(None);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum AlarmRateDecision {
    Emit { first_seen_ms: u64 },
    Deduplicated,
    RateLimited,
}

#[derive(Debug, Clone, Copy)]
struct AlarmSeenState {
    first_seen_ms: u64,
    last_emit_ms: u64,
}

#[derive(Debug)]
struct AlarmRateLimiter {
    dedup_window_ms: u64,
    min_emit_interval_ms: u64,
    seen: BTreeMap<String, AlarmSeenState>,
}

impl AlarmRateLimiter {
    fn new(dedup_window_ms: u64, min_emit_interval_ms: u64) -> Self {
        Self {
            dedup_window_ms,
            min_emit_interval_ms,
            seen: BTreeMap::new(),
        }
    }

    fn should_emit(&mut self, alarm_id: &str, occurred_ms: u64) -> AlarmRateDecision {
        let entry = self
            .seen
            .entry(alarm_id.to_string())
            .or_insert(AlarmSeenState {
                first_seen_ms: occurred_ms,
                last_emit_ms: occurred_ms,
            });

        if entry.first_seen_ms == occurred_ms && entry.last_emit_ms == occurred_ms {
            return AlarmRateDecision::Emit {
                first_seen_ms: entry.first_seen_ms,
            };
        }

        if occurred_ms <= entry.first_seen_ms.saturating_add(self.dedup_window_ms) {
            return AlarmRateDecision::Deduplicated;
        }

        if occurred_ms < entry.last_emit_ms.saturating_add(self.min_emit_interval_ms) {
            return AlarmRateDecision::RateLimited;
        }

        entry.last_emit_ms = occurred_ms;
        AlarmRateDecision::Emit {
            first_seen_ms: entry.first_seen_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{AnchorKind, DiagnosisAnchor, DiagnosisCategory, DiagnosisReport};
    use std::fs;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::time::Duration;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock works")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn diagnosis_fixture() -> DiagnosisReport {
        DiagnosisReport {
            schema_version: 1,
            anchors: vec![DiagnosisAnchor {
                kind: AnchorKind::Timeout,
                tick: Some(2),
                trace_index: Some(3),
                detail: "timeout observed".to_string(),
            }],
            candidates: vec![DiagnosisCandidate {
                issue_code: "DIAG-IN-001".to_string(),
                category: DiagnosisCategory::ExpectedInputNeverChanged,
                rank: 1,
                confidence: 0.91,
                evidence: vec!["no DI edge observed".to_string()],
                suggested_fix: "inject DI earlier".to_string(),
                evidence_source: EvidenceSource::RuntimeLive,
            }],
        }
    }

    #[test]
    fn build_alarm_event_contract_includes_required_fields() {
        let diagnosis = diagnosis_fixture();
        let event = build_alarm_event(AlarmBuildInput {
            diagnosis: &diagnosis,
            severity: AlarmSeverity::Critical,
            first_seen_ms: 120,
            top_n: 3,
            evidence_ref: "out/runtime/trace.jsonl",
            evidence_source: EvidenceSource::RuntimeLive,
            scenario_or_recipe_id: "recipe_a",
        });

        assert!(event.alarm_id.starts_with("ALM-"));
        assert_eq!(event.severity, AlarmSeverity::Critical);
        assert_eq!(event.first_seen_ms, 120);
        assert_eq!(event.top_candidates.len(), 1);
        assert_eq!(event.evidence_ref, "out/runtime/trace.jsonl");
        assert_eq!(event.evidence_source, EvidenceSource::RuntimeLive);
        assert_eq!(event.scenario_or_recipe_id, "recipe_a");
    }

    #[test]
    fn rate_limiter_deduplicates_and_rate_limits_same_alarm_id() {
        let mut limiter = AlarmRateLimiter::new(1_000, 200);

        let first = limiter.should_emit("ALM-1", 1_000);
        assert!(matches!(first, AlarmRateDecision::Emit { .. }));

        let dedup = limiter.should_emit("ALM-1", 1_100);
        assert!(matches!(dedup, AlarmRateDecision::Deduplicated));

        let allowed = limiter.should_emit("ALM-1", 2_050);
        assert!(matches!(allowed, AlarmRateDecision::Emit { .. }));

        let rate_limited = limiter.should_emit("ALM-1", 2_100);
        assert!(matches!(rate_limited, AlarmRateDecision::RateLimited));
    }

    #[test]
    fn dispatcher_writes_audit_when_websocket_unavailable() {
        let base = temp_dir("alarm_dispatch_ws_down");
        let audit_path = base.join("alarm_events.ndjson");

        let dispatcher = AlarmDispatcher::new(AlarmDispatchConfig {
            audit_path: audit_path.clone(),
            websocket_url: Some("ws://127.0.0.1:9/alarm".to_string()),
            dedup_window_ms: 1_000,
            min_emit_interval_ms: 200,
            queue_capacity: 8,
        })
        .expect("dispatcher init");

        let status = dispatcher
            .publish(build_alarm_event(AlarmBuildInput {
                diagnosis: &diagnosis_fixture(),
                severity: AlarmSeverity::Critical,
                first_seen_ms: 10,
                top_n: 2,
                evidence_ref: "out/trace.jsonl",
                evidence_source: EvidenceSource::RuntimeLive,
                scenario_or_recipe_id: "runtime_a",
            }))
            .expect("publish should not fail");

        assert_eq!(status, AlarmPublishStatus::Enqueued);
        dispatcher.close().expect("close dispatcher");

        let body = fs::read_to_string(&audit_path).expect("read audit");
        let lines = body.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);
        let event: AlarmEvent = serde_json::from_str(lines[0]).expect("valid alarm json");
        assert_eq!(event.scenario_or_recipe_id, "runtime_a");
    }

    #[test]
    fn dispatcher_supports_websocket_realtime_channel() {
        let base = temp_dir("alarm_dispatch_ws_ok");
        let audit_path = base.join("alarm_events.ndjson");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = mpsc::channel::<String>();

        let server_thread = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept websocket client");
            let mut ws = tungstenite::accept(stream).expect("websocket accept");
            let msg = ws.read().expect("read websocket message");
            let text = msg.into_text().expect("text frame");
            tx.send(text.to_string()).expect("send received payload");
        });

        let dispatcher = AlarmDispatcher::new(AlarmDispatchConfig {
            audit_path,
            websocket_url: Some(format!("ws://{addr}/alarm")),
            dedup_window_ms: 1_000,
            min_emit_interval_ms: 200,
            queue_capacity: 8,
        })
        .expect("dispatcher init");

        dispatcher
            .publish(build_alarm_event(AlarmBuildInput {
                diagnosis: &diagnosis_fixture(),
                severity: AlarmSeverity::Critical,
                first_seen_ms: 99,
                top_n: 1,
                evidence_ref: "out/trace.jsonl",
                evidence_source: EvidenceSource::RuntimeLive,
                scenario_or_recipe_id: "runtime_live_recipe",
            }))
            .expect("publish alarm");

        dispatcher.close().expect("close dispatcher");

        let payload = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("receive websocket payload");
        let event: AlarmEvent = serde_json::from_str(&payload).expect("alarm event payload");
        assert_eq!(event.scenario_or_recipe_id, "runtime_live_recipe");
        assert_eq!(event.evidence_source, EvidenceSource::RuntimeLive);

        server_thread.join().expect("server thread join");
    }
}
