import React from 'react';
import { LinkOutlined, SafetyCertificateOutlined } from '@ant-design/icons';
import type { DeliveryProjectDetail, EvidenceRecord, ReleaseProjection, WorkbenchTab } from '../../types/workbench';
import { StatusPill } from './WorkbenchPrimitives';
import { formatTime, shortCommit } from './workbenchUtils';

const EvidenceInspector: React.FC<{
  project?: DeliveryProjectDetail;
  releaseProjection?: ReleaseProjection;
  activeTab?: WorkbenchTab;
  evidence: EvidenceRecord[];
  onCollapse: () => void;
}> = ({ project, releaseProjection, activeTab, evidence, onCollapse }) => {
  const blocked = evidence.filter((item) => item.evidence_state === 'blocked');
  const stale = evidence.filter((item) => item.stale || item.evidence_state === 'stale');
  const selectedEvidence = evidence.find((item) => item.artifact_ref?.includes(activeTab?.resource_id ?? '')) ?? evidence[0];
  const hilHold = releaseProjection?.holds.find((hold) => hold.hold_id === 'hil_review');

  return (
    <aside className="wb-inspector" aria-label="Evidence inspector">
      <div className="wb-pane-heading"><strong>Evidence Inspector</strong><button type="button" onClick={onCollapse} aria-label="Collapse evidence inspector">›</button></div>
      <div className="wb-inspector-scroll">
        <section>
          <h2>Selection</h2>
          <dl className="wb-inspector-list">
            <div><dt>View</dt><dd>{activeTab?.label ?? 'No editor selected'}</dd></div>
            <div><dt>Project</dt><dd>{project?.project_id ?? 'No project'}</dd></div>
            <div><dt>Revision</dt><dd className="wb-mono">{shortCommit(project?.source_commit)}</dd></div>
            <div><dt>Layer</dt><dd>{project?.delivery_layer ?? 'Unknown'}</dd></div>
          </dl>
        </section>

        <section>
          <h2>Evidence summary</h2>
          <div className="wb-inspector-summary"><span><StatusPill status="blocked" /> {blocked.length}</span><span><StatusPill status="stale" /> {stale.length}</span><span><StatusPill status="verified" /> {evidence.filter((item) => item.evidence_state === 'verified').length}</span></div>
        </section>

        <section>
          <h2>Provenance</h2>
          {selectedEvidence ? (
            <div className="wb-provenance">
              <div><strong>{selectedEvidence.label}</strong><StatusPill status={selectedEvidence.evidence_state} /></div>
              <dl className="wb-inspector-list">
                <div><dt>Producer</dt><dd>{selectedEvidence.producer ?? 'Unknown'}</dd></div>
                <div><dt>Observed</dt><dd>{formatTime(selectedEvidence.timestamp)}</dd></div>
                <div><dt>Digest</dt><dd className="wb-mono wb-break">{selectedEvidence.digest ?? 'Not supplied'}</dd></div>
                <div><dt>Responsibility</dt><dd>{selectedEvidence.responsibility_state ?? 'Unassigned'}</dd></div>
              </dl>
              {selectedEvidence.artifact_ref && <a className="wb-link-button" href={artifactHref(selectedEvidence.artifact_ref)} target="_blank" rel="noreferrer"><LinkOutlined /> {selectedEvidence.artifact_ref}</a>}
            </div>
          ) : <p className="wb-empty-copy">Select an evidence-backed object to inspect its producer, revision, and artifact digest.</p>}
        </section>

        <section>
          <h2>Release boundary</h2>
          <div className="wb-release-boundary" data-release-status={releaseProjection?.status ?? project?.release_verdict ?? 'unknown'}>
            <SafetyCertificateOutlined />
            <div>
              <strong>{releaseProjection?.status ?? project?.release_verdict ?? 'Release not projected'}</strong>
              {releaseProjection ? (
                <dl className="wb-inspector-list wb-release-projection">
                  <div><dt>Delivery gate</dt><dd>{releaseProjection.delivery_status_gate.status}</dd></div>
                  <div><dt>Delivery status</dt><dd>{releaseProjection.delivery_status}</dd></div>
                  <div><dt>HIL review</dt><dd><StatusPill status={holdEvidenceState(hilHold?.status)} label={holdStatusLabel(hilHold?.status)} /></dd></div>
                  {hilHold?.reason && <div><dt>HIL reason</dt><dd>{hilHold.reason}</dd></div>}
                  {releaseProjection.delivery_status_gate.error_code && <div><dt>Gate code</dt><dd className="wb-mono">{releaseProjection.delivery_status_gate.error_code}</dd></div>}
                </dl>
              ) : <p>Release projection is unavailable.</p>}
            </div>
          </div>
          {releaseProjection && releaseProjection.blocked_prerequisites.length > 0 && (
            <div className="wb-prerequisite-list">
              <strong>Blocked prerequisites</strong>
              <ul>
                {releaseProjection.blocked_prerequisites.map((hold) => (
                  <li key={hold.hold_id}>
                    <span>{hold.hold_id.replaceAll('_', ' ')}</span>
                    <StatusPill status={holdEvidenceState(hold.status)} label={holdStatusLabel(hold.status)} />
                    {hold.reason && <small>{hold.reason}</small>}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </section>
      </div>
    </aside>
  );
};

function artifactHref(path: string): string {
  const normalized = path.replace(/\\/g, '/').replace(/^\/?api\/artifacts\//, '').replace(/^\/?artifacts\//, '').replace(/^\//, '');
  return `/api/artifacts/${normalized.split('/').map(encodeURIComponent).join('/')}`;
}

function holdStatusLabel(status?: string): string {
  if (!status) return 'unavailable';
  if (status === 'human_confirmed') return 'confirmed';
  if (status === 'human_action_required') return 'action required';
  return status.replaceAll('_', ' ');
}

function holdEvidenceState(status?: string): 'verified' | 'blocked' | 'stale' {
  if (status === 'human_confirmed') return 'verified';
  if (status === 'stale') return 'stale';
  return 'blocked';
}

export default EvidenceInspector;
