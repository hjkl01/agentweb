import { Activity, Brain, Check, Circle, FileText, LoaderCircle, Terminal, Wrench } from 'lucide-react';
import type { ActivityItem } from '../types';

type Props = { items: ActivityItem[]; open: boolean; onToggle: () => void };

function icon(type: string) {
  if (type.startsWith('thinking.')) return <Brain size={13} />;
  if (type.startsWith('tool.')) return <Wrench size={13} />;
  if (type.startsWith('command.')) return <Terminal size={13} />;
  if (type.startsWith('file.')) return <FileText size={13} />;
  return <Circle size={8} />;
}

function state(type: string) {
  if (type.endsWith('.completed') || type.startsWith('file.')) return <Check size={12} />;
  if (type.endsWith('.started')) return <LoaderCircle className="activity-spin" size={12} />;
  return null;
}

function ActivityRow({ item }: { item: ActivityItem }) {
  return (
    <div className="activity-item" key={item.id}>
      <span className="activity-icon">{icon(item.type)}</span>
      <div className="activity-body">
        <div className="activity-label"><span>{item.label}</span><span>{state(item.type)}</span></div>
        {item.detail && <pre>{item.detail}</pre>}
      </div>
    </div>
  );
}

export function AgentActivity({ items, open, onToggle }: Props) {
  if (!items.length) return null;
  const thinking = items.find(item => item.key === 'thinking');

  return (
    <div className="activity-card">
      <button className="activity-header" onClick={onToggle}>
        {open ? <Activity size={14} /> : <Circle size={8} />}
        <span>Agent activity</span>
        <small>{items.length} steps</small>
      </button>
      {!open && thinking && (
        <div className="activity-summary">
          <span className="activity-icon"><Brain size={13} /></span>
          <span>Thinking</span>
          <span className="activity-summary-state">{state(thinking.type) || <LoaderCircle className="activity-spin" size={12} />}</span>
        </div>
      )}
      {open && <div className="activity-list">{items.map(item => <ActivityRow key={item.id} item={item} />)}</div>}
    </div>
  );
}
