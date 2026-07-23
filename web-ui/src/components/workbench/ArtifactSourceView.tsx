import React from 'react';
import Editor, { loader, type OnMount } from '@monaco-editor/react';
import * as monacoEditor from 'monaco-editor/editor/editor.api';
import EditorWorker from 'monaco-editor/editor/editor.worker?worker';
import { FileTextOutlined } from '@ant-design/icons';
import { useQuery } from '@tanstack/react-query';
import { deliveryProjectApi } from '../../services/api';
import { WorkbenchState } from './WorkbenchPrimitives';

loader.config({ monaco: monacoEditor });

(globalThis as typeof globalThis & {
  MonacoEnvironment?: { getWorker: () => Worker };
}).MonacoEnvironment ??= {
  getWorker: () => new EditorWorker(),
};

interface ArtifactLocation {
  artifactRef: string;
  line: number;
  column: number;
}

function parseArtifactLocation(resourceId?: string): ArtifactLocation | undefined {
  if (!resourceId) return undefined;
  const normalized = resourceId.replace(/\\/g, '/');
  const location = normalized.match(/^(.*?):(\d+)(?::(\d+))?$/);
  return {
    artifactRef: location?.[1] ?? normalized,
    line: Math.max(1, Number(location?.[2] ?? 1)),
    column: Math.max(1, Number(location?.[3] ?? 1)),
  };
}

function languageFor(path: string) {
  const extension = path.split('.').at(-1)?.toLowerCase();
  if (extension === 'json' || extension === 'jsonl') return 'json';
  if (extension === 'toml') return 'ini';
  if (extension === 'md') return 'markdown';
  if (extension === 'yaml' || extension === 'yml') return 'yaml';
  if (extension === 'rs') return 'rust';
  return 'plaintext';
}

const ArtifactSourceView: React.FC<{ resourceId?: string }> = ({ resourceId }) => {
  const location = parseArtifactLocation(resourceId);
  const artifactQuery = useQuery({
    queryKey: ['delivery-artifact', location?.artifactRef],
    queryFn: () => deliveryProjectApi.getArtifactText(location!.artifactRef),
    enabled: Boolean(location?.artifactRef),
    retry: false,
  });

  const handleMount: OnMount = (editor) => {
    if (!location) return;
    editor.setPosition({ lineNumber: location.line, column: location.column });
    editor.revealPositionInCenter({ lineNumber: location.line, column: location.column });
    editor.focus();
  };

  if (!location) return <WorkbenchState kind="empty" title="No artifact selected" detail="Choose a diagnostic, test, verification stage, or evidence record with an owning artifact." />;
  if (artifactQuery.isLoading) return <WorkbenchState kind="loading" title="Loading owning artifact" detail={location.artifactRef} />;
  if (artifactQuery.isError) return <WorkbenchState kind="error" title="Artifact unavailable" detail={`${location.artifactRef} is not readable through the delivery artifact API.`} onRetry={() => void artifactQuery.refetch()} />;

  return (
    <div className="wb-view wb-artifact-view" data-artifact-path={location.artifactRef} data-artifact-line={location.line}>
      <header className="wb-view-header wb-artifact-header">
        <div><h1><FileTextOutlined /> {location.artifactRef.split('/').at(-1)}</h1><p>{location.artifactRef}</p></div>
        <span className="wb-mono">Ln {location.line}, Col {location.column}</span>
      </header>
      <div className="wb-artifact-editor">
        <Editor
          path={location.artifactRef}
          value={artifactQuery.data ?? ''}
          language={languageFor(location.artifactRef)}
          onMount={handleMount}
          options={{
            readOnly: true,
            minimap: { enabled: false },
            automaticLayout: true,
            fontSize: 12,
            lineNumbersMinChars: 3,
            renderWhitespace: 'selection',
            scrollBeyondLastLine: false,
            wordWrap: 'off',
          }}
          theme="vs-dark"
        />
      </div>
    </div>
  );
};

export default ArtifactSourceView;
