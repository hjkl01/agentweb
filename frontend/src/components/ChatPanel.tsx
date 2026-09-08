import { Activity, Bot, ChevronDown, ChevronRight, Circle, Send, Square } from 'lucide-react';
import type { ActivityItem, Agent, ChatMessage, Session } from '../types';

type Props = {
  current?: Session;
  currentAgent?: Agent;
  active?: string;
  messages: ChatMessage[];
  stream: string;
  activity: ActivityItem[];
  activityOpen: boolean;
  input: string;
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  chatRef: React.RefObject<HTMLElement | null>;
  onInputChange: (value: string) => void;
  onSend: () => void;
  onToggleActivity: () => void;
  onStop: () => void;
  onNewChat: () => void;
  onAutoResize: () => void;
};

export function ChatPanel({ current, currentAgent, active, messages, stream, activity, activityOpen, input, textareaRef, chatRef, onInputChange, onSend, onToggleActivity, onStop, onNewChat, onAutoResize }: Props) {
  return (
    <main className="main-panel">
      <header className="topbar">
        <div className="chat-title">
          <strong>{current?.title || 'Agent Web'}</strong>
          {currentAgent && <span><span className="status-dot" /> {currentAgent.name}</span>}
        </div>
        {active && (
          <div className="top-actions">
            <span className={`run-state ${current?.status === 'running' ? 'running' : ''}`}>{current?.status || 'ready'}</span>
            {current?.status === 'running' && <button className="stop" onClick={onStop}><Square size={13} /> Stop</button>}
          </div>
        )}
      </header>

      <section className="chat" ref={chatRef}>
        {!active && (
          <div className="welcome">
            <div className="welcome-icon"><Bot size={30} /></div>
            <h1>Build with your Agent</h1>
            <p>选择一个 Agent 开始新的会话。Agent Web 负责会话、文件和实时交互，Agent 负责实际执行任务。</p>
            <button className="new-chat" onClick={onNewChat}><span>＋</span> New chat</button>
          </div>
        )}

        {active && !messages.length && !stream && (
          <div className="welcome compact">
            <div className="welcome-icon"><Activity size={24} /></div>
            <h2>How can I help?</h2>
            <p>向 {currentAgent?.name || current?.agent_id} 描述你想完成的任务。</p>
          </div>
        )}

        {messages.map(message => (
          <div key={message.id} className={`message-row ${message.role}`}>
            <div className="avatar">{message.role === 'user' ? 'U' : message.role === 'error' ? '!' : <Bot size={15} />}</div>
            <div className="message-content">
              <div className="message-role">{message.role === 'user' ? 'You' : message.role === 'error' ? 'Error' : currentAgent?.name || 'Agent'}</div>
              <pre>{message.content}</pre>
            </div>
          </div>
        ))}

        {stream && (
          <div className="message-row assistant">
            <div className="avatar"><Bot size={15} /></div>
            <div className="message-content">
              <div className="message-role">{currentAgent?.name || 'Agent'} <span className="streaming">streaming</span></div>
              <pre>{stream}<span className="cursor" /></pre>
            </div>
          </div>
        )}

        {activity.length > 0 && (
          <div className="activity-card">
            <button className="activity-header" onClick={onToggleActivity}>
              {activityOpen ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
              <Activity size={14} /><span>Agent activity</span><small>{activity.length} events</small>
            </button>
            {activityOpen && (
              <div className="activity-list">
                {activity.map(item => (
                  <div className="activity-item" key={item.id}>
                    <Circle size={7} />
                    <div><span>{item.label}</span>{item.detail && <pre>{item.detail}</pre>}</div>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </section>

      <div className="composer-wrap">
        <div className="composer">
          <textarea
            ref={textareaRef}
            value={input}
            onChange={event => { onInputChange(event.target.value); onAutoResize(); }}
            onKeyDown={event => {
              if (event.key === 'Enter' && !event.shiftKey) {
                event.preventDefault();
                onSend();
              }
            }}
            placeholder={active ? 'Message your agent…' : 'Create a chat to start'}
            disabled={!active || current?.status === 'running'}
            rows={1}
          />
          <button className="send-button" onClick={onSend} disabled={!active || !input.trim() || current?.status === 'running'}>
            {current?.status === 'running' ? <Square size={16} /> : <Send size={16} />}
          </button>
        </div>
        <div className="composer-hint">Enter 发送 · Shift + Enter 换行</div>
      </div>
    </main>
  );
}