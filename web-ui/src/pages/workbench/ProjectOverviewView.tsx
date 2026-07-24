import React, { useMemo, useState } from 'react';
import {
  BranchesOutlined,
  CodeOutlined,
  FileTextOutlined,
  LockOutlined,
  RobotOutlined,
  SafetyCertificateOutlined,
  ToolOutlined,
} from '@ant-design/icons';
import type {
  AgentRun,
  DeliveryProjectDetail,
  EvidenceState,
  EvidenceRecord,
  HoldSignatureContext,
  HumanHold,
  SignHoldRequest,
  VerificationStage,
} from '../../types/workbench';
import { StatusPill } from '../../components/workbench/WorkbenchPrimitives';
import { useDialogFocus } from '../../components/workbench/useDialogFocus';
import { formatTime, shortCommit } from '../../components/workbench/workbenchUtils';
import { useAppStore } from '../../stores/appStore';

interface ProjectOverviewViewProps {
  project: DeliveryProjectDetail;
  runs: AgentRun[];
  verification: VerificationStage[];
  evidence: EvidenceRecord[];
  signatureContext?: HoldSignatureContext;
  signingHoldId?: string;
  onSign: (holdId: string, request: SignHoldRequest) => Promise<void>;
}

const ProjectOverviewView: React.FC<ProjectOverviewViewProps> = ({
  project,
  runs,
  verification,
  evidence,
  signatureContext,
  signingHoldId,
  onSign,
}) => {
  const currentUser = useAppStore((state) => state.currentUser);
  const [selectedHold, setSelectedHold] = useState<HumanHold | null>(null);
  const [decision, setDecision] = useState<'approve' | 'reject'>('approve');
  const [comment, setComment] = useState('');
  const [acknowledged, setAcknowledged] = useState(false);
  const { dialogRef, onDialogKeyDown } = useDialogFocus<HTMLFormElement>(Boolean(selectedHold), () => setSelectedHold(null));
  const latestRun = runs[0];
  const blockedStages = verification.filter((stage) => stage.status === 'blocked');
  const verifiedStages = verification.filter((stage) => stage.status === 'verified').length;
  const staleEvidence = evidence.filter((item) => item.stale || item.evidence_state === 'stale');
  const sourceAuthoringVerdict = latestRun?.attribution?.source_authoring_verdict
    ?? latestRun?.unattended_verdict
    ?? 'not_proven';
  const pointCheckHold = project.human_holds?.find((hold) => hold.hold_id === 'point_check_completion');
  const confirmedHoldCount = project.human_holds?.filter((hold) => hold.status === 'confirmed').length ?? 0;
  const holdCount = project.human_holds?.length ?? 0;
  const compilerState: EvidenceState = verification.length === 0
    ? 'missing'
    : blockedStages.length > 0
      ? 'blocked'
      : verifiedStages === verification.length
        ? 'verified'
        : 'warning';
  const physicalState = holdStateToEvidence(pointCheckHold?.status);
  const releaseState = releaseVerdictToEvidence(project.release_verdict);
  const signaturesByHold = useMemo(() => {
    const signatures = [...(signatureContext?.signatures ?? [])]
      .sort((left, right) => right.signed_at_ms - left.signed_at_ms);
    return new Map(signatures.map((signature) => [signature.hold_id, signature]));
  }, [signatureContext?.signatures]);

  const openSignature = (hold: HumanHold) => {
    setSelectedHold(hold);
    setDecision('approve');
    setComment('');
    setAcknowledged(false);
  };

  const closeSignature = () => setSelectedHold(null);

  const submitSignature = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!selectedHold || !signatureContext || !acknowledged) return;
    await onSign(selectedHold.hold_id, {
      hold_type: selectedHold.hold_id,
      attestation_standard: signatureContext.attestation_standard,
      source_commit: signatureContext.source_commit,
      evidence_digests: signatureContext.current_evidence_digests,
      decision,
      comment: comment.trim() || undefined,
    });
    closeSignature();
  };

  return (
    <div className="wb-view wb-overview">
      <header className="wb-view-header">
        <div>
          <div className="wb-view-title-row">
            <h1>{project.name ?? project.project_id}</h1>
            <StatusPill status={project.stale ? 'stale' : project.status} />
          </div>
          <p>{project.project_id} / {project.delivery_layer} delivery asset</p>
        </div>
        <div className="wb-header-facts">
          <span><BranchesOutlined /> {shortCommit(project.source_commit)}</span>
          <span><FileTextOutlined /> {project.source_entry ?? 'Source entry missing'}</span>
          <span><LockOutlined /> {project.responsibility_state ?? 'Responsibility unreported'}</span>
        </div>
      </header>

      {(project.stale || staleEvidence.length > 0) && (
        <div className="wb-notice wb-notice--stale">
          <StatusPill status="stale" />
          <span>Evidence revision differs from the selected source commit. Review digests before approval.</span>
        </div>
      )}

      <div className="wb-overview-grid">
        <section className="wb-section wb-section--wide wb-responsibility-section" aria-labelledby="delivery-responsibility-title">
          <div className="wb-section-heading">
            <h2 id="delivery-responsibility-title">Delivery responsibility chain</h2>
            <span>Source to release</span>
          </div>
          <div className="wb-responsibility-chain" role="list" aria-label="Delivery responsibility and evidence chain">
            <div
              className="wb-responsibility-step"
              role="listitem"
              data-responsibility-stage="agent-authoring"
              data-responsibility-owner="agent"
              data-verdict={sourceAuthoringVerdict}
            >
              <span className="wb-responsibility-icon" aria-hidden="true"><RobotOutlined /></span>
              <div className="wb-responsibility-title"><strong>Agent source authoring</strong><small>Agent-owned</small></div>
              <StatusPill status={verdictToEvidence(sourceAuthoringVerdict)} label={humanizeVerdict(sourceAuthoringVerdict)} />
              <p>{sourceAuthoringDetail(latestRun)}</p>
            </div>
            <div
              className="wb-responsibility-step"
              role="listitem"
              data-responsibility-stage="compiler-verification"
              data-responsibility-owner="compiler"
              data-verdict={compilerState}
            >
              <span className="wb-responsibility-icon" aria-hidden="true"><CodeOutlined /></span>
              <div className="wb-responsibility-title"><strong>Compiler verification</strong><small>RustPLC-owned</small></div>
              <StatusPill status={compilerState} />
              <p>{verifiedStages} of {verification.length} indexed stages verified; {blockedStages.length} blocked.</p>
            </div>
            <div
              className="wb-responsibility-step"
              role="listitem"
              data-responsibility-stage="physical-validation"
              data-responsibility-owner="human"
              data-verdict={pointCheckHold?.status ?? 'pending'}
            >
              <span className="wb-responsibility-icon" aria-hidden="true"><ToolOutlined /></span>
              <div className="wb-responsibility-title"><strong>Wiring and point checks</strong><small>Human-owned</small></div>
              <StatusPill status={physicalState} label={humanizeVerdict(pointCheckHold?.status ?? 'pending')} />
              <p>{confirmedHoldCount} of {holdCount} human hold points confirmed. Physical observations remain independent evidence.</p>
            </div>
            <div
              className="wb-responsibility-step"
              role="listitem"
              data-responsibility-stage="release-authorization"
              data-responsibility-owner="human"
              data-verdict={project.release_verdict ?? 'not_issued'}
            >
              <span className="wb-responsibility-icon" aria-hidden="true"><LockOutlined /></span>
              <div className="wb-responsibility-title"><strong>Release authorization</strong><small>Human-owned</small></div>
              <StatusPill status={releaseState} label={humanizeVerdict(project.release_verdict ?? 'not_issued')} />
              <p>The release decision is bound to the current source revision and evidence digests.</p>
            </div>
          </div>
        </section>

        <section className="wb-section">
          <div className="wb-section-heading"><h2>Delivery contract</h2><span>Authored boundaries</span></div>
          <dl className="wb-definition-list">
            <div><dt>System contract</dt><dd>{project.system_contract ?? 'Missing required artifact'}</dd></div>
            <div><dt>Architecture</dt><dd>{project.architecture ?? 'Not indexed'}</dd></div>
            <div><dt>Source revision</dt><dd>{project.source_commit}</dd></div>
            <div><dt>Release verdict</dt><dd>{project.release_verdict ?? 'Not issued'}</dd></div>
          </dl>
        </section>

        <section className="wb-section">
          <div className="wb-section-heading"><h2>Latest unattended run</h2><span>{latestRun ? formatTime(latestRun.completed_at ?? latestRun.started_at) : 'No run evidence'}</span></div>
          {latestRun ? (
            <dl className="wb-definition-list">
              <div><dt>Run ID</dt><dd>{latestRun.run_id}</dd></div>
              <div><dt>Verdict</dt><dd>{latestRun.unattended_verdict ?? latestRun.status ?? 'Unreported'}</dd></div>
              <div><dt>Model</dt><dd>{latestRun.model ?? 'Not recorded'}</dd></div>
              <div><dt>Input manifest</dt><dd className="wb-mono">{latestRun.input_manifest_digest ?? 'Digest missing'}</dd></div>
            </dl>
          ) : <p className="wb-empty-copy">Run provenance appears after the first imported or executed agent run.</p>}
        </section>

        <section className="wb-section wb-section--wide">
          <div className="wb-section-heading"><h2>Compiler evidence chain</h2><span>{blockedStages.length} blocked stages</span></div>
          <div className="wb-pipeline" role="list" aria-label="Compiler evidence chain">
            {verification.length > 0 ? verification.map((stage) => (
              <div className="wb-pipeline-row" role="listitem" key={stage.stage}>
                <strong>{stage.stage}</strong>
                <StatusPill status={stage.status} />
                <span>{stage.diagnostic_code ?? stage.producer ?? 'Compiler artifact'}</span>
                <span className="wb-mono">{shortCommit(stage.source_commit ?? project.source_commit)}</span>
              </div>
            )) : <p className="wb-empty-copy">Verification stages will appear when the project API indexes compiler artifacts.</p>}
          </div>
        </section>

        <section className="wb-section wb-section--wide">
          <div className="wb-section-heading"><h2>Human safety holds</h2><span>Responsibility remains independent from compiler status</span></div>
          <div className="wb-hold-list">
            {(project.human_holds ?? []).length > 0 ? project.human_holds?.map((hold) => {
              const signature = signaturesByHold.get(hold.hold_id);
              const authorized = canSignHold(currentUser?.role, hold.hold_id);
              const signable = authorized && hold.status !== 'blocked' && hold.status !== 'confirmed';
              return (
                <div className="wb-hold-row" key={hold.hold_id}>
                  <strong>{hold.label}</strong>
                  <span>{hold.role ?? 'Role not assigned'}</span>
                  <span className={`wb-hold-state wb-hold-state--${hold.status}`}>{hold.status.replaceAll('_', ' ')}</span>
                  <span className="wb-hold-action">
                    <span>{signature ? `${signature.user.name} / ${signature.decision}${signature.stale ? ' / stale' : ''}` : hold.reason ?? 'Awaiting signature'}</span>
                    {signable && <button className="wb-button" type="button" onClick={() => openSignature(hold)} disabled={signingHoldId === hold.hold_id}><SafetyCertificateOutlined /> Sign</button>}
                    {!authorized && <small>{hold.role ?? 'Assigned role'} required</small>}
                  </span>
                </div>
              );
            }) : <p className="wb-empty-copy">No hold-point records were returned. Release approval remains unproven.</p>}
          </div>
        </section>
      </div>

      {selectedHold && signatureContext && (
        <div className="wb-dialog-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) closeSignature(); }}>
          <form ref={dialogRef} className="wb-sign-dialog" role="dialog" aria-modal="true" aria-labelledby="hold-sign-title" tabIndex={-1} onKeyDown={onDialogKeyDown} onSubmit={submitSignature}>
            <header><div><h2 id="hold-sign-title">Sign {selectedHold.label}</h2><p>{selectedHold.role ?? 'Assigned reviewer'} responsibility</p></div><button type="button" aria-label="Close signature dialog" data-dialog-autofocus onClick={closeSignature}>x</button></header>
            <dl className="wb-sign-facts">
              <div><dt>Project</dt><dd>{project.project_id}</dd></div>
              <div><dt>Attestation</dt><dd>{signatureContext.attestation_standard}</dd></div>
              <div><dt>Source commit</dt><dd className="wb-mono">{signatureContext.source_commit}</dd></div>
              <div><dt>Evidence set</dt><dd>{Object.keys(signatureContext.current_evidence_digests).length} current artifact digests</dd></div>
            </dl>
            <p className="wb-sign-scope">{signatureContext.attestation_scope}</p>
            <div className="wb-digest-preview">
              {Object.entries(signatureContext.current_evidence_digests).slice(0, 4).map(([path, digest]) => <div key={path}><span>{path}</span><code>{digest}</code></div>)}
              {Object.keys(signatureContext.current_evidence_digests).length > 4 && <p>+ {Object.keys(signatureContext.current_evidence_digests).length - 4} additional digests bound to this signature</p>}
            </div>
            <div className="wb-sign-decision" role="group" aria-label="Signature decision">
              <button type="button" aria-pressed={decision === 'approve'} onClick={() => setDecision('approve')}>Approve</button>
              <button type="button" aria-pressed={decision === 'reject'} onClick={() => setDecision('reject')}>Reject</button>
            </div>
            <label className="wb-sign-comment"><span>Review comment</span><textarea value={comment} onChange={(event) => setComment(event.target.value)} rows={3} placeholder="Record the review basis or rejection condition" /></label>
            <label className="wb-sign-attestation"><input type="checkbox" checked={acknowledged} onChange={(event) => setAcknowledged(event.target.checked)} /><span>I attest that I reviewed the named source revision and current evidence set. This signature records my human engineering decision and does not replace physical validation.</span></label>
            <footer><button className="wb-button" type="button" onClick={closeSignature}>Cancel</button><button className="wb-button wb-button--primary" type="submit" disabled={!acknowledged || signingHoldId === selectedHold.hold_id}>{signingHoldId === selectedHold.hold_id ? 'Signing...' : `${decision === 'approve' ? 'Approve' : 'Reject'} and sign`}</button></footer>
          </form>
        </div>
      )}
    </div>
  );
};

