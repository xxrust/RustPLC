import React from 'react';
import { useTranslation } from 'react-i18next';

interface ValidationErrorPanelProps {
  errors: Array<{
    code: string;
    path: string;
    message: string;
  }>;
  onClose: () => void;
}

const ValidationErrorPanel: React.FC<ValidationErrorPanelProps> = ({ errors, onClose }) => {
  const { t } = useTranslation();
  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        background: 'rgba(0,0,0,0.7)',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 10000,
      }}
      onClick={onClose}
    >
      <div
        style={{
          width: 600,
          maxHeight: '80vh',
          background: '#2d2d2d',
          border: '1px solid #f5222d',
          borderRadius: 8,
          boxShadow: '0 8px 32px rgba(0,0,0,0.6)',
          overflow: 'hidden',
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div
          style={{
            padding: '16px 20px',
            background: '#f5222d22',
            borderBottom: '1px solid #f5222d',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <div
              style={{
                width: 32,
                height: 32,
                background: '#f5222d',
                borderRadius: '50%',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontSize: 18,
              }}
            >
              ⚠
            </div>
            <div>
              <h3 style={{ margin: 0, fontSize: 16, color: '#e0e0e0', fontWeight: 600 }}>
                {t('validation.title')}
              </h3>
              <p style={{ margin: '2px 0 0 0', fontSize: 12, color: '#a0a0a0' }}>
                {errors.length} {errors.length > 1 ? t('validation.errorsFoundPlural') : t('validation.errorsFound')}
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            style={{
              background: 'none',
              border: 'none',
              color: '#a0a0a0',
              fontSize: 24,
              cursor: 'pointer',
              padding: 0,
              width: 32,
              height: 32,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}
          >
            ×
          </button>
        </div>

        {/* Error list */}
        <div
          style={{
            maxHeight: 'calc(80vh - 140px)',
            overflowY: 'auto',
            padding: 20,
          }}
        >
          {errors.map((error, idx) => (
            <div
              key={idx}
              style={{
                marginBottom: 16,
                padding: 12,
                background: '#1e1e1e',
                border: '1px solid #3a3a3a',
                borderRadius: 6,
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
                <span
                  style={{
                    padding: '2px 6px',
                    background: '#f5222d22',
                    border: '1px solid #f5222d',
                    borderRadius: 3,
                    color: '#f5222d',
                    fontSize: 10,
                    fontWeight: 600,
                    textTransform: 'uppercase',
                    fontFamily: 'JetBrains Mono, monospace',
                  }}
                >
                  {error.code}
                </span>
                <span
                  style={{
                    fontSize: 11,
                    color: '#a0a0a0',
                    fontFamily: 'JetBrains Mono, monospace',
                  }}
                >
                  {error.path}
                </span>
              </div>
              <p
                style={{
                  margin: 0,
                  fontSize: 13,
                  color: '#e0e0e0',
                  lineHeight: 1.5,
                }}
              >
                {error.message}
              </p>
            </div>
          ))}
        </div>

        {/* Footer */}
        <div
          style={{
            padding: '12px 20px',
            background: '#1e1e1e',
            borderTop: '1px solid #3a3a3a',
            display: 'flex',
            justifyContent: 'flex-end',
          }}
        >
          <button
            onClick={onClose}
            style={{
              padding: '8px 16px',
              background: '#3a3a3a',
              border: 'none',
              borderRadius: 4,
              color: '#e0e0e0',
              fontSize: 12,
              fontWeight: 600,
              cursor: 'pointer',
            }}
          >
            {t('validation.close')}
          </button>
        </div>
      </div>
    </div>
  );
};

export default ValidationErrorPanel;
