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
      ? { color: tab.reportedWork, text: workLabels[tab.reportedWork], detail: 'Reported by Gajae-Code' }
      : { color: 'saved', text: 'Terminal running · no Agent status reported' };
    default: return { color: 'saved', text: 'Session status unavailable' };
  }
}

export function SessionRow({ session, tab, selected, onSelect }: {
  session?: Session; tab?: TabState; selected: boolean; onSelect: () => void;
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
  </div>;
}

