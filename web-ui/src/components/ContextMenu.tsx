import React, { useEffect, useRef } from 'react';

export interface MenuItem {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  badge?: string; // e.g., "native", "approximate", "pending"
  danger?: boolean;
}

interface ContextMenuProps {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

const ContextMenu: React.FC<ContextMenuProps> = ({ x, y, items, onClose }) => {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    document.addEventListener('keydown', handleEscape);

    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
      document.removeEventListener('keydown', handleEscape);
    };
  }, [onClose]);

  const getBadgeColor = (badge?: string) => {
    switch (badge) {
      case 'native':
        return '#52c41a';
      case 'approximate':
        return '#faad14';
      case 'pending':
        return '#8c8c8c';
      default:
        return '#8c8c8c';
    }
  };

  return (
    <div
      ref={menuRef}
      style={{
        position: 'fixed',
        left: x,
        top: y,
        background: '#2d2d2d',
        border: '1px solid #3a3a3a',
        borderRadius: 4,
        boxShadow: '0 4px 12px rgba(0,0,0,0.5)',
        minWidth: 180,
        zIndex: 10000,
        padding: '4px 0',
      }}
    >
      {items.map((item, idx) => (
        <button
          key={idx}
          onClick={() => {
            if (!item.disabled) {
              item.onClick();
              onClose();
            }
          }}
          disabled={item.disabled}
          style={{
            width: '100%',
            padding: '6px 12px',
            background: 'transparent',
            border: 'none',
            color: item.danger ? '#f5222d' : item.disabled ? '#5a5a5a' : '#e0e0e0',
            fontSize: 12,
            textAlign: 'left',
            cursor: item.disabled ? 'not-allowed' : 'pointer',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            fontFamily: 'system-ui, -apple-system, sans-serif',
          }}
          onMouseEnter={(e) => {
            if (!item.disabled) {
              e.currentTarget.style.background = '#3a3a3a';
            }
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = 'transparent';
          }}
        >
          <span>{item.label}</span>
          {item.badge && (
            <span
              style={{
                fontSize: 9,
                padding: '1px 4px',
                borderRadius: 2,
                background: getBadgeColor(item.badge),
                color: '#fff',
                fontWeight: 600,
                textTransform: 'uppercase',
              }}
            >
              {item.badge}
            </span>
          )}
        </button>
      ))}
    </div>
  );
};

export default ContextMenu;
