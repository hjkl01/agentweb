import { FormEvent, useState } from 'react';
import { api } from '../lib/api';
import '../styles/login.css';

type Props = { onAuthenticated: () => void };

export function LoginPage({ onAuthenticated }: Props) {
  const [username, setUsername] = useState('admin');
  const [password, setPassword] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!username.trim() || !password) return;
    setLoading(true);
    setError('');
    try {
      await api('/auth/login', {
        method: 'POST',
        body: JSON.stringify({ username: username.trim(), password }),
      });
      onAuthenticated();
    } catch (err) {
      setError(err instanceof Error ? err.message : '登录失败，请检查用户名和密码。');
    } finally {
      setLoading(false);
    }
  };

  return (
    <main className="login-page">
      <div className="login-glow login-glow-one" />
      <div className="login-glow login-glow-two" />
      <section className="login-card">
        <div className="login-brand">
          <div className="login-mark">A</div>
          <div>
            <strong>Agent Web</strong>
            <span>AI coding workspace</span>
          </div>
        </div>
        <div className="login-heading">
          <h1>欢迎回来</h1>
          <p>登录你的 Agent Web 工作空间</p>
        </div>
        <form onSubmit={submit} className="login-form">
          <label>用户名<input autoFocus autoComplete="username" value={username} onChange={event => setUsername(event.target.value)} placeholder="用户名" /></label>
          <label>密码<input type="password" autoComplete="current-password" value={password} onChange={event => setPassword(event.target.value)} placeholder="输入密码" /></label>
          {error && <div className="login-error" role="alert">{error}</div>}
          <button className="login-submit" type="submit" disabled={loading || !username.trim() || !password}>
            {loading ? <><span className="login-spinner" />正在登录…</> : '登录'}
          </button>
        </form>
        <p className="login-footer">你的会话通过安全 Cookie 保持登录状态</p>
      </section>
      <footer className="login-page-footer">Agent Web · Multi-Agent Development Workspace</footer>
    </main>
  );
}
