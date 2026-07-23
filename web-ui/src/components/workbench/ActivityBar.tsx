import React from 'react';
import {
  ApartmentOutlined,
  BranchesOutlined,
  CheckSquareOutlined,
  FileSearchOutlined,
  FolderOpenOutlined,
  HistoryOutlined,
  SearchOutlined,
} from '@ant-design/icons';
import type { ActivityId } from '../../stores/workbenchStore';

const activities: Array<{ id: ActivityId; label: string; icon: React.ReactNode }> = [
  { id: 'projects', label: 'Projects', icon: <FolderOpenOutlined /> },
  { id: 'runs', label: 'Agent Runs', icon: <HistoryOutlined /> },
  { id: 'wiring', label: 'Wiring', icon: <ApartmentOutlined /> },
  { id: 'verification', label: 'Verification', icon: <CheckSquareOutlined /> },
  { id: 'evidence', label: 'Evidence', icon: <FileSearchOutlined /> },
  { id: 'search', label: 'Search', icon: <SearchOutlined /> },
  { id: 'source-control', label: 'Source Control', icon: <BranchesOutlined /> },
];

const ActivityBar: React.FC<{
  active: ActivityId;
  onChange: (activity: ActivityId) => void;
  problemCount: number;
}> = ({ active, onChange, problemCount }) => (
  <nav className="wb-activity-bar" aria-label="Workbench activities">
    <div className="wb-activity-brand" aria-label="RustPLC">RP</div>
    {activities.map((activity) => (
      <button
        key={activity.id}
        type="button"
        className={active === activity.id ? 'is-active' : undefined}
        aria-current={active === activity.id ? 'page' : undefined}
        aria-label={activity.label}
        title={activity.label}
        onClick={() => onChange(activity.id)}
      >
        {activity.icon}
        {activity.id === 'verification' && problemCount > 0 && (
          <span className="wb-activity-badge" aria-label={`${problemCount} open problems`}>{problemCount}</span>
        )}
      </button>
    ))}
  </nav>
);

export default ActivityBar;
