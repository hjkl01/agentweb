import { useEffect, useState } from 'react';
import { App } from '../App';
import { LoginPage } from '../components/LoginPage';
import { useAppRoute } from './router';

type AuthState='checking'|'authenticated'|'anonymous';

export function AppRouter(){
 const {sessionId,navigate}=useAppRoute(); const [auth,setAuth]=useState<AuthState>('checking');
 const checkAuth=async()=>{try{const response=await fetch('/api/auth/me',{credentials:'include'});setAuth(response.ok?'authenticated':'anonymous');}catch{setAuth('anonymous');}};
 useEffect(()=>{checkAuth();const onExpired=()=>setAuth('anonymous');window.addEventListener('agentweb:auth-expired',onExpired);return()=>window.removeEventListener('agentweb:auth-expired',onExpired);},[]);
 useEffect(()=>{if(auth==='authenticated'&&window.location.pathname!=='/'&&!sessionId)navigate('/',true);},[auth,navigate,sessionId]);
 if(auth==='checking')return <div className="auth-loading"><div className="auth-loading-mark">A</div><span>正在加载 Agent Web…</span></div>;
 if(auth==='anonymous')return <LoginPage onAuthenticated={()=>{setAuth('authenticated');navigate('/',true)}}/>;
 return <App sessionId={sessionId} navigate={navigate}/>;
}
