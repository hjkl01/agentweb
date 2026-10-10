import type { ActivityItem, AgentEvent } from '../types';

const MAX_ITEMS = 40;
const IDENTIFIERS = ['activity_id', 'item_id', 'call_id', 'command_id', 'tool_call_id'];

export function applyAgentEvent(items: ActivityItem[], event: AgentEvent): ActivityItem[] {
  const data = event.data || {};
  const detail = detailFor(event, data);
  const label = labelFor(event, data);
  const id = activityKey(items, event, data);
  const existing = items.findIndex(item => item.key === id && sameActivity(item.type, event.type));

  if (existing >= 0) {
    const next = [...items];
    const previous = next[existing];
    next[existing] = {
      ...previous,
      type: event.type,
      label: label || previous.label,
      detail: appendDetail(event.type) ? `${previous.detail || ''}${detail || ''}` : detail || previous.detail,
    };
    return next;
  }

  return [...items, { id: crypto.randomUUID(), key: id, type: event.type, label, detail }].slice(-MAX_ITEMS);
}

function activityKey(items: ActivityItem[], event: AgentEvent, data: Record<string, any>) {
  if (event.type.startsWith('thinking.')) return 'thinking';
  if (event.type === 'session.completed') return 'session:completed';
  if (event.type === 'agent.error' || event.type === 'error') return 'session:error';
  const explicit = IDENTIFIERS.map(name => data[name]).find(Boolean);
  if (explicit) return String(explicit);

  const base = baseType(event.type);
  if (isContinuation(event.type)) {
    const active = [...items].reverse().find(item => baseType(item.type) === base && !isTerminal(item.type));
    if (active) return active.key;
  }
  return `${base}:${crypto.randomUUID()}`;
}

function sameActivity(left: string, right: string) {
  return baseType(left) === baseType(right);
}

function isContinuation(type: string) {
  return type.endsWith('.delta') || type.endsWith('.output') || type.endsWith('.completed');
}

function isTerminal(type: string) {
  return type.endsWith('.completed') || type.endsWith('.failed') || type.endsWith('.error');
}

function appendDetail(type: string) {
  return type.endsWith('.delta') || type.endsWith('.output');
}

function baseType(type: string) {
  return type.replace(/\.(started|delta|output|completed|failed|error)$/, '');
}

function labelFor(event: AgentEvent, data: Record<string, any>) {
  if (event.type.startsWith('thinking.')) return '分析过程';
  if (event.type.startsWith('tool.')) return `工具调用：${data.tool || data.name || '工具'}`;
  if (event.type.startsWith('command.')) return `执行命令：${data.command || data.name || '命令'}`;
  if (event.type === 'file.created') return '创建文件';
  if (event.type === 'file.modified') return '修改文件';
  if (event.type === 'file.deleted') return '删除文件';
  if (event.type === 'message.started') return '开始处理请求';
  if (event.type === 'message.completed') return '生成回复';
  if (event.type === 'session.completed') return '任务完成';
  if (event.type === 'agent.error' || event.type === 'error') return '执行失败';
  return event.type;
}

function detailFor(event: AgentEvent, data: Record<string, any>) {
  if (event.type.startsWith('thinking.') || event.type === 'message.delta') return data.text;
  if (event.type.startsWith('tool.') && (event.type.endsWith('.output') || event.type.endsWith('.delta'))) return data.output || data.text;
  if (event.type.startsWith('command.') && (event.type.endsWith('.output') || event.type.endsWith('.delta'))) return data.output || data.text;
  if (event.type.startsWith('file.')) return data.path;
  if (event.type === 'agent.error' || event.type === 'error') return data.message;
  return undefined;
}
