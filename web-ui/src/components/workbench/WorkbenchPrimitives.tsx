import React from 'react';
import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleOutlined,
  ExclamationCircleOutlined,
  LoadingOutlined,
  MinusCircleOutlined,
  ReloadOutlined,
  StopOutlined,
} from '@ant-design/icons';
import type { EvidenceState } from '../../types/workbench';

const evidenceLabel: Record<EvidenceState, string> = {
  authored: 'Authored',
  derived: 'Derived',
  verified: 'Verified',
  observed: 'Observed',
  warning: 'Warning',
  blocked: 'Blocked',
  missing: 'Missing',
  stale: 'Stale',
};

const evidenceIcon: Record<EvidenceState, React.ReactNode> = {
  authored: <ClockCircleOutlined />,
  derived: <ClockCircleOutlined />,
  verified: <CheckCircleOutlined />,
  observed: <CheckCircleOutlined />,
  warning: <ExclamationCircleOutlined />,
  blocked: <StopOutlined />,
  missing: <MinusCircleOutlined />,
  stale: <ReloadOutlined />,
};

export const StatusPill: React.FC<{ status?: EvidenceState; label?: string }> = ({
  status = 'missing',
  label,
}) => (
  <span className={`wb-status wb-status--${status}`}>
    {evidenceIcon[status]}
    <span>{label ?? evidenceLabel[status]}</span>
  </span>
);

export const WorkbenchState: React.FC<{
  kind: 'loading' | 'empty' | 'error' | 'blocked' | 'stale';
  title: string;
  detail: string;
  onRetry?: () => void;
}> = ({ kind, title, detail, onRetry }) => {
  const icon =
    kind === 'loading' ? (
      <LoadingOutlined spin />
    ) : kind === 'error' ? (
      <CloseCircleOutlined />
    ) : kind === 'blocked' ? (
      <StopOutlined />
    ) : kind === 'stale' ? (
      <ReloadOutlined />
    ) : (
      <MinusCircleOutlined />
    );

  return (
    <div className={`wb-state wb-state--${kind}`} role={kind === 'error' ? 'alert' : 'status'}>
      <div className="wb-state__icon" aria-hidden="true">{icon}</div>
      <div>
        <strong>{title}</strong>
        <p>{detail}</p>
      </div>
      {onRetry && (
        <button className="wb-button" type="button" onClick={onRetry}>
          <ReloadOutlined /> Retry request
        </button>
      )}
    </div>
  );
};
