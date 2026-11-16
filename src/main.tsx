import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { ask } from '@tauri-apps/plugin-dialog';
import { api, readable } from './api';
import { NewSession } from './NewSession';
import { KeyboardShortcuts } from './KeyboardShortcuts';
import { Settings } from './Settings';
import { projectGroups, SessionRow, sessionIndicator, SessionSwitcher } from './SessionNavigator';
import { TerminalPane, type TabState } from './TerminalPane';
import { agents, type Launch, type Overview, type Session } from './types';
import './style.css';

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { ref.current?.showModal(); }, []);
  return <dialog ref={ref} aria-labelledby="modal-title" onCancel={event => { event.preventDefault(); onClose(); }}>
    <div className="modal-head"><h2 id="modal-title">{title}</h2><button onClick={onClose} aria-label="Close dialog">×</button></div>
    <div className="modal-body">{children}</div>
  </dialog>;
}

const live = (tab: TabState) => ['starting', 'running', 'stopping'].includes(tab.status);
const historyId = (tab: TabState) => tab.launch.resume_from ?? tab.launch.id;
const tabForHistory = (tabs: TabState[], id: string, active: string) =>
  tabs.find(tab => tab.launch.id === active && historyId(tab) === id)
  ?? [...tabs].reverse().find(tab => historyId(tab) === id);

