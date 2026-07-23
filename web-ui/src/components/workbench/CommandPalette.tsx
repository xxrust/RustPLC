import React, { useEffect, useMemo, useRef, useState } from 'react';
import { MacCommandOutlined, SearchOutlined } from '@ant-design/icons';
import type { WorkbenchSearchField, WorkbenchSearchIndex } from '../../types/workbench';

const COMMAND_RENDER_LIMIT = 80;
const FIELD_ALIASES: Record<string, WorkbenchSearchField> = {
  project: 'project',
  layer: 'layer',
  stage: 'stage',
  diagnostic: 'diagnostic',
  code: 'diagnostic',
  evidence: 'evidence',
  evidence_state: 'evidence',
  'evidence-state': 'evidence',
  commit: 'commit',
  source_commit: 'commit',
  'source-commit': 'commit',
  revision: 'commit',
  status: 'status',
  responsibility: 'responsibility',
  producer: 'producer',
  run: 'run',
  test: 'test',
  suite: 'suite',
  verdict: 'verdict',
  model: 'model',
  category: 'category',
};

interface SearchToken {
  field?: WorkbenchSearchField;
  value: string;
}

export interface WorkbenchCommand {
  id: string;
  label: string;
  category: string;
  shortcut?: string;
  detail?: string;
  searchText?: string;
  search?: WorkbenchSearchIndex;
  execute: () => void;
}

function tokenizeQuery(query: string): string[] {
  const tokens: string[] = [];
  let token = '';
  let quoted = false;
  for (const character of query.trim()) {
    if (character === '"') {
      quoted = !quoted;
      continue;
    }
    if (/\s/.test(character) && !quoted) {
      if (token) tokens.push(token);
      token = '';
      continue;
    }
    token += character;
  }
  if (token) tokens.push(token);
  return tokens;
}

function parseWorkbenchSearchQuery(query: string): SearchToken[] {
  return tokenizeQuery(query).map((rawToken) => {
    const separator = rawToken.indexOf(':');
    if (separator <= 0) return { value: rawToken.toLowerCase() };
    const field = FIELD_ALIASES[rawToken.slice(0, separator).toLowerCase()];
    if (!field) return { value: rawToken.toLowerCase() };
    return { field, value: rawToken.slice(separator + 1).toLowerCase() };
  }).filter((token) => token.value.length > 0);
}

function searchValues(command: WorkbenchCommand, field: WorkbenchSearchField): string[] {
  const indexed = command.search?.[field];
  const values = Array.isArray(indexed) ? indexed : [indexed];
  if (field === 'category') values.push(command.category);
  return values.filter((value): value is string => Boolean(value)).map((value) => value.toLowerCase());
}

function matchesCommand(command: WorkbenchCommand, tokens: SearchToken[]): boolean {
  const searchable = [
    command.category,
    command.label,
    command.detail,
    command.searchText,
    ...Object.values(command.search ?? {}).flatMap((value) => Array.isArray(value) ? value : [value]),
  ].filter((value): value is string => Boolean(value)).join(' ').toLowerCase();
  return tokens.every((token) => token.field
    ? searchValues(command, token.field).some((value) => value.includes(token.value))
    : searchable.includes(token.value));
}

function dataValue(command: WorkbenchCommand, field: WorkbenchSearchField): string | undefined {
  const values = searchValues(command, field);
  return values.length > 0 ? values.join(' ') : undefined;
}

const CommandPalette: React.FC<{
  open: boolean;
  commands: WorkbenchCommand[];
  initialQuery?: string;
  onQueryChange?: (query: string) => void;
  onClose: () => void;
}> = ({ open, commands, initialQuery = '', onQueryChange, onClose }) => {
  const [query, setQuery] = useState(initialQuery);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const filtered = useMemo(() => {
    const tokens = parseWorkbenchSearchQuery(query);
    if (tokens.length === 0) return commands;
    return commands.filter((command) => matchesCommand(command, tokens));
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

  const updateQuery = (value: string) => {
    setQuery(value);
    setSelectedIndex(0);
    onQueryChange?.(value);
  };

  const execute = (command?: WorkbenchCommand) => {
    if (!command) return;
    command.execute();
    onClose();
  };

  return (
    <div className="wb-command-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section
        className="wb-command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            onClose();
            return;
          }
          if (event.key !== 'Tab') return;
          const focusable = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('input, button:not(:disabled)'));
          const currentIndex = focusable.indexOf(document.activeElement as HTMLElement);
          const nextIndex = event.shiftKey
            ? (currentIndex <= 0 ? focusable.length - 1 : currentIndex - 1)
            : (currentIndex >= focusable.length - 1 ? 0 : currentIndex + 1);
          event.preventDefault();
          focusable[nextIndex]?.focus();
        }}
      >
        <label className="wb-command-input">
          <SearchOutlined />
          <span className="wb-visually-hidden">Search projects, compiler stages, diagnostics, evidence, and source commits</span>
          <input
            ref={inputRef}
            value={query}
            onChange={(event) => updateQuery(event.currentTarget.value)}
            placeholder="Open project, artifact, diagnostic, or command"
            role="combobox"
            aria-expanded="true"
            aria-controls="wb-command-results"
            aria-describedby="wb-command-result-count"
            aria-activedescendant={visible[activeIndex] ? `wb-command-${visible[activeIndex].id}` : undefined}
            onKeyDown={(event) => {
              if (event.key === 'ArrowDown') { event.preventDefault(); setSelectedIndex((index) => Math.min(index + 1, visible.length - 1)); }
              if (event.key === 'ArrowUp') { event.preventDefault(); setSelectedIndex((index) => Math.max(0, index - 1)); }
              if (event.key === 'Home') { event.preventDefault(); setSelectedIndex(0); }
              if (event.key === 'End') { event.preventDefault(); setSelectedIndex(Math.max(0, visible.length - 1)); }
              if (event.key === 'Enter') { event.preventDefault(); execute(visible[activeIndex]); }
            }}
          />
          <kbd>Esc</kbd>
        </label>
        <div
          id="wb-command-results"
          className="wb-command-results"
          role="listbox"
          aria-label="Commands"
          data-query={query}
          data-filtered-count={filtered.length}
        >
          {visible.length > 0 ? visible.map((command, index) => (
            <button
              id={`wb-command-${command.id}`}
              key={command.id}
              type="button"
              role="option"
              aria-selected={activeIndex === index}
              className={activeIndex === index ? 'is-selected' : undefined}
              data-command-id={command.id}
              data-search-project={dataValue(command, 'project')}
              data-search-stage={dataValue(command, 'stage')}
              data-search-diagnostic={dataValue(command, 'diagnostic')}
              data-search-evidence={dataValue(command, 'evidence')}
              data-search-commit={dataValue(command, 'commit')}
              data-search-status={dataValue(command, 'status')}
              onMouseEnter={() => setSelectedIndex(index)}
              onClick={() => execute(command)}
            >
              <MacCommandOutlined />
              <span><strong>{command.label}</strong><small>{command.category}{command.detail ? ` / ${command.detail}` : ''}</small></span>
              {command.shortcut && <kbd>{command.shortcut}</kbd>}
            </button>
          )) : (
            <p>No command matches &quot;{query}&quot;.</p>
          )}
          <p id="wb-command-result-count" className="wb-command-result-summary" role="status" aria-live="polite">
            {filtered.length > visible.length
              ? `Showing ${visible.length} of ${filtered.length} matches.`
              : `${filtered.length} ${filtered.length === 1 ? 'match' : 'matches'}.`}
          </p>
        </div>
      </section>
    </div>
  );
};

export default CommandPalette;
