import { useEffect, useRef, useState } from 'react';
import { agents, type Session, type WorkStatus } from './types';
import type { TabState } from './TerminalPane';

const workLabels: Record<WorkStatus, string> = {
  in_progress: 'Work in progress',
  completed: 'Agent run finished · check result',
  waiting_for_reply: 'Your response needed',
};

export function sessionIndicator(tab?: TabState): { color: string; text: string; detail?: string } {
  if (!tab) return { color: 'saved', text: 'Saved session · no process running' };
  if (tab.error || tab.status === 'failed') return { color: 'failed', text: 'Session error' };
  switch (tab.status) {
    case 'starting': return { color: 'starting', text: 'New session · starting terminal' };
    case 'stopping': return { color: 'stopping', text: 'Stopping terminal' };
    case 'stopped': return { color: 'saved', text: 'Terminal stopped' };
    case 'exited': return tab.code
      ? { color: 'failed', text: `Terminal exited with error · code ${tab.code}` }
      : { color: 'saved', text: 'Terminal exited normally' };
    case 'running': return tab.reportedWork
      ? { color: tab.reportedWork, text: workLabels[tab.reportedWork], detail: tab.session?.version === '0.10.2'
        ? 'Detected from the GJC 0.10.2 terminal' : 'Reported by Gajae-Code' }
      : { color: 'saved', text: 'Terminal running · no Agent status reported' };
    default: return { color: 'saved', text: 'Session status unavailable' };
  }
}

export function SessionRow({ session, tab, selected, onSelect, pinned, onPin }: {
  session?: Session; tab?: TabState; selected: boolean; onSelect: () => void;
  pinned?: boolean; onPin?: () => void;
}) {
  const status = sessionIndicator(tab);
  const name = session?.name ?? tab?.launch.name;
  return <div className={`session-row ${selected ? 'selected' : ''}`}>
    <button className="session-item" onClick={onSelect} aria-current={selected ? 'true' : undefined}>
      <strong>{name}</strong>
      {session && <><span>{agents[session.agent]} · {session.version}</span>
        <small className="ellipsis" title={session.cwd}>{session.cwd}</small></>}
      <small className="session-status"><i className={`dot ${status.color}`} aria-hidden="true" />{status.text}</small>
      {status.detail && <small>{status.detail}</small>}
    </button>
    {onPin && <button type="button" className="session-pin" aria-label={`${pinned ? 'Unpin' : 'Pin'} ${name}`}
      title={pinned ? 'Unpin session' : 'Pin session'} aria-pressed={pinned} onClick={onPin}>{pinned ? '★' : '☆'}</button>}
  </div>;
}

export interface SwitchItem {
  id: string;
  name: string;
  cwd: string;
  agent: string;
  status: string;
  choose: () => void;
}

export function SessionSwitcher({ items }: { items: SwitchItem[] }) {
  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const frame = requestAnimationFrame(() => input.current?.focus());
    return () => cancelAnimationFrame(frame);
  }, []);
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  const results = items.filter(item => {
    const searchable = `${item.name} ${item.cwd} ${item.agent} ${item.status}`.toLowerCase();
    return words.every(word => searchable.includes(word));
  }).slice(0, 80);
  return <div className="stack">
    <input ref={input} aria-label="Search sessions" placeholder="Search sessions, paths, agents, or status" value={query}
      onChange={event => { setQuery(event.target.value); setCursor(0); }}
      onKeyDown={event => {
        if (event.key === 'Enter') { event.preventDefault(); results[Math.min(cursor, results.length - 1)]?.choose(); }
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
          event.preventDefault();
          const next = Math.max(0, Math.min(results.length - 1, cursor + (event.key === 'ArrowDown' ? 1 : -1)));
          setCursor(next);
          event.currentTarget.parentElement?.querySelectorAll('.switch-item')[next]?.scrollIntoView({ block: 'nearest' });
        }
      }} />
    {!results.length && <p className="muted">No matching sessions.</p>}
    <div className="switch-results">
      {results.map((item, index) => <button type="button" className={`switch-item${index === cursor ? ' selected' : ''}`}
        key={item.id} onClick={item.choose}>
        <strong>{item.name}</strong><small>{item.agent} · {item.status}</small>
        <small className="ellipsis" title={item.cwd}>{item.cwd}</small>
      </button>)}
    </div>
  </div>;
}

export function projectGroups(sessions: Session[]) {
  const groups = new Map<string, { path: string; sessions: Session[] }>();
  for (const session of sessions) {
    const key = session.cwd.replaceAll('/', '\\').replace(/\\+$/, '').toLowerCase();
    let group = groups.get(key);
    if (!group) {
      group = { path: session.cwd, sessions: [] };
      groups.set(key, group);
    }
    group.sessions.push(session);
  }
  return [...groups.values()];
}

