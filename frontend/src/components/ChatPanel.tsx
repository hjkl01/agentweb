import { Activity, AtSign, Bot, Send, Square } from 'lucide-react';
import { useMemo, useState } from 'react';
import type { RefObject } from 'react';
import type { ActivityItem, Agent, AgentModel, ChatMessage, Session } from '../types';
import { AgentActivity } from './AgentActivity';
import { MentionPicker } from './MentionPicker';
import { ModelSelector } from './ModelSelector';
import { AgentSelector } from './AgentSelector';

type Props = { current?: Session; currentAgent?: Agent; agents: Agent[]; active?: string; models: AgentModel[]; modelLoading: boolean; messages: ChatMessage[]; stream: string; activity: ActivityItem[]; activityOpen: boolean; input: string; textareaRef: RefObject<HTMLTextAreaElement | null>; chatRef: RefObject<HTMLElement | null>; onInputChange: (value: string) => void; onAgentChange: (id: string) => void; onModelChange: (model?: string) => void; onSend: () => void; onToggleActivity: () => void; onStop: () => void; onNewChat: () => void; onAutoResize: () => void; };
export function ChatPanel({ current, currentAgent, agents, active, models, modelLoading, messages, stream, activity, activityOpen, input, textareaRef, chatRef, onInputChange, onAgentChange, onModelChange, onSend, onToggleActivity, onStop, onNewChat, onAutoResize }: Props) {
  const running = current?.status === 'running'; const [mentionOpen, setMentionOpen] = useState(false);
  const mentionQuery = useMemo(() => { const match = input.match(/(?:^|\s)@(?:\{([^}]*)\}|([^\s]*))$/); return match ? (match[1] ?? match[2] ?? '') : ''; }, [input]);
  const shouldMention = !!active && !running && /(?:^|\s)@(?:\{[^}]*|[^\s]*)$/.test(input);
  const chooseMention = (path: string) => { const match = input.match(/(?:^|\s)@(?:\{[^}]*)?[^\s]*$/); if (!match) return; const prefix = input.slice(0, match.index! + (match[0].startsWith(' ') ? 1 : 0)); const mention = path.includes(' ') ? `@{${path}}` : `@${path}`; onInputChange(`${prefix}${mention} `); setMentionOpen(false); requestAnimationFrame(() => textareaRef.current?.focus()); };
  return <main className="main-panel">
    <header className="topbar"><div className="chat-title"><strong>{current?.title || 'Agent Web'}</strong>{currentAgent && <span><span className="status-dot" /> {currentAgent.name}</span>}</div>{active && <div className="top-actions"><AgentSelector value={current?.agent_id} agents={agents} disabled={running} onChange={onAgentChange}/><ModelSelector value={current?.model} models={models} loading={modelLoading} disabled={running} onChange={onModelChange} /></div>}</header>
    <section className="chat" ref={chatRef}>
      {!active && <div className="welcome"><div className="welcome-icon"><Bot size={30} /></div><h1>Build with your Agent</h1><p>新建对话会自动使用默认 Agent 和模型。也可以在对话顶部临时切换。</p><button className="new-chat" onClick={onNewChat}><span>＋</span> New chat</button></div>}
      {active && !messages.length && !stream && <div className="welcome compact"><div className="welcome-icon"><Activity size={24} /></div><h2>How can I help?</h2><p>向 {currentAgent?.name || current?.agent_id} 描述你想完成的任务。</p></div>}
      {messages.map(message => <div key={message.id} className={`message-row ${message.role}`}><div className="avatar">{message.role === 'user' ? 'U' : message.role === 'error' ? '!' : <Bot size={15} />}</div><div className="message-content"><div className="message-role">{message.role === 'user' ? 'You' : message.role === 'error' ? 'Error' : currentAgent?.name || 'Agent'}</div><pre>{message.content}</pre></div></div>)}
      {stream && <div className="message-row assistant"><div className="avatar"><Bot size={15} /></div><div className="message-content"><div className="message-role">{currentAgent?.name || 'Agent'} <span className="streaming">streaming</span></div><pre>{stream}<span className="cursor" /></pre></div></div>}
      <AgentActivity items={activity} open={activityOpen} onToggle={onToggleActivity} />
    </section>
    <div className="composer-wrap"><div className="composer-container">{mentionOpen && <MentionPicker open query={mentionQuery} sessionId={active!} onSelect={chooseMention} onClose={() => setMentionOpen(false)} />}<div className="composer"><textarea ref={textareaRef} value={input} onChange={event => { const value = event.target.value; onInputChange(value); setMentionOpen(/(?:^|\s)@(?:\{[^}]*|[^\s]*)$/.test(value)); onAutoResize(); }} onKeyDown={event => { if (event.key === 'Escape' && mentionOpen) { event.preventDefault(); setMentionOpen(false); return; } if (event.key === 'Enter' && !event.shiftKey && !running && !shouldMention) { event.preventDefault(); onSend(); } }} placeholder={active ? '输入消息，使用 @ 选择文件或文件夹…' : 'Create a chat to start'} disabled={!active || running} rows={1} /><button className="send-button" onClick={running ? onStop : onSend} disabled={!active || (!running && !input.trim())} aria-label={running ? 'Stop' : 'Send'} title={running ? 'Stop agent' : 'Send message'}>{running ? <Square size={14} strokeWidth={2.2} /> : <Send size={16} />}</button></div></div><div className="composer-hint"><AtSign size={12} /> @ 选择文件/文件夹 · Enter 发送 · Shift + Enter 换行</div></div>
  </main>;
}
