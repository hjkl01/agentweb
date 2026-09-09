import { Bot, Check, ChevronDown } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import type { Agent } from '../types';

type Props={value?:string;agents:Agent[];disabled?:boolean;onChange:(id:string)=>void};
export function AgentSelector({value,agents,disabled,onChange}:Props){
 const [open,setOpen]=useState(false);const ref=useRef<HTMLDivElement>(null);const installed=agents.filter(a=>a.installed);const selected=agents.find(a=>a.id===value);
 useEffect(()=>{if(!open)return;const close=(e:MouseEvent)=>{if(!ref.current?.contains(e.target as Node))setOpen(false)};document.addEventListener('mousedown',close);return()=>document.removeEventListener('mousedown',close)},[open]);
 if(!selected)return null;
 return <div className="model-selector-wrap" ref={ref}>
  <button className={`model-selector ${open?'open':''}`} disabled={disabled} onClick={()=>setOpen(v=>!v)} aria-label="选择 Agent" aria-expanded={open}>
   <span className="model-selector-icon"><Bot size={14}/></span><span className="model-selector-info"><small>AGENT</small><strong>{selected.name}</strong></span><ChevronDown size={14} className="model-chevron"/>
  </button>
  {open&&<div className="model-menu">{installed.map(agent=><button key={agent.id} className={agent.id===value?'selected':''} onClick={()=>{onChange(agent.id);setOpen(false)}}><span><strong>{agent.name}</strong><small>{agent.description||'Ready to use'}</small></span>{agent.id===value&&<Check size={15}/>}</button>)}</div>}
 </div>;
}
