import { useState, type FormEvent } from 'react';
import { api, browse, environment, lines, readable } from './api';
import { RouterModels } from './RouterModels';
import { GjcPresetSettings } from './GjcPresets';
import { AgentInstallSettings, type InstallReview } from './AgentInstallSettings';
import { agents, agentKeys, type Agent, type Candidate, type Overview, type Probe, type Router } from './types';

type Refresh = () => Promise<void>;
type Task = (work: () => Promise<void>) => Promise<void>;

export function Settings({ data, refresh }: { data: Overview; refresh: Refresh }) {
  const [section, setSection] = useState('Agent Runtimes');
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  const [review, setReview] = useState<InstallReview>();
  const task: Task = async work => {
    setBusy(true);
    setMessage('');
    try { await work(); } catch (error) { setMessage(readable(error)); }
    finally { setBusy(false); }
  };
  return <>
    <nav className="settings-nav" aria-label="Settings sections">
      {['Agent Runtimes', 'Install Agents', 'GJC Presets', 'Local LLM Router', 'Terminal Preferences', 'Application & Offline Settings'].map(name =>
        <button key={name} disabled={busy} className={section === name ? 'selected' : ''} onClick={() => { setReview(undefined); setSection(name); setMessage(''); }}>{name}</button>)}
    </nav>
    {message && <p className="notice danger" role="alert">{message}</p>}
    <fieldset className="settings-body" disabled={busy}>
      {section === 'Agent Runtimes' && <RuntimeSettings data={data} refresh={refresh} task={task} review={review} />}
      {section === 'Install Agents' && <AgentInstallSettings task={task} onReview={value => {
        setReview(value); setSection('Agent Runtimes');
      }} />}
      {section === 'GJC Presets' && <GjcPresetSettings data={data} refresh={refresh} />}
      {section === 'Local LLM Router' && <RouterSettings data={data} refresh={refresh} task={task} />}
      {section === 'Terminal Preferences' && <TerminalSettings data={data} refresh={refresh} task={task} />}
      {section === 'Application & Offline Settings' && <OfflineSettings data={data} refresh={refresh} task={task} />}
    </fieldset>
    {busy && <p className="muted" role="status">Working…</p>}
  </>;
}

