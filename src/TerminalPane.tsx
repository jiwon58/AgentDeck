import { useEffect, useRef, useState, type PointerEvent } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { listen } from '@tauri-apps/api/event';
import { api, readable } from './api';
import { harnessKey } from './terminalKeys';
import { legacyGjcStatus } from './legacyGjcStatus';
import { TerminalTools } from './TerminalTools';
import type { Agent, Launch, Preferences, Session, TerminalEvent, WorkStatus } from './types';
import '@xterm/xterm/css/xterm.css';

export interface TabState {
  launch: Launch; session?: Session; status: string; error?: string; code?: number | null;
  reportedWork?: WorkStatus;
}
interface Props {
  tab: TabState;
  active: boolean;
  pane?: 'left' | 'right';
  autoFocus: boolean;
  onActivate: () => void;
  preferences: Preferences;
  update: (id: string, patch: Partial<TabState>) => void;
  onStart: (session: Session) => void;
}

export function TerminalPane({ tab, active, pane, autoFocus, onActivate, preferences, update, onStart }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const scrollbar = useRef<HTMLInputElement>(null);
  const instance = useRef<Terminal | null>(null);
  const fitRef = useRef<(() => void) | null>(null);
  const [readOnly, setReadOnly] = useState(false);
  const inputLocked = useRef(false);
  const visible = active || !!pane;
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
      allowProposedApi: true, // Search highlighting uses decorations in xterm.js 5.5.
      theme: { background: '#10141d', foreground: '#dce5f3', cursor: '#7ac6f4', selectionBackground: '#354a64' },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    const element = host.current;
    terminal.open(element);
    const controls = new AbortController();
    const blockEdit = (event: Event) => {
      if (!inputLocked.current) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.type === 'input' && event.target instanceof HTMLTextAreaElement) event.target.value = '';
    };
    // Block user input at the DOM boundary. onData also carries replies to
    // terminal queries and must stay connected in a read-only pane.
    for (const type of ['paste', 'cut', 'drop', 'beforeinput', 'input', 'compositionstart', 'compositionupdate', 'compositionend']) {
      element.addEventListener(type, blockEdit, { capture: true, signal: controls.signal });
    }
    const blockKey = (event: KeyboardEvent) => {
      if (!inputLocked.current) return;
      const copy = event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'c' && terminal.hasSelection();
      if (!copy && event.key !== 'Tab') event.preventDefault();
      event.stopPropagation();
    };
    for (const type of ['keydown', 'keypress', 'keyup'] as const) {
      element.addEventListener(type, blockKey, { capture: true, signal: controls.signal });
    }
    element.addEventListener('mousedown', event => {
      if (terminal.modes.mouseTrackingMode !== 'none' && !event.shiftKey) blockEdit(event);
      else if (event.button === 1) blockEdit(event);
    }, { capture: true, signal: controls.signal });
    element.addEventListener('mousemove', event => {
      if (terminal.modes.mouseTrackingMode !== 'none' && !event.shiftKey) blockEdit(event);
    }, { capture: true, signal: controls.signal });
    element.addEventListener('auxclick', event => {
      if (event.button === 1) blockEdit(event);
    }, { capture: true, signal: controls.signal });
    element.addEventListener('wheel', event => {
      if (inputLocked.current && (terminal.modes.mouseTrackingMode !== 'none' || terminal.buffer.active.type === 'alternate')) {
        blockEdit(event);
        terminal.scrollLines(Math.sign(event.deltaY) * 3);
      }
    }, { capture: true, passive: false, signal: controls.signal });
    const syncScrollbar = () => {
      if (!scrollbar.current) return;
      const { baseY, viewportY } = terminal.buffer.active;
      scrollbar.current.max = String(baseY);
      scrollbar.current.value = String(viewportY);
      scrollbar.current.disabled = baseY === 0;
    };
    const scroll = terminal.onScroll(syncScrollbar);
    const buffer = terminal.buffer.onBufferChange(syncScrollbar);
    syncScrollbar();
    instance.current = terminal;
    let disposed = false;
    let started = false;
    let connected = false;
    let wantsDrain = false;
    let busy = false;
    let sequence = 0;
    let runningAgent: Agent | undefined;
    let runningVersion: string | undefined;
    let reported: WorkStatus | undefined;
    let hasNativeStatus = false;
    let renderStatus: ReturnType<typeof setTimeout> | undefined;
    let release: (() => void) | undefined;
    let input = Promise.resolve();
    const fail = (error: unknown) => { if (!disposed) update(id, { error: readable(error) }); };
    const report = (value: WorkStatus | undefined) => {
      if (value === reported) return;
      reported = value;
      update(id, { reportedWork: value });
    };
    const hostStatus = terminal.parser.registerOscHandler(777, payload => {
      if (runningAgent !== 'gajae' || disposed || !connected) return false;
      let reportedWork: WorkStatus | undefined;
      switch (payload) {
        case 'notify;Terax;gjc;working': reportedWork = 'in_progress'; break;
        case 'notify;Terax;gjc;attention': reportedWork = 'waiting_for_reply'; break;
        case 'notify;Terax;gjc;finished': reportedWork = 'completed'; break;
        case 'notify;Terax;gjc;idle': break;
        default: return false;
      }
      hasNativeStatus = true;
      report(reportedWork);
      return true;
    });
    const inspectRenderedStatus = () => {
      if (runningAgent !== 'gajae' || runningVersion !== '0.10.2' || hasNativeStatus) return;
      clearTimeout(renderStatus);
      renderStatus = setTimeout(() => {
        if (disposed || !connected || hasNativeStatus) return;
        const buffer = terminal.buffer.active;
        const end = buffer.baseY + terminal.rows;
        const lines = [];
        for (let line = Math.max(buffer.baseY, end - 22); line < end; line++) {
          lines.push(buffer.getLine(line)?.translateToString(true) ?? '');
        }
        report(legacyGjcStatus(lines, reported));
      }, 35);
    };
    const write = (bytes: string | Uint8Array) => new Promise<void>(resolve => {
      if (disposed) resolve();
      else terminal.write(bytes, () => { syncScrollbar(); inspectRenderedStatus(); resolve(); });
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
              report(undefined);
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
    terminal.attachCustomKeyEventHandler(event => {
      const bytes = harnessKey(event, runningAgent);
      if (!bytes) return true;
      event.preventDefault();
      send(bytes);
      return false;
    });
    const resize = () => {
      if (!host.current?.clientWidth || !host.current.clientHeight) return;
      fit.fit();
      syncScrollbar();
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
        runningVersion = session.version;
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
      controls.abort();
      clearTimeout(renderStatus);
      observer.disconnect();
      scroll.dispose();
      buffer.dispose();
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
    if (visible) fitRef.current?.();
  }, [visible, pane, preferences.font_size, preferences.scrollback]);
  useEffect(() => {
    if (active && autoFocus) instance.current?.focus();
  }, [active, autoFocus]);
  const scrollFromPointer = (event: PointerEvent<HTMLInputElement>) => {
    const scrollbar = event.currentTarget;
    const track = scrollbar.getBoundingClientRect();
    const fraction = (event.clientY - track.top - 9) / Math.max(1, track.height - 18);
    const line = Math.round(Math.max(0, Math.min(1, fraction)) * Number(scrollbar.max));
    scrollbar.valueAsNumber = line;
    instance.current?.scrollToLine(line);
  };
  return <div className={`terminal-pane${pane ? ' pane-' + pane : ''}${active && pane ? ' pane-focused' : ''}`}
    hidden={!visible} aria-label="Session terminal" onFocusCapture={onActivate}>
    <TerminalTools terminalRef={instance} name={tab.launch.name} comparison={!!pane} readOnly={readOnly}
      toggleReadOnly={() => { inputLocked.current = !inputLocked.current; setReadOnly(inputLocked.current); }} />
    <div className="terminal-body">
    <div ref={host} className="terminal-host" />
    <input ref={scrollbar} className="terminal-scrollbar" aria-label="Terminal scrollback"
      aria-orientation="vertical" type="range" min={0} max={0} defaultValue={0} disabled
      onInput={event => instance.current?.scrollToLine(event.currentTarget.valueAsNumber)}
      onPointerDown={event => {
        if (event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.focus({ preventScroll: true });
        event.currentTarget.setPointerCapture(event.pointerId);
        scrollFromPointer(event);
      }}
      onPointerMove={event => {
        if (event.currentTarget.hasPointerCapture(event.pointerId)) scrollFromPointer(event);
      }} />
    </div>
  </div>;
}