function canSignHold(role: string | undefined, holdId: string): boolean {
  if (role === 'admin') return true;
  return (
    (role === 'electrical_engineer' && holdId === 'wiring_review')
    || (role === 'commissioning_engineer' && ['point_check_completion', 'hil_review'].includes(holdId))
    || (role === 'safety_reviewer' && holdId === 'safety_review')
    || (role === 'release_approver' && holdId === 'release_approval')
  );
}

function verdictToEvidence(verdict: string): EvidenceState {
  const normalized = verdict.toLowerCase().replaceAll(' ', '_');
  if (['proven', 'verified', 'complete', 'approved', 'release_approved'].includes(normalized)) return 'verified';
  if (['blocked', 'failed', 'rejected', 'human_intervention_detected', 'not_releasable'].includes(normalized)) return 'blocked';
  if (['missing', 'not_recorded', 'not_issued'].includes(normalized)) return 'missing';
  return 'warning';
}

function holdStateToEvidence(status: HumanHold['status'] | undefined): EvidenceState {
  if (status === 'confirmed') return 'observed';
  if (status === 'blocked' || status === 'rejected') return 'blocked';
  if (status === 'stale') return 'stale';
  if (status === 'pending') return 'warning';
  return 'missing';
}

function releaseVerdictToEvidence(verdict: string | undefined): EvidenceState {
  return verdict ? verdictToEvidence(verdict) : 'missing';
}

function humanizeVerdict(verdict: string): string {
  return verdict.replaceAll('_', ' ');
}

function sourceAuthoringDetail(run: AgentRun | undefined): string {
  const recordCount = run?.attribution?.source_authoring_record_count ?? 0;
  return recordCount > 0
    ? `${recordCount} file-level source records bind authorship to this run.`
    : 'No file-level source records bind authored PLC source to this run.';
}

export default ProjectOverviewView;