function App() {
  const [data, setData] = useState<Overview>();
  const [tabs, setTabs] = useState<TabState[]>([]);
  const [sessionsOpen, setSessionsOpen] = useState(true);
  const [active, setActive] = useState('');
  const [comparison, setComparison] = useState<{ left: string; right: string }>();
  const [focusTerminal, setFocusTerminal] = useState(true);
  const [selected, setSelected] = useState<Session>();
  const [modal, setModal] = useState<'settings' | 'new' | 'shortcuts' | 'switch' | 'compare'>();
  const [previous, setPrevious] = useState<Session>();
  const [resume, setResume] = useState(false);
  const [error, setError] = useState('');
  const [checking, setChecking] = useState<string>();
  const askingToQuit = useRef(false);
  const helpButton = useRef<HTMLButtonElement>(null);

  const refresh = useCallback(async () => setData(await api.overview()), []);
  useEffect(() => { void refresh().catch(reason => setError(readable(reason))); }, [refresh]);
  const update = useCallback((id: string, patch: Partial<TabState>) => {
    setTabs(current => current.map(tab => tab.launch.id === id ? { ...tab, ...patch } : tab));
  }, []);
  const onStart = useCallback(() => { void refresh().catch(reason => setError(readable(reason))); }, [refresh]);
  const activateTab = useCallback((id: string, focus = true) => {
    if (!id) return;
    setComparison(current => {
      if (!current || current.left === id || current.right === id) return current;
      return active === current.right ? { ...current, right: id } : { ...current, left: id };
    });
    setSelected(undefined);
    setFocusTerminal(focus);
    setActive(id);
  }, [active]);

  const newSession = useCallback(() => { setPrevious(undefined); setResume(false); setModal('new'); }, []);
  const closeTab = useCallback(async (id: string) => {
    const tab = tabs.find(item => item.launch.id === id);
    if (!tab || ['starting', 'stopping'].includes(tab.status)) return;
    try {
      if (live(tab) && !await ask('Stop the Agent process and close this tab?', { title: 'Close terminal', kind: 'warning' })) return;
      await api.close(id);
      setTabs(current => current.filter(item => item.launch.id !== id));
      const otherPane = comparison?.left === id ? comparison.right : comparison?.right === id ? comparison.left : undefined;
      if (otherPane) setComparison(undefined);
      setActive(current => current === id ? otherPane ?? tabs.find(item => item.launch.id !== id)?.launch.id ?? '' : current);
      setSelected(undefined);
    } catch (reason) { setError(readable(reason)); }
  }, [tabs, comparison]);
  const quit = useRef<() => Promise<void>>(async () => {});
  quit.current = async () => {
    if (askingToQuit.current) return;
    askingToQuit.current = true;
    try {
      const count = tabs.filter(live).length;
      if (!count || await ask(`Close Agent Deck and stop ${count} active terminal session(s)?`, { title: 'Exit Agent Deck', kind: 'warning' })) await api.quit();
    } catch (reason) { setError(readable(reason)); }
    finally { askingToQuit.current = false; }
  };
  useEffect(() => {
    let disposed = false;
    let release: (() => void) | undefined;
    void listen('request-quit', () => { void quit.current(); }).then(unlisten => {
      if (disposed) unlisten();
      else release = unlisten;
    }).catch(reason => setError(readable(reason)));
    return () => { disposed = true; release?.(); };
  }, []);
  useEffect(() => {
    const keys = (event: KeyboardEvent) => {
      if (!data || modal || event.defaultPrevented || event.isComposing || event.repeat ||
          !event.ctrlKey || event.altKey || event.metaKey ||
          !(event.target instanceof Element) || !event.target.closest('[data-deck-shortcuts]') ||
          event.target.closest('input, textarea, select, [contenteditable="true"]')) return;
      const key = event.key.toLowerCase();
      if (!(key === 'tab' || (!event.shiftKey && ['t', 'w', 'k'].includes(key)))) return;
      event.preventDefault();
      event.stopPropagation();
      if (key === 't') newSession();
      if (key === 'k') setModal('switch');
      if (key === 'w' && active) {
        setFocusTerminal(false);
        helpButton.current?.focus();
        void closeTab(active);
      }
      if (key === 'tab' && tabs.length) {
        const index = tabs.findIndex(tab => tab.launch.id === active);
        const next = index < 0 ? (event.shiftKey ? tabs.length - 1 : 0) : (index + (event.shiftKey ? -1 : 1) + tabs.length) % tabs.length;
        activateTab(tabs[next].launch.id, false);
      }
    };
    window.addEventListener('keydown', keys);
    return () => window.removeEventListener('keydown', keys);
  }, [tabs, active, modal, data, closeTab, newSession, activateTab]);

  const launch = (input: Launch) => {
    setError('');
    setModal(undefined);
    setTabs(current => [...current, { launch: input, status: 'starting' }]);
    activateTab(input.id);
  };
  const select = (session: Session) => {
    const tab = tabForHistory(tabs, session.id, active);
    if (tab) activateTab(tab.launch.id);
    else { setSelected(session); setComparison(undefined); setActive(''); }
    setError('');
  };
  const restart = async (session: Session, resumeConversation = false) => {
    setChecking(session.id);
    setError('');
    try { await api.resolve(session.id); setPrevious(session); setResume(resumeConversation); setModal('new'); }
    catch (reason) { setError(readable(reason)); }
    finally { setChecking(undefined); }
  };
  const activeTab = tabs.find(tab => tab.launch.id === active);
  const history = data?.sessions ?? [];
  const activeSession = activeTab?.session ?? history.find(session => session.id === (activeTab && historyId(activeTab))) ?? selected;
  const activeStatus = sessionIndicator(activeTab);
  const router = data?.routers.find(item => item.id === activeSession?.router_id);
  const waiting = tabs.filter(tab => tab.status === 'running' && tab.reportedWork === 'waiting_for_reply' && !tab.error);
  const transientTabs = tabs.filter(tab => !history.some(session => session.id === historyId(tab)));
  const pinned = new Set(data?.session_layout.pinned);
  const pinnedSessions = history.filter(session => pinned.has(session.id));
  const unpinnedSessions = history.filter(session => !pinned.has(session.id));
  const updateLayout = (task: ReturnType<typeof api.groupSessions>) => void task.then(layout => {
    setData(current => current && { ...current, session_layout: layout });
  }).catch(reason => setError(readable(reason)));
  const renderSaved = (session: Session) => {
    const tab = tabForHistory(tabs, session.id, active);
    return <SessionRow session={session} tab={tab} selected={session.id === (selected?.id ?? (activeTab && historyId(activeTab)))}
      pinned={pinned.has(session.id)} onPin={() => updateLayout(api.pinSession(session.id, !pinned.has(session.id)))}
      key={session.id} onSelect={() => select(session)} />;
  };
  const switchItems = [...pinnedSessions, ...unpinnedSessions].map(session => ({
    id: session.id, name: session.name, cwd: session.cwd, agent: agents[session.agent],
    status: sessionIndicator(tabForHistory(tabs, session.id, active)).text,
    choose: () => { setModal(undefined); select(session); },
  })).concat(transientTabs.map(tab => ({
    id: tab.launch.id, name: tab.launch.name, cwd: tab.launch.cwd,
    agent: agents[tab.session?.agent ?? data?.runtimes.find(item => item.id === tab.launch.runtime_id)?.agent ?? 'generic'],
    status: sessionIndicator(tab).text,
    choose: () => { setModal(undefined); activateTab(tab.launch.id); },
  })));

  return <div className="app">
    <header className="topbar" data-deck-shortcuts>
      <div className="brand"><span className="brand-icon" aria-hidden="true">›_</span><h1>Agent Deck</h1></div>
      <div className="row">
        <button disabled={!switchItems.length} onClick={() => setModal('switch')} title="Find or switch sessions (Ctrl+K on Deck controls)">Sessions</button>
        <button className={waiting.length ? 'attention-action' : ''} disabled={!waiting.length}
          onClick={() => activateTab(waiting[(waiting.findIndex(tab => tab.launch.id === active) + 1) % waiting.length].launch.id)}>
          Next reply{waiting.length > 0 && ` (${waiting.length})`}
        </button>
        <button ref={helpButton} className="help-button" aria-label="Keyboard shortcuts" aria-haspopup="dialog" title="Keyboard shortcuts" onClick={() => setModal('shortcuts')}>?</button>
        <button onClick={() => setModal('settings')} disabled={!data}>Settings</button>
        <button className="accent" onClick={newSession} disabled={!data}>＋ New Session</button></div>
    </header>
    {error && <div className="global-error notice danger" role="alert"><span>{error}</span><button aria-label="Dismiss error" onClick={() => setError('')}>×</button></div>}
    <div className={`workspace ${sessionsOpen ? '' : 'sessions-collapsed'}`}>
      <aside aria-label="Sessions" data-deck-shortcuts>
        <div className="aside-title">
          <h2 hidden={!sessionsOpen}>Sessions</h2>
          <div className="aside-actions">
            <span hidden={!sessionsOpen}>{tabs.filter(live).length} active</span>
            <button className="session-toggle" aria-controls="session-list" aria-expanded={sessionsOpen}
              aria-label={sessionsOpen ? 'Collapse sessions' : 'Expand sessions'}
              title={sessionsOpen ? 'Collapse sessions' : 'Expand sessions'}
              onClick={() => setSessionsOpen(open => !open)}>{sessionsOpen ? '<' : '>'}</button>
          </div>
        </div>
        <div id="session-list" hidden={!sessionsOpen}>
          {!history.length && !tabs.length && <p className="muted">Sessions appear here after launch.</p>}
          {history.length > 0 && <label className="check session-grouping">
            <input type="checkbox" checked={data?.session_layout.grouped ?? false}
              onChange={event => updateLayout(api.groupSessions(event.target.checked))} /> Group by project
          </label>}
          {pinnedSessions.length > 0 && <section><h3 className="session-group-name">Pinned</h3>{pinnedSessions.map(renderSaved)}</section>}
          {transientTabs.map(tab =>
            <SessionRow key={tab.launch.id} session={tab.session} tab={tab} selected={tab.launch.id === active}
              onSelect={() => activateTab(tab.launch.id)} />)}
          {data?.session_layout.grouped ? projectGroups(unpinnedSessions).map(group =>
            <section key={group.path} className="session-group">
              <h3 className="session-group-name ellipsis" title={group.path}>{group.path}</h3>
              {group.sessions.map(renderSaved)}
            </section>) : unpinnedSessions.map(renderSaved)}
        </div>
      </aside>
      <main>
        <div className="tabs" aria-label="Terminal tabs" data-deck-shortcuts>
          {tabs.map(tab => {
            const status = sessionIndicator(tab);
            return <div key={tab.launch.id} className={`tab ${active === tab.launch.id ? 'selected' : ''}`}>
            <button onClick={() => activateTab(tab.launch.id)} aria-current={active === tab.launch.id ? 'true' : undefined}>
              <i className={`dot ${status.color}`} title={status.text} aria-label={status.text} />{agents[tab.session?.agent ?? data?.runtimes.find(item => item.id === tab.launch.runtime_id)?.agent ?? 'generic']} · {tab.launch.name}
              {status.color === 'waiting_for_reply' && <small className="reply-badge">Reply needed</small>}
            </button>
            <button aria-label={`Close ${tab.launch.name}`} className="tab-close" disabled={tab.status === 'starting'} onClick={() => void closeTab(tab.launch.id)}>×</button>
          </div>; })}
          <button className="add-tab" onClick={newSession} disabled={!data} aria-label="New terminal session">＋</button>
          {activeTab && tabs.length > 1 && <button className="compare-action" onClick={() => {
            if (comparison) setComparison(undefined);
            else setModal('compare');
          }}>{comparison ? 'Close split' : 'Side by side'}</button>}
        </div>
        {activeTab?.error && <p className="terminal-error notice danger" role="alert">{activeTab.error}</p>}
        <div className="terminal-area">
          {data && tabs.map(tab => <TerminalPane key={tab.launch.id} tab={tab} active={active === tab.launch.id}
            pane={comparison?.left === tab.launch.id ? 'left' : comparison?.right === tab.launch.id ? 'right' : undefined}
            autoFocus={focusTerminal} onActivate={() => { if (tab.launch.id !== active) activateTab(tab.launch.id, false); }}
            preferences={data.preferences} update={update} onStart={onStart} />)}
          {!activeTab && (selected ? <div className="empty">
            <div className="empty-icon" aria-hidden="true">›_</div><h2>{selected.name}</h2>
            <p className="path">{selected.cwd}</p><p className="muted">Saved session</p>
            <div className="row">
              {selected.agent === 'gajae' && !selected.gjc?.history_disabled && <button className="accent" disabled={!!checking} onClick={() => void restart(selected, true)}>Resume Gajae-Code conversation</button>}
              <button className={selected.agent === 'gajae' ? undefined : 'accent'} disabled={!!checking} onClick={() => void restart(selected)}>Restart as new process</button>
              <button onClick={() => void api.remove('session', selected.id).then(() => { setSelected(undefined); return refresh(); }).catch(reason => setError(readable(reason)))}>Remove from history</button></div>
          </div> : <div className="empty">
            <div className="empty-icon" aria-hidden="true">›_</div><h2>{data ? 'Open a terminal session' : 'Opening Agent Deck…'}</h2>
            {data && <><p className="muted">{data.runtimes.length ? 'Choose a runtime and working directory.' : 'Register an Agent runtime in Settings to get started.'}</p>
              <button className="accent" onClick={data.runtimes.length ? newSession : () => setModal('settings')}>{data.runtimes.length ? 'New Session' : 'Register a runtime'}</button></>}
          </div>)}
        </div>
      </main>
    </div>
    <footer className="statusbar">
      <span>{activeSession ? agents[activeSession.agent] : 'No active session'}</span>
      {activeSession && <><span>{activeSession.version}</span><span className="grow ellipsis" title={activeSession.cwd}>{activeSession.cwd}</span></>}
      {activeTab && <span title={activeStatus.detail}>{activeStatus.text}{activeTab.code === 0 && ' · exit 0'}</span>}
      {activeSession && <span title={activeSession.endpoint ?? ''}>{activeSession.endpoint ? `${activeSession.agent === 'claude' ? 'Router' : 'Manual router'} · ${activeSession.endpoint}${router && router.updated_at !== activeSession.router_revision ? ' (profile changed)' : ''}` : 'Agent-managed router'}</span>}
      {data && <span className="app-version">Agent Deck v{data.app_version}</span>}
    </footer>
    {modal === 'shortcuts' && <Modal title="Keyboard shortcuts" onClose={() => setModal(undefined)}>
      <KeyboardShortcuts />
    </Modal>}
    {modal === 'switch' && <Modal title="Switch session" onClose={() => setModal(undefined)}><SessionSwitcher items={switchItems} /></Modal>}
    {modal === 'compare' && <Modal title="View two sessions side by side" onClose={() => setModal(undefined)}>
      <SessionSwitcher items={tabs.filter(tab => tab.launch.id !== active).map(tab => ({
        id: tab.launch.id, name: tab.launch.name, cwd: tab.session?.cwd ?? tab.launch.cwd,
        agent: agents[tab.session?.agent ?? data?.runtimes.find(item => item.id === tab.launch.runtime_id)?.agent ?? 'generic'],
        status: sessionIndicator(tab).text,
        choose: () => {
          if (active) {
            setComparison({ left: active, right: tab.launch.id });
            setModal(undefined);
          }
        },
      }))} />
    </Modal>}
    {data && (modal === 'settings' || modal === 'new') && <Modal title={modal === 'settings' ? 'Settings' : resume ? 'Resume conversation' : previous ? 'Restart Session' : 'New Session'} onClose={() => setModal(undefined)}>
      {modal === 'settings' ? <Settings data={data} refresh={refresh} /> : <NewSession data={data} previous={previous} resume={resume}
        runningPaths={tabs.filter(live).map(tab => tab.session?.cwd ?? tab.launch.cwd)} onLaunch={launch} onSettings={() => setModal('settings')} />}
    </Modal>}
  </div>;
}
const root = document.getElementById('root');
window.addEventListener('error', () => {
  void invoke('report_ui_fault', { kind: 'script_error' }).catch(() => {});
}, { once: true });
window.addEventListener('unhandledrejection', () => {
  void invoke('report_ui_fault', { kind: 'unhandled_rejection' }).catch(() => {});
}, { once: true });
if (root) createRoot(root).render(<App />);

