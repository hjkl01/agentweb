import { Terminal as TerminalIcon, X } from 'lucide-react';
import { useCallback, useEffect, useRef, useState } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';

type Props = { onClose: () => void };

function websocketUrl() {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}/api/terminal/ws`;
}

export function TerminalPanel({ onClose }: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | undefined>(undefined);
  const socketRef = useRef<WebSocket | undefined>(undefined);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string>();

  const sendResize = useCallback(() => {
    const terminal = terminalRef.current;
    const socket = socketRef.current;
    if (!terminal || !socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify({ type: 'resize', cols: terminal.cols, rows: terminal.rows }));
  }, []);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const terminal = new Terminal({ cursorBlink: true, cursorStyle: 'bar', fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace', fontSize: 13, lineHeight: 1.2, scrollback: 5000 });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
    fit.fit();
    terminal.focus();
    terminalRef.current = terminal;

    const socket = new WebSocket(websocketUrl());
    socket.binaryType = 'arraybuffer';
    socketRef.current = socket;
    socket.onopen = () => { setConnected(true); setError(undefined); sendResize(); terminal.focus(); };
    socket.onmessage = event => {
      if (typeof event.data === 'string') { terminal.write(event.data); return; }
      terminal.write(new Uint8Array(event.data as ArrayBuffer));
    };
    socket.onerror = () => { setConnected(false); setError('Terminal WebSocket 连接失败'); };
    socket.onclose = () => setConnected(false);
    const input = terminal.onData(data => { if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: 'input', data })); });
    const resize = () => { fit.fit(); sendResize(); };
    const observer = new ResizeObserver(resize);
    observer.observe(container);
    window.addEventListener('resize', resize);
    return () => { observer.disconnect(); window.removeEventListener('resize', resize); input.dispose(); socket.close(); terminal.dispose(); socketRef.current = undefined; terminalRef.current = undefined; };
  }, [sendResize]);

  return <main className="terminal-panel">
    <header className="terminal-header"><div className="terminal-title"><TerminalIcon size={16} /><strong>Terminal</strong><span className={connected ? 'terminal-status connected' : 'terminal-status'}>{connected ? 'Connected' : 'Disconnected'}</span></div><button className="icon-button" onClick={onClose} aria-label="关闭 Terminal"><X size={16} /></button></header>
    <div className="terminal-body" ref={containerRef} />
    {error && <div className="terminal-error">{error}</div>}
  </main>;
}
