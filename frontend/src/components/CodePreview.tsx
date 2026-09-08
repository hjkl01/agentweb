import { useMemo } from 'react';
import type { WorkspaceFile } from '../types';

type Props = { file: WorkspaceFile };

const languageNames: Record<string, string> = {
  rs: 'Rust', ts: 'TypeScript', tsx: 'TSX', js: 'JavaScript', jsx: 'JSX', json: 'JSON', py: 'Python', go: 'Go', java: 'Java', css: 'CSS', html: 'HTML', md: 'Markdown', yaml: 'YAML', yml: 'YAML', toml: 'TOML', sh: 'Shell', bash: 'Shell', sql: 'SQL',
};

function language(path: string) {
  const extension = path.split('.').pop()?.toLowerCase() || '';
  return languageNames[extension] || extension.toUpperCase() || 'Text';
}

export function CodePreview({ file }: Props) {
  const lines = useMemo(() => file.content.split('\n'), [file.content]);
  return (
    <div className="code-preview">
      <div className="code-preview-head"><span>{language(file.path)}</span><span>{lines.length.toLocaleString()} lines</span></div>
      <div className="code-preview-scroll">
        <pre>{lines.map((line, index) => <span className="code-line" key={index}><span className="code-line-number">{index + 1}</span><code>{line || ' '}</code></span>)}</pre>
      </div>
    </div>
  );
}
