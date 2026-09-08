import { ChevronDown, ChevronRight, File, Folder, FolderOpen, LoaderCircle, Search } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';

type Item = { name:string; path:string; kind:string; size:number };
type Props = { open:boolean; query:string; onSelect:(path:string)=>void; onClose:()=>void };

function TreeNode({ item, depth, onSelect }: { item:Item; depth:number; onSelect:(path:string)=>void }) {
 const [expanded,setExpanded]=useState(false); const [children,setChildren]=useState<Item[]>([]); const [loading,setLoading]=useState(false);
 const directory=item.kind==='directory';
 const toggle=async()=>{if(!directory){onSelect(item.path);return;} if(expanded){setExpanded(false);return;} setLoading(true);try{setChildren(await api<Item[]>(`/filesystem/tree?path=${encodeURIComponent(item.path)}`));setExpanded(true)}catch{}finally{setLoading(false)}};
 return <div className="mention-tree-node"><button className="mention-tree-row" style={{paddingLeft:12+depth*16}} onClick={toggle}>{directory?(expanded?<ChevronDown size={14}/>:<ChevronRight size={14}/>):<span className="mention-spacer"/>}{directory?(expanded?<FolderOpen size={15}/>:<Folder size={15}/>):<File size={14}/>}<span title={item.path}>{item.name}</span>{loading&&<LoaderCircle size={13} className="spin"/>}</button>{expanded&&children.map(child=><TreeNode key={child.path} item={child} depth={depth+1} onSelect={onSelect}/>)}</div>;
}

export function MentionPicker({open,query,onSelect,onClose}:Props){
 const [root,setRoot]=useState<Item[]>([]); const [loading,setLoading]=useState(false);
 useEffect(()=>{if(!open)return;setLoading(true);api<Item[]>('/filesystem/tree').then(setRoot).catch(()=>setRoot([])).finally(()=>setLoading(false))},[open]);
 if(!open)return null;
 const normalized=query.toLowerCase(); const visible=normalized?root.filter(item=>item.name.toLowerCase().includes(normalized)):root;
 return <div className="mention-picker" onMouseDown={event=>event.preventDefault()}>
  <div className="mention-picker-head"><div><strong>选择文件或文件夹</strong><small>从 HOME 开始</small></div><button onClick={onClose}>Esc</button></div>
  <div className="mention-search"><Search size={14}/><span>@{query||'输入名称搜索'}</span></div>
  <div className="mention-tree">{loading?<div className="mention-loading"><LoaderCircle size={16} className="spin"/>加载 HOME…</div>:visible.length?visible.map(item=><TreeNode key={item.path} item={item} depth={0} onSelect={onSelect}/>):<div className="mention-empty">没有匹配的文件或文件夹</div>}</div>
 </div>;
}
