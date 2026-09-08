import { Check, ChevronDown, Cpu, LoaderCircle } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { AgentModel } from '../types';

type Props = {
  value?: string;
  models: AgentModel[];
  loading: boolean;
  disabled?: boolean;
  onChange: (model?: string) => void;
};

export function ModelSelector({ value, models, loading, disabled, onChange }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const selected = models.find(model => model.id === value);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!ref.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  if (loading) return <div className="model-selector loading"><LoaderCircle className="spin" size={14} /><span>加载模型…</span></div>;
  if (!models.length) return <div className="model-selector empty"><Cpu size={14} /><span>Agent 默认模型</span></div>;

  const choose = (model?: string) => {
    onChange(model);
    setOpen(false);
  };

  return (
    <div className="model-selector-wrap" ref={ref}>
      <button className={`model-selector ${open ? 'open' : ''}`} disabled={disabled} onClick={() => setOpen(current => !current)} aria-label="选择模型" aria-expanded={open}>
        <span className="model-selector-icon"><Cpu size={14} /></span>
        <span className="model-selector-info">
          <small>MODEL</small>
          <strong>{selected?.name || '默认模型'}</strong>
          {selected?.provider && <em>{selected.provider}</em>}
        </span>
        <ChevronDown size={14} className="model-chevron" />
      </button>
      {open && <div className="model-menu">
        <button className={!value ? 'selected' : ''} onClick={() => choose(undefined)}>
          <span><strong>默认模型</strong><small>由 Agent 自己选择</small></span>
          {!value && <Check size={15} />}
        </button>
        {models.map(model => (
          <button key={model.id} className={model.id === value ? 'selected' : ''} onClick={() => choose(model.id)}>
            <span><strong>{model.name}</strong><small>{model.provider || model.source || model.id}</small></span>
            {model.id === value && <Check size={15} />}
          </button>
        ))}
      </div>}
    </div>
  );
}
