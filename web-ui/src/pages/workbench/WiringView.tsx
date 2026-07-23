import React, { useMemo, useState } from 'react';
import { SearchOutlined } from '@ant-design/icons';
import type { WiringPoint } from '../../types/workbench';
import { StatusPill, WorkbenchState } from '../../components/workbench/WorkbenchPrimitives';

const WiringView: React.FC<{ points: WiringPoint[] }> = ({ points }) => {
  const [query, setQuery] = useState('');
  const filtered = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return points;
    return points.filter((point) =>
      [point.point_id, point.controller, point.channel, point.alias, point.device_terminal, point.wire_id]
        .filter(Boolean)
        .some((value) => value?.toLowerCase().includes(needle))
    );
  }, [points, query]);

  return (
    <div className="wb-view wb-table-view">
      <header className="wb-view-header">
        <div><h1>Wiring and Point Checks</h1><p>Compiler-derived mapping and physical observation remain separate.</p></div>
        <label className="wb-filter-input">
          <SearchOutlined />
          <span className="wb-visually-hidden">Filter wiring points</span>
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Filter alias, channel, terminal, wire ID" />
        </label>
      </header>
      {points.length === 0 ? (
        <WorkbenchState kind="empty" title="No wiring artifact" detail="The project API has not returned compiler-derived controller I/O and device-terminal mappings." />
      ) : (
        <div className="wb-table-scroll">
          <table className="wb-data-table">
            <thead><tr><th>Controller / Channel</th><th>Semantic alias</th><th>Direction</th><th>Device terminal</th><th>Signal</th><th>Safe state</th><th>Wire ID</th><th>Compiler</th><th>Point check</th></tr></thead>
            <tbody>
              {filtered.map((point) => (
                <tr key={point.point_id}>
                  <td><strong>{point.controller ?? 'Unknown'}</strong><span>{point.channel ?? point.point_id}</span></td>
                  <td className="wb-mono">{point.alias ?? 'Unresolved alias'}</td>
                  <td>{point.direction ?? 'Unknown'}</td>
                  <td>{point.device_terminal ?? 'Unbound'}</td>
                  <td>{point.signal_type ?? 'Unknown'}</td>
                  <td>{point.safe_state ?? 'Missing'}</td>
                  <td className="wb-mono">{point.wire_id ?? 'Not assigned'}</td>
                  <td><StatusPill status={point.compiler_status} /></td>
                  <td><StatusPill status={point.point_check_status === 'pending' ? 'missing' : point.point_check_status} label={point.point_check_status ?? 'pending'} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
};

export default WiringView;