function RuntimeSettings({ data, refresh, task, review }: { data: Overview; refresh: Refresh; task: Task; review?: InstallReview }) {
  const [agent, setAgent] = useState<Agent>(review?.agent ?? 'gajae');
  const [root, setRoot] = useState(review?.directory ?? '');
  const [path, setPath] = useState(review?.candidate.path ?? '');
  const [candidates, setCandidates] = useState<Candidate[]>([]);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [scanned, setScanned] = useState(false);
  const [candidate, setCandidate] = useState<Candidate | undefined>(review?.candidate);
  const [name, setName] = useState(review ? agents[review.agent] : '');
  const [args, setArgs] = useState('');
  const [probeArgs, setProbeArgs] = useState('');
  const [override, setOverride] = useState('');
  const [dependencies, setDependencies] = useState('');
  const [bindings, setBindings] = useState('');
  const [approved, setApproved] = useState(false);
  const [probe, setProbe] = useState(true);
  const select = (item: Candidate) => {
    setCandidate(item); setApproved(false); setName(agents[agent]);
    setArgs(''); setProbeArgs(''); setOverride(''); setDependencies(''); setBindings('');
  };
  const scan = (path: string) => task(async () => {
    setCandidate(undefined);
    const result = await api.scan(agent, path);
    setRoot(path);
    setCandidates(result.candidates);
    setWarnings(result.warnings);
    setScanned(true);
    await refresh();
  });
  const register = (event: FormEvent) => {
    event.preventDefault();
    if (!candidate || !approved) return;
    void task(async () => {
      await api.register({ agent, candidate, name, arguments: lines(args), probe_args: lines(probeArgs),
        root, cwd_override: override || null, dependencies, approved, probe, router_environment: environment(bindings) });
      setCandidate(undefined);
      await refresh();
    });
  };
  return <div className="stack">
    <label>Agent<select value={agent} onChange={event => {
      setAgent(event.target.value as Agent); setRoot(''); setPath(''); setCandidate(undefined); setCandidates([]); setScanned(false); setWarnings([]);
    }}>{agentKeys.map(key => <option key={key} value={key}>{agents[key]}</option>)}</select></label>
    <div className="row end"><label className="grow">Installation root<input value={root} onChange={e => setRoot(e.target.value)} placeholder="D:\AI-Agents\gajae-code" /></label>
      <button onClick={() => void task(async () => { const selected = await browse(true); if (selected) setRoot(selected); })}>Browse</button>
      <button className="accent" disabled={!root} onClick={() => void scan(root)}>Scan</button>
    </div>
    {data.roots.filter(item => item.agent === agent).map(item => <div className="data-row" key={item.id}>
      <span className="grow path">{item.path}</span><button onClick={() => void scan(item.path)}>Rescan</button>
      <button onClick={() => void task(async () => { await api.remove('root', item.id); await refresh(); })}>Remove</button>
    </div>)}
    {warnings.length > 0 && <details><summary>Scan warnings ({warnings.length})</summary><ul>{warnings.map((value, index) => <li key={index}>{value}</li>)}</ul></details>}
    {scanned && candidates.length === 0 && <p className="muted">No matching executable or launcher found. Inspect a command profile below.</p>}
    {candidates.map(item => <div className="data-row" key={item.path}>
      <span className="grow path">{item.path}<small>{item.architecture} · {item.launch_type} · {item.sha256}</small></span>
      {item.launch_type === 'native_exe' ? <button onClick={() => select(item)}>Review</button> :
        <span className="muted">Register its interpreter below</span>}
    </div>)}
    <div className="row end"><label className="grow">Custom executable or interpreter<input value={path} onChange={e => setPath(e.target.value)} /></label>
      <button onClick={() => void task(async () => { const selected = await browse(false); if (selected) { setPath(selected); select(await api.inspect(selected)); } })}>Browse</button>
      <button disabled={!path} onClick={() => void task(async () => select(await api.inspect(path)))}>Inspect</button>
    </div>
    {candidate && <form className="review stack" aria-label="Runtime approval" onSubmit={register}>
      <h3>Review executable</h3><div className="path">{candidate.path}<small>{candidate.architecture} · SHA-256 {candidate.sha256}</small></div>
      <label>Runtime name<input value={name} onChange={e => setName(e.target.value)} required /></label>
      <div className="two">
        <label>Launch arguments, one per line<textarea value={args} onChange={e => setArgs(e.target.value)} /></label>
        <label>Version probe arguments before --version<textarea value={probeArgs} onChange={e => setProbeArgs(e.target.value)} disabled={!probe} /></label>
      </div>
      <div className="two">
        <label>Working directory override, optional<input value={override} onChange={e => setOverride(e.target.value)} /></label>
        <label>Dependencies or interpreter notes<input value={dependencies} onChange={e => setDependencies(e.target.value)} /></label>
      </div>
      {agent !== 'claude' && <label>Router environment bindings, optional<textarea value={bindings}
        placeholder="VARIABLE_NAME=endpoint" spellCheck={false} onChange={e => setBindings(e.target.value)} />
        <small>One NAME=endpoint or NAME=model per line. Use variables documented for this executable.</small>
      </label>}
      <p className="muted">Arguments are saved with the runtime. Enter credentials as session environment overrides.</p>
      <label className="check"><input type="checkbox" checked={probe} onChange={e => setProbe(e.target.checked)} /> Run the reviewed command with --version before registering</label>
      <label className="check"><input type="checkbox" checked={approved} onChange={e => setApproved(e.target.checked)} /> I trust this executable, its arguments and dependencies</label>
      {approved && <div className="path"><small>Launch command as argument array</small><code>{JSON.stringify([candidate.path, ...lines(args)])}</code>
        {probe && <><small>Probe command as argument array</small><code>{JSON.stringify([candidate.path, ...lines(probeArgs), '--version'])}</code></>}
      </div>}
      <button className="accent" disabled={!approved}>Register runtime</button>
    </form>}
    <h3>Registered runtimes</h3>
    {data.runtimes.filter(item => item.agent === agent).map(item => <div className="data-row" key={item.id}>
      <span className="grow path"><strong>{item.name} · {item.version}</strong><small>{item.executable.path}</small><small>{item.executable.architecture} · SHA-256 {item.executable.sha256}</small></span>
      <button onClick={() => void task(async () => { await api.remove('runtime', item.id); await refresh(); })}>Remove</button>
    </div>)}
  </div>;
}

