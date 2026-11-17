import { useState } from 'react';
import { api, browse, browseGjcHistory, readable } from './api';
import type { GjcHistoryEntry, Launch, Overview, Session } from './types';

export function OpenGjcHistory({ data, previous, resume, onLaunch, onSettings }: {
  data: Overview; previous?: Session; resume?: boolean;
  onLaunch: (launch: Launch) => void; onSettings: () => void;
}) {
  const saved = resume ? previous?.external_history : undefined;
  const managed = resume ? previous?.native_gjc : undefined;
  const runtimes = data.runtimes.filter(item => item.agent === 'gajae' && item.verified && item.trusted &&
    (item.version === '0.10.2' || item.version === '0.20.0'));
  const [runtime, setRuntime] = useState(previous?.runtime_id ?? runtimes.find(item => item.version === '0.10.2')?.id ?? runtimes[0]?.id ?? '');
  const [directory, setDirectory] = useState(managed?.agent_directory ?? saved?.agent_directory ?? '');
  const [catalog, setCatalog] = useState<Awaited<ReturnType<typeof api.gjcHistory>>>();
  const [entry, setEntry] = useState<GjcHistoryEntry | undefined>(saved && previous ?
    { source: saved, cwd: previous.cwd, title: previous.name, modified_at: previous.created_at } : undefined);
  const [model, setModel] = useState('');
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    setError('');
    try { await action(); } catch (reason) { setError(readable(reason)); }
    finally { setBusy(false); }
  };
  const changeDirectory = (path: string) => { setDirectory(path); setEntry(undefined); setCatalog(undefined); };
  const name = previous?.name || entry?.title || 'GJC conversation';
  return <form className="stack" onSubmit={event => {
    event.preventDefault();
    if (!entry) return;
    onLaunch({ id: crypto.randomUUID(), name, runtime_id: runtime, cwd: entry.cwd, router_id: null,
      model, extra: [], env: {}, token, manual_router: false, cols: 80, rows: 24,
      external_history: entry.source, resume_from: saved || managed ? previous?.id : undefined });
  }}>
    <p className="notice">Close this conversation in any other terminal before opening it. GJC will continue writing to the original file.</p>
    {error && <p role="alert" className="notice danger">{error}</p>}
    <fieldset className="stack" disabled={busy}>
      <label>GJC runtime<select value={runtime} disabled={!!managed} onChange={event => setRuntime(event.target.value)} required>
        <option value="">Select a verified runtime</option>{runtimes.map(item =>
          <option key={item.id} value={item.id}>{item.version} · {item.name}</option>)}
      </select></label>
      {!runtimes.length && <button type="button" onClick={onSettings}>Open Settings</button>}
      <div className="row end">
        <label className="grow">Original GJC agent directory<input value={directory} readOnly={!!(saved || managed)}
          placeholder="GJC default" onChange={event => changeDirectory(event.target.value)} /></label>
        {!saved && !managed && <button type="button" onClick={() => void run(async () => {
          const path = await browse(true); if (path) changeDirectory(path);
        })}>Browse folder</button>}
      </div>
      <small>Choose the folder containing the original models.yml and config.yml. GJC reads those files directly.</small>
      {!saved && <div className="row">
        <button type="button" onClick={() => void run(async () => {
          setCatalog(managed && previous ? await api.managedGjcHistory(previous.id) : await api.gjcHistory(directory.trim()));
        })}>Find sessions</button>
        {!managed && <button type="button" onClick={() => void run(async () => {
          const path = await browseGjcHistory(); if (path) setEntry(await api.inspectGjcHistory(path, directory.trim()));
        })}>Choose .jsonl file</button>}
      </div>}
      {catalog?.limited && <small>Scan limit reached. Use the file picker for an unlisted session.</small>}
      {catalog && !catalog.entries.length && <small>No sessions found here. Select a different agent directory or choose a file directly.</small>}
      {!!catalog?.entries.length && <label>Existing conversation<select value={entry?.source.path ?? ''} onChange={event =>
        setEntry(catalog.entries.find(item => item.source.path === event.target.value))}>
        <option value="">Select a conversation</option>
        {catalog.entries.map(item => <option key={item.source.path} value={item.source.path}>
          {item.title || item.source.session_id} · {item.cwd} · {new Date(item.modified_at).toLocaleString()}
        </option>)}
      </select></label>}
      {entry && <div className="notice"><div className="path">{entry.source.path}</div><small>Working directory · {entry.cwd}</small></div>}
      <label>Replacement model, optional<input value={model} placeholder="provider/model"
        onChange={event => setModel(event.target.value)} /></label>
      {managed && <label>Router credential, if required<input type="password" autoComplete="off" value={token}
        onChange={event => setToken(event.target.value)} /></label>}
      <div className="row"><button className="accent" disabled={!entry || !runtimes.some(item => item.id === runtime)}>Resume original conversation</button></div>
    </fieldset>
  </form>;
}
