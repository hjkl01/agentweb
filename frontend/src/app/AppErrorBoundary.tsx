import { Component, type ErrorInfo, type ReactNode } from 'react';

type Props = { children: ReactNode };
type State = { error?: Error };

export class AppErrorBoundary extends Component<Props, State> {
  state: State = {};

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Agent Web render error:', error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;

    return (
      <main className="fatal-error">
        <div className="fatal-error-card">
          <h1>Agent Web 加载失败</h1>
          <p>前端运行时发生错误，请打开浏览器控制台查看详细信息。</p>
          <pre>{this.state.error.message}</pre>
          <button onClick={() => window.location.reload()}>重新加载</button>
        </div>
      </main>
    );
  }
}
