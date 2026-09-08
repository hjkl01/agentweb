import { Activity, Bot, Send, Square } from 'lucide-react';
import type { RefObject } from 'react';
import type { ActivityItem, Agent, AgentModel, ChatMessage, Session } from '../types';
import { AgentActivity } from './AgentActivity';
import { ModelSelector } from './ModelSelector';

type Props = {
  current?: Session; currentAgent?: Agent; active?: string; models: AgentModel[]; modelLoading: boolean;
  messages: ChatMessage[]; stream: string; activity: ActivityItem[]; activityOpen: boolean; input: string;
  textareaRef: RefObject<HTMLTextAreaElement | null>; chatRef: RefObject<HTMLElement | null>;
  onInputChange: (value: string) => void; onModelChange: (model?: string) => void; onSend: () => void;
  onToggleActivity: () => void; onStop: () => void; onNewChat: () => void; onAutoResize: () => void;
};

export function ChatPanel({ current, currentAgent, active, models, modelLoading, messages, stream, activity, activityOpen, input, textareaRef, chatRef, onInputChange, onModelChange, onSend, onToggleActivity, onStop, onNewChat, onAutoResize }: Props) {
  const running = current?.status === 'running';

  return (
    <main className="main-panel">
      <header className="topbar">
        <div className="chat-title"><strong>{current?.title || 'Agent Web'}</strong>{currentAgent && <span><span className="status-dot" /> {currentAgent.name}</span>}</div>
        {active && <div className="top-actions">
          <ModelSelector value={current?.model} models={models} loading={modelLoading} disabled={running} onChange={onModelChange} />
        </div>}
      </header>

      <section className="chat" ref={chatRef}>
        {!active && <div className="welcome"><div className="welcome-icon"><Bot size={30} /></div><h1>Build with your Agent</h1><p>选择一个 Agent 开始新的会话。Agent Web 负责会话、文件和实时交互，Agent 负责实际执行任务。</p><button className="new-chat" onClick={onNewChat}><span>＋</span> New chat</button></div>}
        {active && !messages.length && !stream && <div className="welcome compact"><div className="welcome-icon"><Activity size={24} /></div><h2>How can I help?</h2><p>向 {currentAgent?.name || current?.agent_id} 描述你想完成的任务。</p></div>}
        {messages.map(message => <div key={message.id} className={`message-row ${message.role}`}><div className="avatar">{message.role === 'user' ? 'U' : message.role === 'error' ? '!' : <Bot size={15} />}</div><div className="message-content"><div className="message-role">{message.role === 'user' ? 'You' : message.role === 'error' ? 'Error' : currentAgent?.name || 'Agent'}</div><pre>{message.content}</pre></div></div>)}
        {stream && <div className="message-row assistant"><div className="avatar"><Bot size={15} /></div><div className="message-content"><div className="message-role">{currentAgent?.name || 'Agent'} <span className="streaming">streaming</span></div><pre>{stream}<span className="cursor" /></pre></div></div>}
        {running && <div className="conversation-running"><div className="conversation-running-main"><span className="running-pulse" /><div><strong>Agent is working</strong><small>{currentAgent?.name || 'Agent'} 正在执行当前任务</small></div></div><button className="conversation-stop" onClick={onStop}><Square size={13} /> Stop</button></div>}
        <AgentActivity items={activity} open={activityOpen} onToggle={onToggleActivity} />
      </section>

      <div className="composer-wrap"><div className="composer"><textarea ref={textareaRef} value={input} onChange={event => { onInputChange(event.target.value); onAutoResize(); }} onKeyDown={event => { if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); onSend(); } }} placeholder={active ? 'Message your agent…' : 'Create a chat to start'} disabled={!active || running} rows={1} /><button className="send-button" onClick={onSend} disabled={!active || !input.trim() || running}>{running ? <Square size={16} /> : <Send size={16} />}</button></div><div className="composer-hint">Enter 发送 · Shift + Enter 换行</div></div>
    </main>
  );
}
