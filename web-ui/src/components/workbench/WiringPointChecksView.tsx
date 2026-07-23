import React, { useMemo, useState } from 'react';
import {
  CameraOutlined,
  CloseOutlined,
  ExperimentOutlined,
  SaveOutlined,
  SearchOutlined,
} from '@ant-design/icons';
import { useAppStore } from '../../stores/appStore';
import type {
  PointCheckProjection,
  PointCheckProjectionPoint,
  PointObservationStatus,
  RecordPointObservationRequest,
} from '../../types/workbench';
import { StatusPill, WorkbenchState } from './WorkbenchPrimitives';
import { formatTime } from './workbenchUtils';

interface WiringPointChecksViewProps {
  points: PointCheckProjectionPoint[];
  summary?: PointCheckProjection['summary'];
  recordingPointId?: string;
  recordError?: string;
  onRecord: (pointId: string, request: RecordPointObservationRequest, photo?: File) => Promise<void>;
}

const WiringPointChecksView: React.FC<WiringPointChecksViewProps> = ({ points, summary, recordingPointId, recordError, onRecord }) => {
  const currentUser = useAppStore((state) => state.currentUser);
  const [query, setQuery] = useState('');
  const [selectedPoint, setSelectedPoint] = useState<PointCheckProjectionPoint>();
  const [status, setStatus] = useState<PointObservationStatus>('pass');
  const [measurementValue, setMeasurementValue] = useState('');
  const [measurementUnit, setMeasurementUnit] = useState('');
  const [instrumentId, setInstrumentId] = useState('');
  const [traceRef, setTraceRef] = useState('');
  const [note, setNote] = useState('');
  const [photo, setPhoto] = useState<File>();
  const [localError, setLocalError] = useState('');

  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return points;
    return points.filter((point) => {
      const authored = point.authored;
      return [point.point_id, authored.controller, authored.channel, authored.alias, authored.device_terminal, authored.wire_id]
        .filter(Boolean)
        .some((value) => value?.toLowerCase().includes(needle));
    });
  }, [points, query]);

  const canRecord = ['electrical_engineer', 'commissioning_engineer', 'admin'].includes(currentUser?.role ?? '');
  const hasEvidence = Boolean(measurementValue.trim() || photo || traceRef.trim() || note.trim());
  const isRecording = recordingPointId === selectedPoint?.point_id;

  const openObservation = (point: PointCheckProjectionPoint) => {
    setSelectedPoint(point);
    setStatus('pass');
    setMeasurementValue('');
    setMeasurementUnit('');
    setInstrumentId('');
    setTraceRef('');
    setNote('');
    setPhoto(undefined);
    setLocalError('');
  };

  const closeObservation = () => {
    if (!isRecording) setSelectedPoint(undefined);
  };

  const submitObservation = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!selectedPoint || !canRecord || !hasEvidence) return;
    if (!measurementValue.trim() && (measurementUnit.trim() || instrumentId.trim())) {
      setLocalError('Measurement value is required when unit or instrument is supplied.');
      return;
    }
    setLocalError('');
    const request: RecordPointObservationRequest = {
      status,
      measurement: measurementValue.trim() ? {
        value: measurementValue.trim(),
        unit: measurementUnit.trim() || undefined,
        instrument_id: instrumentId.trim() || undefined,
      } : undefined,
      trace_ref: traceRef.trim() || undefined,
      note: note.trim() || undefined,
    };
    try {
      await onRecord(selectedPoint.point_id, request, photo);
      setSelectedPoint(undefined);
    } catch {
      // Mutation state supplies the server error while preserving the entered evidence.
    }
  };

  return (
    <div className="wb-view wb-table-view">
      <header className="wb-view-header">
        <div>
          <h1>Wiring and Point Checks</h1>
          <p>Compiler mapping and accountable physical observations are reported as separate evidence.</p>
        </div>
        <div className="wb-wiring-header-tools">
          {summary && (
            <div className="wb-point-summary" aria-label="Point check projection summary">
              <span><strong>{summary.observed_points}</strong> observed</span>
              <span><strong>{summary.blocked_points}</strong> blocked</span>
              <span><strong>{summary.remaining_points}</strong> remaining</span>
            </div>
          )}
          <label className="wb-filter-input">
            <SearchOutlined />
            <span className="wb-visually-hidden">Filter wiring points</span>
            <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Filter alias, channel, terminal, wire ID" />
          </label>
        </div>
      </header>
      {points.length === 0 ? (
        <WorkbenchState kind="empty" title="No point-check projection" detail="The project API has not returned the authored wiring map and its physical evidence projection." />
      ) : (
        <div className="wb-table-scroll">
          <table className="wb-data-table wb-wiring-table">
            <thead><tr><th>Controller / Channel</th><th>Semantic alias</th><th>Direction</th><th>Device terminal</th><th>Signal</th><th>Safe state</th><th>Wire ID</th><th>Compiler</th><th>Point check</th><th>Latest observation</th><th><span className="wb-visually-hidden">Actions</span></th></tr></thead>
            <tbody>
              {filtered.map((point) => {
                const authored = point.authored;
                const latest = point.latest_observation;
                return (
                  <tr key={point.point_id}>
                    <td><strong>{authored.controller ?? 'Unknown'}</strong><span>{authored.channel ?? point.point_id}</span></td>
                    <td className="wb-mono">{authored.alias ?? 'Unresolved alias'}</td>
                    <td>{authored.direction ?? 'Unknown'}</td>
                    <td>{authored.device_terminal ?? 'Unbound'}</td>
                    <td>{authored.signal_type ?? 'Unknown'}</td>
                    <td>{authored.safe_state ?? 'Missing'}</td>
                    <td className="wb-mono">{authored.wire_id ?? 'Not assigned'}</td>
                    <td><StatusPill status={authored.compiler_status} /></td>
                    <td><StatusPill status={point.status === 'pending' ? 'missing' : point.status} label={point.status} /></td>
                    <td>{latest ? <span className="wb-latest-observation"><strong>{latest.user.name}</strong><small>{formatTime(latest.observed_at)} / {latest.status}</small></span> : <span className="wb-muted-copy">Not observed</span>}</td>
                    <td><button className="wb-icon-command" type="button" title={`Record evidence for ${authored.alias ?? point.point_id}`} aria-label={`Record evidence for ${authored.alias ?? point.point_id}`} onClick={() => openObservation(point)}><ExperimentOutlined /></button></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {selectedPoint && (
        <div className="wb-dialog-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) closeObservation(); }}>
          <form className="wb-point-dialog" role="dialog" aria-modal="true" aria-labelledby="point-observation-title" onSubmit={(event) => void submitObservation(event)}>
            <header>
              <div><h2 id="point-observation-title">Record point observation</h2><p>{selectedPoint.authored.alias ?? selectedPoint.point_id} / {selectedPoint.authored.channel ?? selectedPoint.point_id}</p></div>
              <button type="button" aria-label="Close point observation dialog" onClick={closeObservation} disabled={isRecording}><CloseOutlined /></button>
            </header>

            <div className="wb-point-accountability">
              <span>Observer</span><strong>{currentUser?.name ?? 'Unauthenticated'}</strong><span>Role</span><strong>{currentUser?.role ?? 'No role'}</strong>
            </div>

            <div className="wb-sign-decision" role="group" aria-label="Point observation status">
              {(['pass', 'fail', 'blocked'] as const).map((value) => <button key={value} type="button" aria-pressed={status === value} onClick={() => setStatus(value)}>{value}</button>)}
            </div>

            <fieldset className="wb-point-fieldset">
              <legend>Measurement</legend>
              <label><span>Value</span><input value={measurementValue} onChange={(event) => setMeasurementValue(event.target.value)} maxLength={256} /></label>
              <label><span>Unit</span><input value={measurementUnit} onChange={(event) => setMeasurementUnit(event.target.value)} maxLength={256} /></label>
              <label><span>Instrument ID</span><input value={instrumentId} onChange={(event) => setInstrumentId(event.target.value)} maxLength={256} /></label>
            </fieldset>

            <label className="wb-point-field"><span>Trace reference</span><input value={traceRef} onChange={(event) => setTraceRef(event.target.value)} placeholder="delivery-projects/.../trace.jsonl" /></label>
            <label className="wb-point-field"><span>Note</span><textarea value={note} onChange={(event) => setNote(event.target.value)} rows={3} maxLength={4096} /></label>
            <label className="wb-photo-input"><CameraOutlined /><span>{photo?.name ?? 'Attach point photo'}</span><input type="file" accept="image/*" onChange={(event) => setPhoto(event.target.files?.[0])} /></label>

            {!canRecord && <p className="wb-form-error">The authenticated role cannot record point-check evidence.</p>}
            {(localError || recordError) && <p className="wb-form-error" role="alert">{localError || recordError}</p>}

            <footer>
              <button className="wb-button" type="button" onClick={closeObservation} disabled={isRecording}>Cancel</button>
              <button className="wb-button wb-button--primary" type="submit" disabled={!canRecord || !hasEvidence || isRecording}><SaveOutlined /> {isRecording ? 'Recording...' : 'Record evidence'}</button>
            </footer>
          </form>
        </div>
      )}
    </div>
  );
};

export default WiringPointChecksView;
