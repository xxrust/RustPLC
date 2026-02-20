import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';

interface ComponentLibraryProps {
  onDragStart: (type: string, label: string) => void;
}

const ComponentLibrary: React.FC<ComponentLibraryProps> = ({ onDragStart }) => {
  const { t } = useTranslation();
  const [search, setSearch] = useState('');
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const COMPONENT_TYPES = [
    { type: 'cylinder', label: t('componentLibrary.cylinder'), icon: '⊢', category: t('componentLibrary.actuators') },
    { type: 'sensor', label: t('componentLibrary.sensor'), icon: '◉', category: t('componentLibrary.sensors') },
    { type: 'switch', label: t('componentLibrary.switch'), icon: '⊣', category: t('componentLibrary.sensors') },
    { type: 'stepper_pd', label: t('componentLibrary.stepper'), icon: '⊙', category: t('componentLibrary.actuators') },
    { type: 'generic', label: t('componentLibrary.generic'), icon: '□', category: t('componentLibrary.other') },
  ];

  const CATEGORIES = [
    t('componentLibrary.actuators'),
    t('componentLibrary.sensors'),
    t('componentLibrary.other')
  ];

  const filtered = COMPONENT_TYPES.filter(
    (c) =>
      c.label.toLowerCase().includes(search.toLowerCase()) ||
      c.type.toLowerCase().includes(search.toLowerCase())
  );

  const toggleCategory = (cat: string) =>
    setCollapsed((prev) => ({ ...prev, [cat]: !prev[cat] }));

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Search */}
      <div style={{ padding: '8px 12px', borderBottom: '1px solid #3a3a3a' }}>
        <input
          type="text"
          placeholder={t('componentLibrary.searchPlaceholder')}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{
            width: '100%',
            background: '#1e1e1e',
            border: '1px solid #3a3a3a',
            borderRadius: 4,
            color: '#e0e0e0',
            padding: '4px 8px',
            fontSize: 12,
            outline: 'none',
          }}
        />
      </div>

      {/* Component list */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '8px 0' }}>
        {CATEGORIES.map((cat) => {
          const items = filtered.filter((c) => c.category === cat);
          if (items.length === 0) return null;
          const isCollapsed = collapsed[cat];

          return (
            <div key={cat}>
              <button
                onClick={() => toggleCategory(cat)}
                style={{
                  width: '100%',
                  background: 'none',
                  border: 'none',
                  cursor: 'pointer',
                  display: 'flex',
                  alignItems: 'center',
                  gap: 6,
                  padding: '4px 12px',
                  color: '#a0a0a0',
                  fontSize: 11,
                  textTransform: 'uppercase',
                  letterSpacing: '0.08em',
                }}
              >
                <span style={{ fontSize: 9 }}>{isCollapsed ? '▶' : '▼'}</span>
                {cat}
              </button>

              {!isCollapsed &&
                items.map((comp) => (
                  <div
                    key={comp.type}
                    draggable
                    onDragStart={() => onDragStart(comp.type, comp.label)}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      gap: 8,
                      padding: '6px 12px 6px 24px',
                      cursor: 'grab',
                      color: '#e0e0e0',
                      fontSize: 12,
                      borderRadius: 4,
                      margin: '1px 4px',
                      userSelect: 'none',
                    }}
                    onMouseEnter={(e) => (e.currentTarget.style.background = '#3a3a3a')}
                    onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
                  >
                    <span style={{ color: '#00bcd4', fontSize: 14, width: 16, textAlign: 'center' }}>
                      {comp.icon}
                    </span>
                    <span>{comp.label}</span>
                  </div>
                ))}
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default ComponentLibrary;
