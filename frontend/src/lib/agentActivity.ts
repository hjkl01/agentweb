import type { ActivityItem, AgentEvent } from '../types';

const MAX_ITEMS = 40;

export function applyAgentEvent(items: ActivityItem[], event: AgentEvent): ActivityItem[] {
  const data = event.data || {};
  const id = String(data.activity_id || data.item_id || data.call_id || data.command_id || data.tool_call_id || event.type);
  const detail = detailFor(event, data);
  const label = labelFor(event, data);

  if (event.type.endsWith('.delta') || event.type === 'tool.output' || event.type === 'command.output') {
    const index = items.findIndex(item => item.key === id && item.type === baseType(event.type));
    if (index >= 0) {
      const next = [...items];
      next[index] = { ...next[index], detail: `${next[index].detail || ''}${detail || ''}` };
      return next;
    }
  }

  const existing = items.findIndex(item => item.key === id && item.type === baseType(event.type));
  if (existing >= 0) {
    const next = [...items];
    next[existing] = { ...next[existing], type: event.type, label, detail: detail || next[existing].detail };
    return next;
  }

  return [...items, { id: crypto.randomUUID(), key: id, type: event.type, label, detail }].slice(-MAX_ITEMS);
}

function baseType(type: string) {
  return type.replace(/\.(started|delta|output|completed)$/, '');
}

function labelFor(event: AgentEvent, data: Record<string, any>) {
  if (event.type.startsWith('thinking.')) return 'Thinking';
  if (event.type.startsWith('tool.')) return `Tool: ${data.tool || 'unknown'}`;
  if (event.type.startsWith('command.')) return `Command: ${data.command || 'running'}`;
  if (event.type === 'file.created') return 'File created';
  if (event.type === 'file.modified') return 'File modified';
  if (event.type === 'file.deleted') return 'File deleted';
  if (event.type === 'message.started') return 'Agent started';
  if (event.type === 'agent.error' || event.type === 'error') return 'Agent error';
  return event.type;
}

function detailFor(event: AgentEvent, data: Record<string, any>) {
  if (event.type.startsWith('thinking.') || event.type === 'message.delta') return data.text;
  if (event.type === 'tool.output') return data.output;
  if (event.type === 'command.output') return data.output;
  if (event.type.startsWith('file.')) return data.path;
  if (event.type === 'agent.error' || event.type === 'error') return data.message;
  return undefined;
}
