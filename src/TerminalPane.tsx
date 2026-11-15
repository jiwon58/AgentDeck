import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { listen } from '@tauri-apps/api/event';
import { api, readable } from './api';
import type { Agent, Launch, Preferences, Session, TerminalEvent, WorkStatus } from './types';
import '@xterm/xterm/css/xterm.css';

export interface TabState {
  launch: Launch; session?: Session; status: string; error?: string; code?: number | null;
  reportedWork?: WorkStatus;
}
interface Props {
  tab: TabState;
  active: boolean;
  autoFocus: boolean;
  preferences: Preferences;
  update: (id: string, patch: Partial<TabState>) => void;
  onStart: (session: Session) => void;
}

export function TerminalPane({ tab, active, autoFocus, preferences, update, onStart }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const instance = useRef<Terminal | null>(null);
  const fitRef = useRef<(() => void) | null>(null);
  const initial = useRef<{ tab: TabState; preferences: Preferences } | null>({ tab, preferences });
  useEffect(() => {
    if (!host.current || !initial.current) return;
    const { tab: { launch: initialLaunch }, preferences } = initial.current;
    const launch = { ...initialLaunch };
    initial.current = null;
    const id = launch.id;
    const terminal = new Terminal({
      fontFamily: 'Cascadia Mono, Consolas, monospace', fontSize: preferences.font_size, scrollback: preferences.scrollback,
      cursorBlink: true, convertEol: false,
      theme: { background: '#10141d', foreground: '#dce5f3', cursor: '#7ac6f4', selectionBackground: '#354a64' },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host.current);
    instance.current = terminal;
    let disposed = false;
    let started = false;
    let connected = false;
    let wantsDrain = false;
    let busy = false;
    let sequence = 0;
    let runningAgent: Agent | undefined;
    let release: (() => void) | undefined;
    let input = Promise.resolve();
    const fail = (error: unknown) => { if (!disposed) update(id, { error: readable(error) }); };
    const hostStatus = terminal.parser.registerOscHandler(777, payload => {
      if (runningAgent !== 'gajae' || disposed || !connected) return false;
      let reportedWork: WorkStatus;
      switch (payload) {
        case 'notify;Terax;gjc;working': reportedWork = 'in_progress'; break;
        case 'notify;Terax;gjc;attention': reportedWork = 'waiting_for_reply'; break;
        case 'notify;Terax;gjc;finished': reportedWork = 'completed'; break;
        default: return false;
      }
      update(id, { reportedWork });
      return true;
    });
    const write = (bytes: string | Uint8Array) => new Promise<void>(resolve => {
      if (disposed) resolve();
      else terminal.write(bytes, resolve);
    });
    const pump = async () => {
      wantsDrain = true;
      if (busy || !started || disposed) return;
      busy = true;
      try {
        while (wantsDrain && !disposed) {
          wantsDrain = false;
          const events = await api.drain(id);
          for (const event of events) {
            if (disposed) break;
            if (sequence !== event.sequence_number) {
              update(id, { reportedWork: undefined });
              terminal.reset();
              await write('\r\n[Agent Deck: output omitted because the terminal queue was full]\r\n');
            }
            sequence = event.sequence_number + 1;
            if (event.event_type === 'terminal_output') await write(Uint8Array.from(event.payload));
            if (event.event_type === 'terminal_error') fail(event.payload);
            if (event.event_type === 'terminal_exited') {
              connected = false;
              update(id, { status: event.payload.status, code: event.payload.code });
            }
          }
        }
      } catch (error) { fail(error); }
      finally { busy = false; }
    };
    const send = (data: string) => {
      if (!connected || disposed) return;
      input = input.then(async () => {
        // Iterate by code point so chunking never splits a surrogate pair.
        let chunk = '';
        for (const character of data) {
          chunk += character;
          if (chunk.length >= 8000) { await api.write(id, chunk); chunk = ''; }
        }
        if (chunk) await api.write(id, chunk);
      }).catch(fail);
    };
    const data = terminal.onData(send);
    const resize = () => {
      if (!host.current?.clientWidth || !host.current.clientHeight) return;
      fit.fit();
      if (connected) void api.resize(id, terminal.cols, terminal.rows).catch(fail);
    };
    fitRef.current = resize;
    const observer = new ResizeObserver(resize);
    observer.observe(host.current);
    void (async () => {
      try {
        const unlisten = await listen<TerminalEvent>('terminal-ready', event => {
          if (event.payload.session_id === id) void pump();
        });
        if (disposed) { unlisten(); return; }
        release = unlisten;
        resize();
        const result = api.start({ ...launch, cols: terminal.cols, rows: terminal.rows });
        launch.env = {};
        launch.token = '';
        launch.extra = [];
        const session = await result;
        if (disposed) { await api.close(id); return; }
        runningAgent = session.agent;
        started = connected = true;
        update(id, { session, status: 'running', launch });
        onStart(session);
        resize();
        await pump();
      } catch (error) {
        if (!disposed) {
          update(id, { status: 'failed', error: readable(error), launch: { ...launch, env: {}, token: '', extra: [] } });
          await write(`\r\n[Agent Deck: launch failed]\r\n`);
        }
      }
    })();
    return () => {
      disposed = true;
      observer.disconnect();
      release?.();
      data.dispose();
      hostStatus.dispose();
      terminal.dispose();
      instance.current = null;
      fitRef.current = null;
    };
  }, []);
  useEffect(() => {
    const terminal = instance.current;
    if (!terminal) return;
    terminal.options.fontSize = preferences.font_size;
    terminal.options.scrollback = preferences.scrollback;
    if (active) fitRef.current?.();
  }, [active, preferences.font_size, preferences.scrollback]);
  useEffect(() => {
    if (active && autoFocus) instance.current?.focus();
  }, [active, autoFocus]);
  return <div ref={host} className="terminal-pane" hidden={!active} aria-label="Session terminal" />;
}

