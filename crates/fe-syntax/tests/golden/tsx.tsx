// A status bar component, used as a golden highlighting sample.
import { useEffect, useState, type ReactNode } from 'react';

export type Mode = 'insert' | 'normal' | 'visual';

interface StatusBarProps {
  path: string | null;
  mode: Mode;
  dirty?: boolean;
  children?: ReactNode;
}

const MODE_LABELS: Record<Mode, string> = {
  insert: 'INSERT',
  normal: 'NORMAL',
  visual: 'VISUAL',
};

function useClock(intervalMs = 1000): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), intervalMs);
    return () => clearInterval(timer);
  }, [intervalMs]);
  return now;
}

export function StatusBar({ path, mode, dirty = false, children }: StatusBarProps) {
  const now = useClock();
  const name = path?.split('/').pop() ?? 'untitled';
  const time = now.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  return (
    <footer className="status-bar" data-mode={mode}>
      <span className={`mode mode-${mode}`}>{MODE_LABELS[mode]}</span>
      <span className="file" title={path ?? undefined}>
        {name}
        {dirty && <em aria-label="unsaved"> ●</em>}
      </span>
      {children}
      <time dateTime={now.toISOString()}>{time}</time>
    </footer>
  );
}

export function Spacer(): JSX.Element {
  return <div style={{ flex: 1 }} />;
}

export default function Demo() {
  const [mode, setMode] = useState<Mode>('normal');
  const toggle = () => setMode((m) => (m === 'normal' ? 'insert' : 'normal'));
  return (
    <StatusBar path="/tmp/notes.md" mode={mode} dirty>
      <Spacer />
      <button type="button" onClick={toggle} disabled={false}>
        Toggle
      </button>
    </StatusBar>
  );
}
