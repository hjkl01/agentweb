import { FormEvent, useEffect, useState } from 'react';
import { KeyRound, Monitor, ShieldCheck, X } from 'lucide-react';
import { api } from '../lib/api';

type AuthSession={id:string;created_at:string;expires_at:string;current:boolean};
type Props={open:boolean;onClose:()=>void;onLogout:()=>void};

export function UserPage({open,onClose,onLogout}:Props){
  const [username,setUsername]=useState(''); const [sessions,setSessions]=useState<AuthSession[]>([]);
  const [current,setCurrent]=useState(''); const [oldPassword,setOldPassword]=useState(''); const [newPassword,setNewPassword]=useState('');
  const [message,setMessage]=useState(''); const [error,setError]=useState(''); const [loading,setLoading]=useState(false);
  const load=async()=>{try{const me=await api<{username:string;session_id:string}>('/auth/me'); setUsername(me.username);setCurrent(me.session_id);setSessions(await api<AuthSession[]>('/auth/sessions'));}catch(e){setError(e instanceof Error?e.message:String(e));}};
  useEffect(()=>{if(open){setError('');setMessage('');load();}},[open]);
  if(!open)return null;
  const change=async(e:FormEvent)=>{e.preventDefault();setError('');setMessage('');setLoading(true);try{await api('/auth/password',{method:'PUT',body:JSON.stringify({current_password:oldPassword,new_password:newPassword})});setOldPassword('');setNewPassword('');setMessage('密码已修改，其他登录设备已退出。');await load();}catch(e){setError(e instanceof Error?e.message:String(e));}finally{setLoading(false);}};
  const revoke=async()=>{if(!confirm('确定退出其他所有设备吗？'))return;try{await api('/auth/sessions/revoke-all',{method:'POST'});await load();setMessage('其他设备已全部退出。');}catch(e){setError(e instanceof Error?e.message:String(e));}};
  return <div className="account-overlay"><section className="account-page"><header><div><span className="account-eyebrow"><ShieldCheck size={14}/> ACCOUNT</span><h1>账号与安全</h1><p>管理密码和当前登录设备</p></div><button className="icon-button" onClick={onClose}><X size={20}/></button></header>
    <div className="account-grid"><section className="account-card"><div className="account-card-title"><KeyRound size={18}/><div><strong>修改密码</strong><span>修改后其他设备会自动退出</span></div></div><form onSubmit={change} className="account-form"><label>用户名<input value={username} disabled/></label><label>当前密码<input type="password" autoComplete="current-password" value={oldPassword} onChange={e=>setOldPassword(e.target.value)}/></label><label>新密码<input type="password" autoComplete="new-password" value={newPassword} onChange={e=>setNewPassword(e.target.value)} minLength={8}/></label><button disabled={loading||!oldPassword||newPassword.length<8}>{loading?'保存中…':'更新密码'}</button></form></section>
      <section className="account-card sessions-card"><div className="account-card-title"><Monitor size={18}/><div><strong>登录设备</strong><span>{sessions.length} 个有效会话</span></div><button className="text-button" onClick={revoke} disabled={sessions.length<=1}>退出其他设备</button></div><div className="device-list">{sessions.map(s=><div className="device" key={s.id}><div className="device-icon"><Monitor size={17}/></div><div><strong>{s.current?'当前设备':'其他设备'}</strong><span>登录于 {new Date(s.created_at).toLocaleString()} · 到期 {new Date(s.expires_at).toLocaleDateString()}</span></div>{s.current&&<em>当前</em>}</div>)}</div></section></div>
    {message&&<div className="account-message">{message}</div>}{error&&<div className="account-error">{error}</div>}<footer><button className="danger-button" onClick={onLogout}>退出当前账号</button></footer>
  </section></div>;
}
