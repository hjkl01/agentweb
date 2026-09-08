import { Bot, Edit3, LogOut, MessageCircle, MoreHorizontal, Pin, PinOff, Plus, Settings, Trash2, UserRound } from 'lucide-react';
import type { Agent, Session } from '../types';

type Props={agents:Agent[];sessions:Session[];active?:string;sessionMenu?:string;onNewChat:()=>void;onSelectSession:(id:string)=>void;onSessionMenu:(id:string)=>void;onRenameSession:(id:string)=>void;onTogglePin:(id:string)=>void;onDeleteSession:(id:string)=>void;onOpenCatalog:()=>void;onOpenAccount:()=>void;onLogout:()=>void;onHome:()=>void};
export function Sidebar({agents,sessions,active,sessionMenu,onNewChat,onSelectSession,onSessionMenu,onRenameSession,onTogglePin,onDeleteSession,onOpenCatalog,onOpenAccount,onLogout,onHome}:Props){return <aside className="sidebar">
  <button className="brand" onClick={onHome} aria-label="返回首页"><div className="brand-mark"><Bot size={18}/></div><div><strong>Agent Web</strong><span>Agent workspace</span></div></button>
  <button className="new-chat" onClick={onNewChat}><Plus size={16}/> New chat</button><div className="section-label">Chats <span>{sessions.length}</span></div>
  <div className="session-list">{sessions.map(session=><div key={session.id} className={`session ${active===session.id?'active':''} ${session.is_pinned?'pinned':''}`} onClick={()=>onSelectSession(session.id)}>
    {session.is_pinned?<Pin size={15}/>:<MessageCircle size={15}/>}<div className="session-main"><span>{session.title}</span><small>{agents.find(a=>a.id===session.agent_id)?.name||session.agent_id}</small></div>
    <button className="icon-button session-menu" onClick={e=>{e.stopPropagation();onSessionMenu(session.id)}}><MoreHorizontal size={15}/></button>
    {sessionMenu===session.id&&<div className="menu" onClick={e=>e.stopPropagation()}><button onClick={()=>onTogglePin(session.id)}>{session.is_pinned?<PinOff size={14}/>:<Pin size={14}/>} {session.is_pinned?'取消置顶':'置顶'}</button><button onClick={()=>onRenameSession(session.id)}><Edit3 size={14}/> Rename</button><button onClick={()=>onDeleteSession(session.id)}><Trash2 size={14}/> Delete</button></div>}
  </div>)}{!sessions.length&&<div className="empty-sidebar">No chats yet</div>}</div>
  <div className="sidebar-bottom"><button className="settings-button" onClick={onOpenCatalog}><Settings size={16}/> Agents & runtimes</button><button className="settings-button" onClick={onOpenAccount}><UserRound size={16}/> Account & sessions</button><button className="settings-button logout-button" onClick={onLogout}><LogOut size={16}/> Sign out</button></div>
</aside>}
