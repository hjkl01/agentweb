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
  const terminalRef = useRef<Terminal | null>(null);
  const socketRef = useRef<WebSocket | null>(null);
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
    const terminal = new Terminal({ cursorBlink: true, cursorStyle: 'bar', fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace', fontSize: 13, lineHeight: 1.2, scrollback: 5000, convertEol: false, allowTransparency: false });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(container);
    terminalRef.current = terminal;

    const fitAndResize = () => {
      if (!container.isConnected) return;
      fit.fit();
      sendResize();
    };
    requestAnimationFrame(fitAndResize);

    const socket = new WebSocket(websocketUrl());
    socket.binaryType = 'arraybuffer';
    socketRef.current = socket;
    socket.onopen = () => { setConnected(true); setError(undefined); fitAndResize(); terminal.focus(); };
    socket.onmessage = event => {
      if (typeof event.data === 'string') terminal.write(event.data);
      else terminal.write(new Uint8Array(event.data as ArrayBuffer));
    };
    socket.onerror = () => { setConnected(false); setError('Terminal WebSocket 连接失败'); };
    socket.onclose = () => { setConnected(false); setError(current => current || 'Terminal 连接已断开'); };

    const input = terminal.onData(data => {
      if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: 'input', data }));
    });
    const observer = new ResizeObserver(() => requestAnimationFrame(fitAndResize));
    observer.observe(container);
    window.addEventListener('resize', fitAndResize);

    return () => {
      observer.disconnect();
      window.removeEventListener('resize', fitAndResize);
      input.dispose();
      socket.close();
      terminal.dispose();
      socketRef.current = null;
      terminalRef.current = null;
    };
  }, [sendResize]);

  return <main className="terminal-panel">
    <header className="terminal-header">
      <div className="terminal-title"><TerminalIcon size={16} /><strong>Terminal</strong><span className={connected ? 'terminal-status connected' : 'terminal-status'}>{connected ? 'Connected' : 'Disconnected'}</span></div>
      <button className="icon-button" onClick={onClose} aria-label="关闭 Terminal" title="关闭 Terminal"><X size={16} /></button>
    </header>
    <div className="terminal-body" ref={containerRef} />
    {error && <div className="terminal-error" role="alert">{error}</div>}
  </main>;
}