const emptyRouter: Router = { id: '', name: '', scheme: 'http', host: '127.0.0.1', port: 0, openai_path: '/v1',
  anthropic_path: null, model: '', authentication: 'none', enabled: true, allow_remote: false, created_at: 0, updated_at: 0 };
function RouterSettings({ data, refresh, task }: { data: Overview; refresh: Refresh; task: Task }) {
  const [profile, setProfile] = useState<Router>(emptyRouter);
  const [token, setToken] = useState('');
  const [result, setResult] = useState<Probe>();
  const [modelContext, setModelContext] = useState(0);
  const resetModels = () => { setResult(undefined); setModelContext(current => current + 1); };
  const patch = (change: Partial<Router>) => {
    setProfile({ ...profile, ...change });
    if (Object.keys(change).some(key => key !== 'model' && key !== 'name' && key !== 'enabled')) resetModels();
  };
  return <div className="stack">
    <div className="row end"><label className="grow">Router profile<select value={profile.id} onChange={e => {
      setProfile(data.routers.find(item => item.id === e.target.value) ?? emptyRouter); setToken(''); resetModels();
    }}><option value="">New profile</option>{data.routers.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
      {profile.id && <button onClick={() => void task(async () => { await api.remove('router', profile.id); setProfile(emptyRouter); setToken(''); resetModels(); await refresh(); })}>Remove</button>}
    </div>
    <form className="stack" onSubmit={e => { e.preventDefault(); void task(async () => { setProfile(await api.router(profile)); setToken(''); resetModels(); await refresh(); }); }}>
      <label>Profile name<input required value={profile.name} onChange={e => patch({ name: e.target.value })} /></label>
      <div className="address"><label>Scheme<select value={profile.scheme} onChange={e => patch({ scheme: e.target.value })}><option>http</option><option>https</option></select></label>
        <label>Host<input required value={profile.host} onChange={e => patch({ host: e.target.value, allow_remote: false })} /></label>
        <label>Port<input required type="number" min={1} max={65535} value={profile.port || ''} onChange={e => patch({ port: Number(e.target.value) })} /></label></div>
      <div className="two">
        <div><label className="check"><input type="checkbox" checked={profile.openai_path !== null} onChange={e => patch({ openai_path: e.target.checked ? '/v1' : null })} /> OpenAI Chat Completions</label>
          {profile.openai_path !== null && <label>OpenAI API base path<input value={profile.openai_path} onChange={e => patch({ openai_path: e.target.value })} /></label>}</div>
        <div><label className="check"><input type="checkbox" checked={profile.anthropic_path !== null} onChange={e => patch({ anthropic_path: e.target.checked ? '' : null })} /> Anthropic Messages</label>
          {profile.anthropic_path !== null && <label>Anthropic API base path<input value={profile.anthropic_path} onChange={e => patch({ anthropic_path: e.target.value })} placeholder="Empty for server root" /></label>}</div>
      </div>
      <div className="two"><label>Default model<input value={profile.model} onChange={e => patch({ model: e.target.value })} /></label>
        <label>Authentication<select value={profile.authentication} onChange={e => { patch({ authentication: e.target.value as Router['authentication'] }); setToken(''); }}><option value="none">None</option><option value="bearer">Bearer token at launch</option></select></label></div>
      {profile.authentication === 'bearer' && <label>Temporary connection test token<input type="password" autoComplete="off" value={token} onChange={e => { setToken(e.target.value); resetModels(); }} /></label>}
      <details><summary>Advanced network settings</summary><label className="check"><input type="checkbox" checked={profile.allow_remote} onChange={e => patch({ allow_remote: e.target.checked })} /> I allow connections to this non-loopback host</label>
        <small>Credentials and Agent requests can leave this computer. Confirm the host and transport.</small></details>
      <label className="check"><input type="checkbox" checked={profile.enabled} onChange={e => patch({ enabled: e.target.checked })} /> Enabled for new sessions</label>
      <div className="row"><button className="accent">Save profile</button><button type="button" disabled={!profile.port || !profile.name} onClick={() => void task(async () => setResult(await api.probe(profile, token)))}>Test Connection</button></div>
      {result && <div className="notice" role="status">TCP {result.tcp}{result.http && ` · HTTP ${result.http}`} · {result.result}</div>}
    </form>
    <RouterModels key={modelContext} profile={profile} token={token} task={task} onDefault={model => patch({ model })} />
  </div>;
}

function TerminalSettings({ data, refresh, task }: { data: Overview; refresh: Refresh; task: Task }) {
  const [fontSize, setFontSize] = useState(data.preferences.font_size);
  const [scrollback, setScrollback] = useState(data.preferences.scrollback);
  return <form className="stack" onSubmit={e => { e.preventDefault(); void task(async () => { await api.preferences(fontSize, scrollback); await refresh(); }); }}>
    <div className="two"><label>Terminal font size<input type="number" min={10} max={26} value={fontSize} onChange={e => setFontSize(Number(e.target.value))} required /></label>
      <label>Scrollback lines<input type="number" min={100} max={10000} value={scrollback} onChange={e => setScrollback(Number(e.target.value))} required /></label></div>
    <button className="accent">Save preferences</button>
  </form>;
}

function OfflineSettings({ data, refresh, task }: { data: Overview; refresh: Refresh; task: Task }) {
  const [config, setConfig] = useState('');
  const [result, setResult] = useState('');
  const exportConfig = () => {
    setConfig(JSON.stringify({ schema: 1, terminal: { font_size: data.preferences.font_size, scrollback: data.preferences.scrollback },
      routers: data.routers }, null, 2));
    setResult('Settings exported to the text field.');
  };
  const importConfig = () => task(async () => {
    const parsed: unknown = JSON.parse(config);
    if (!parsed || typeof parsed !== 'object' || !('schema' in parsed) || parsed.schema !== 1 || !('terminal' in parsed) || !('routers' in parsed) || !Array.isArray(parsed.routers)) {
      throw new Error('Unsupported settings format');
    }
    const terminal = parsed.terminal as { font_size: number; scrollback: number };
    if (!Number.isInteger(terminal?.font_size) || !Number.isInteger(terminal?.scrollback)) throw new Error('Invalid terminal settings');
    await api.importSettings(terminal.font_size, terminal.scrollback, parsed.routers);
    await refresh();
    setResult('Settings imported. Router profiles were added as disabled profiles.');
  });
  return <div className="stack">
    <h3>Local diagnostics</h3>
    <div className="notice"><strong>SQLite connected</strong><small className="path">{data.data_directory}</small>
      <small>{data.runtimes.length} registered runtimes · {data.routers.length} router profiles · {data.sessions.length} saved sessions</small></div>
    <div className="notice">
      <strong>Diagnostic log</strong>
      <small className="path">{data.log_file ?? 'The log file is unavailable. Check access to your local application data and temporary folders.'}</small>
      {data.log_file && <button onClick={() => void task(async () => {
        await navigator.clipboard.writeText(data.log_file!);
        setResult('Log path copied.');
      })}>Copy log path</button>}
    </div>
    <p className="muted">Agent Deck includes its interface assets locally and keeps terminal transcripts in memory. WebView2 is required on Windows.</p>
    <label>Router and terminal settings JSON<textarea className="json" value={config} onChange={e => { setConfig(e.target.value); setResult(''); }} /></label>
    <div className="row"><button onClick={exportConfig}>Export settings</button><button disabled={!config} onClick={() => void importConfig()}>Import settings</button></div>
    {result && <p role="status">{result}</p>}
  </div>;
}

