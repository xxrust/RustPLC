import React, { useMemo } from 'react';
import type { EvidenceRecord, EvidenceState, VerificationStage } from '../../types/workbench';
import { StatusPill, WorkbenchState } from '../../components/workbench/WorkbenchPrimitives';
import { formatTime, shortCommit } from '../../components/workbench/workbenchUtils';

const filters: Array<'all' | EvidenceState> = ['all', 'verified', 'observed', 'warning', 'blocked', 'stale'];

const VerificationEvidenceView: React.FC<{
  stages: VerificationStage[];
  evidence: EvidenceRecord[];
  filter: 'all' | EvidenceState;
  onFilterChange: (filter: 'all' | EvidenceState) => void;
}> = ({ stages, evidence, filter, onFilterChange }) => {
  const filtered = useMemo(
    () => evidence.filter((item) => filter === 'all' || item.evidence_state === filter),
    [evidence, filter]
  );

  return (
    <div className="wb-view wb-verification-view">
      <header className="wb-view-header">
        <div><h1>Verification and Evidence</h1><p>Formal proof, runtime observation, and human responsibility are reported as separate axes.</p></div>
        <div className="wb-segmented" role="group" aria-label="Evidence state filter">
          {filters.map((item) => (
            <button key={item} type="button" aria-pressed={filter === item} onClick={() => onFilterChange(item)}>{item}</button>
          ))}
        </div>
      </header>

      <div className="wb-verification-layout">
        <section className="wb-stage-list" aria-label="Compiler stages">
          <div className="wb-section-heading"><h2>Compiler stages</h2><span>{stages.length} indexed</span></div>
          {stages.length > 0 ? stages.map((stage) => (
            <button className="wb-stage-row" type="button" key={stage.stage} onClick={() => openArtifact(stage.artifact_ref)} disabled={!stage.artifact_ref}>
              <div><strong>{stage.stage}</strong><span>{stage.message ?? stage.producer ?? 'Artifact-backed result'}</span></div>
              <StatusPill status={stage.status} />
              <code>{stage.diagnostic_code ?? stage.evidence_source_type ?? shortCommit(stage.source_commit)}</code>
            </button>
          )) : (
            <WorkbenchState kind="empty" title="No verification stages" detail="Compiler-owned stage artifacts will appear here when indexed by the delivery-project API." />
          )}
        </section>

        <section className="wb-evidence-list" aria-label="Evidence records">
          <div className="wb-section-heading"><h2>Evidence records</h2><span>{filtered.length} visible</span></div>
          {evidence.length === 0 ? (
            <WorkbenchState kind="empty" title="No evidence records" detail="Import a harness run or execute a project gate to populate evidence provenance." />
          ) : filtered.length === 0 ? (
            <WorkbenchState kind="empty" title="No matching evidence" detail={`No records currently have the ${filter} evidence state.`} />
          ) : filtered.map((item) => (
            <article className="wb-evidence-row" key={item.evidence_id}>
              <div className="wb-evidence-row__main">
                <div><strong>{item.label}</strong><StatusPill status={item.evidence_state} /></div>
                <p>{item.blocker_reason ?? item.artifact_ref ?? 'Artifact reference not supplied'}</p>
              </div>
              <dl>
                <div><dt>Producer</dt><dd>{item.producer ?? 'Unknown'}</dd></div>
                <div><dt>Source type</dt><dd>{item.evidence_source_type ?? 'Artifact'}</dd></div>
                <div><dt>Responsibility</dt><dd>{item.responsibility_state ?? 'Unassigned'}</dd></div>
                <div><dt>Revision</dt><dd className="wb-mono">{shortCommit(item.source_commit)}</dd></div>
                <div><dt>Timestamp</dt><dd>{formatTime(item.timestamp)}</dd></div>
              </dl>
              {item.artifact_ref && <button className="wb-link-button" type="button" onClick={() => openArtifact(item.artifact_ref)}>Open artifact</button>}
            </article>
          ))}
        </section>
      </div>
    </div>
  );
};

function openArtifact(path?: string): void {
  if (!path) return;
  const normalized = path.replace(/\\/g, '/').replace(/^\/?api\/artifacts\//, '').replace(/^\/?artifacts\//, '').replace(/^\//, '');
  window.open(`/api/artifacts/${normalized.split('/').map(encodeURIComponent).join('/')}`, '_blank', 'noopener,noreferrer');
}

export default VerificationEvidenceView;
