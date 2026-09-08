import { Copy, FileCode2 } from 'lucide-react';
import { useMemo, useState } from 'react';
import { CodeTokens } from './CodeTokens';
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
  const [copied, setCopied] = useState(false);
  const lines = useMemo(() => file.content.split('\n'), [file.content]);
  const lang = language(file.path);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(file.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    } catch { setCopied(false); }
  };

  return (
    <div className="code-preview">
      <div className="code-preview-head">
        <span className="code-preview-language"><FileCode2 size={13} />{lang}</span>
        <span>{lines.length.toLocaleString()} lines</span>
        <button className="code-copy" onClick={copy} title="Copy file"><Copy size={13} />{copied ? 'Copied' : 'Copy'}</button>
      </div>
      <div className="code-preview-scroll">
        <pre>{lines.map((line, index) => (
          <span className="code-line" key={index}>
            <span className="code-line-number">{index + 1}</span>
            <code><CodeTokens line={line || ' '} /></code>
          </span>
        ))}</pre>
      </div>
    </div>
  );
}
