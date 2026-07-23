import React, { useEffect, useMemo, useRef, useState } from 'react';
import { MacCommandOutlined, SearchOutlined } from '@ant-design/icons';

const COMMAND_RENDER_LIMIT = 80;

export interface WorkbenchCommand {
  id: string;
  label: string;
  category: string;
  shortcut?: string;
  detail?: string;
  searchText?: string;
  execute: () => void;
}

const CommandPalette: React.FC<{
  open: boolean;
  commands: WorkbenchCommand[];
  onClose: () => void;
}> = ({ open, commands, onClose }) => {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const filtered = useMemo(() => {
    const tokens = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
    if (tokens.length === 0) return commands;
    return commands.filter((command) => {
      const searchable = `${command.category} ${command.label} ${command.detail ?? ''} ${command.searchText ?? ''}`.toLowerCase();
      return tokens.every((token) => searchable.includes(token));
    });
  }, [commands, query]);
  const visible = filtered.slice(0, COMMAND_RENDER_LIMIT);

  useEffect(() => {
    if (!open) return;
    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => {
      window.clearTimeout(timer);
      previouslyFocused?.focus();
    };
  }, [open]);

  if (!open) return null;
  const activeIndex = Math.min(selectedIndex, Math.max(0, visible.length - 1));

  const execute = (command?: WorkbenchCommand) => {
    if (!command) return;
    command.execute();
    onClose();
  };

  return (
    <div className="wb-command-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section className="wb-command-palette" role="dialog" aria-modal="true" aria-label="Command palette" onKeyDown={(event) => {
        if (event.key === 'Tab') {
          const focusable = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('input, button:not(:disabled)'));
          const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
          const nextIndex = event.shiftKey
            ? (currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1)
            : (currentIndex >= focusable.length - 1 ? 0 : currentIndex + 1);
          event.preventDefault();
          focusable[nextIndex]?.focus();
        }
      }}>
        <label className="wb-command-input">
          <SearchOutlined />
          <span className="wb-visually-hidden">Search workbench commands</span>
          <input
            ref={inputRef}
            value={query}
            onInput={(event) => { setQuery(event.currentTarget.value); setSelectedIndex(0); }}
            placeholder="Open project, view, artifact, diagnostic, or layout command"
            role="combobox"
            aria-expanded="true"
            aria-controls="wb-command-results"
            aria-activedescendant={visible[activeIndex] ? `wb-command-${visible[activeIndex].id}` : undefined}
            onKeyDown={(event) => {
              if (event.key === 'Escape') onClose();
              if (event.key === 'ArrowDown') { event.preventDefault(); setSelectedIndex((index) => Math.min(index + 1, visible.length - 1)); }
              if (event.key === 'ArrowUp') { event.preventDefault(); setSelectedIndex((index) => Math.max(0, index - 1)); }
              if (event.key === 'Enter') { event.preventDefault(); execute(visible[activeIndex]); }
            }}
          />
          <kbd>Esc</kbd>
        </label>
        <div id="wb-command-results" className="wb-command-results" role="listbox" aria-label="Commands">
          {visible.length > 0 ? visible.map((command, index) => (
            <button
              id={`wb-command-${command.id}`}
              key={command.id}
              type="button"
              role="option"
              aria-selected={activeIndex === index}
              className={activeIndex === index ? 'is-selected' : undefined}
              onMouseEnter={() => setSelectedIndex(index)}
              onClick={() => execute(command)}
            >
              <MacCommandOutlined />
              <span><strong>{command.label}</strong><small>{command.category}{command.detail ? ` · ${command.detail}` : ''}</small></span>
              {command.shortcut && <kbd>{command.shortcut}</kbd>}
            </button>
          )) : (
            <p>No command matches “{query}”.</p>
          )}
          {filtered.length > visible.length && (
            <p className="wb-command-result-summary" role="status">
              Showing {visible.length} of {filtered.length} matches. Refine the query to reach additional commands.
            </p>
          )}
        </div>
      </section>
    </div>
  );
};

export default CommandPalette;
