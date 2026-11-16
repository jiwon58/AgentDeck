import { useState } from 'react';
import { browse, environment, lines, readable } from './api';
import { GjcPresetLaunch } from './GjcPresets';
import { agents, agentKeys, type Agent, type Launch, type Overview, type Session } from './types';

type Props = {
  data: Overview; previous?: Session; resume?: boolean; runningPaths: string[];
  onLaunch: (launch: Launch) => void; onSettings: () => void;
};

export function NewSession(props: Props) {
  const { data, previous, resume } = props;
  const preferred = data.runtimes.find(item => item.id === data.preferences.last_runtime);
  const hasPreset = data.gjc_presets.some(preset => preset.enabled);
  const [presetMode, setPresetMode] = useState(Boolean(previous?.gjc) || (!previous && hasPreset && (!preferred || preferred.agent === 'gajae')));
  return <div className="stack">
    {!resume && <nav className="settings-nav" aria-label="Launch configuration">
      <button type="button" className={presetMode ? 'selected' : ''} onClick={() => setPresetMode(true)}>GJC preset</button>
      <button type="button" className={!presetMode ? 'selected' : ''} onClick={() => setPresetMode(false)}>Agent runtime</button>
    </nav>}
    {presetMode ? <GjcPresetLaunch {...props} /> : <RuntimeLaunch {...props} />}
  </div>;
}

function RuntimeLaunch({ data, previous, resume, runningPaths, onLaunch, onSettings }: Props) {
  const preferred = data.runtimes.find(item => item.id === (previous?.runtime_id ?? data.preferences.last_runtime));
  const [agent, setAgent] = useState<Agent>(previous?.agent ?? preferred?.agent ?? data.runtimes[0]?.agent ?? 'gajae');
  const [runtimeId, setRuntime] = useState(preferred?.id ?? '');
  const [name, setName] = useState(previous?.name ?? '');
  const [cwd, setCwd] = useState(previous?.cwd ?? data.preferences.last_cwd);
  const [routerId, setRouter] = useState(previous?.router_id ?? '');
  const [model, setModel] = useState(previous?.model ?? '');
  const [extra, setExtra] = useState('');
  const [env, setEnv] = useState('');
  const [token, setToken] = useState('');
  const [manual, setManual] = useState(false);
  const [protocol, setProtocol] = useState<Launch['manual_protocol']>();
  const [error, setError] = useState('');
  const runtimes = data.runtimes.filter(item => item.agent === agent);
  const runtime = runtimes.find(item => item.id === runtimeId);
  const router = data.routers.find(item => item.id === routerId);
  const effectiveCwd = runtime?.cwd_override || cwd;
  const overlap = effectiveCwd && runningPaths.some(path => path.toLowerCase() === effectiveCwd.toLowerCase());
  const requiresAnthropic = router && agent === 'claude' && router.anthropic_path === null;
  const manualProtocol = protocol ?? (router?.openai_path != null ? 'openai' : 'anthropic');
  return <form className="stack" onSubmit={event => {
    event.preventDefault();
    try {
      onLaunch({ id: crypto.randomUUID(), name, runtime_id: runtimeId, cwd: effectiveCwd, router_id: routerId || null,
        resume_from: resume ? previous?.id : undefined,
        model, extra: lines(extra), env: environment(env), token, manual_router: manual, manual_protocol: manualProtocol, cols: 80, rows: 24 });
    } catch (reason) { setError(readable(reason)); }
  }}>
    {previous && <p className="notice">{resume
      ? 'Choose the conversation to continue in the Gajae-Code picker. This terminal stays attached to the saved session. Re-enter credentials and any storage overrides.'
      : 'This starts a new process using today’s settings. Enter any credentials and advanced overrides again.'}</p>}
    {error && <p role="alert" className="notice danger">{error}</p>}
    <label>Session name<input value={name} readOnly={resume} onChange={e => setName(e.target.value)} required autoFocus /></label>
    <div className="two">
      <label>Agent<select value={agent} disabled={resume} onChange={e => { setAgent(e.target.value as Agent); setRuntime(''); setManual(false); setToken(''); }}>
        {agentKeys.map(key => <option key={key} value={key}>{agents[key]}</option>)}</select></label>
      <label>Runtime version<select required value={runtimeId} disabled={resume} onChange={e => { setRuntime(e.target.value); setManual(false); }}>
        <option value="">Select a runtime</option>{runtimes.map(item => <option value={item.id} key={item.id}>{item.version} · {item.name} · {item.executable.path}</option>)}
      </select></label>
    </div>
    {!runtimes.length && <div className="notice row"><span className="grow">Register a runtime for {agents[agent]} first.</span><button type="button" onClick={onSettings}>Open Settings</button></div>}
    <div className="row end"><label className="grow">Working directory<input required value={effectiveCwd} readOnly={resume || !!runtime?.cwd_override} onChange={e => setCwd(e.target.value)} /></label>
      <button type="button" disabled={resume || !!runtime?.cwd_override} onClick={() => void browse(true).then(path => { if (path) setCwd(path); }).catch(e => setError(readable(e)))}>Browse</button></div>
    {runtime?.cwd_override && <small>This runtime supplies a working directory override.</small>}
    {overlap && <p className="notice">An active session uses this directory. Concurrent edits can conflict.</p>}
    <div className="two"><label>Router profile<select value={routerId} onChange={e => { setRouter(e.target.value); setManual(false); setProtocol(undefined); setToken(''); }}>
      <option value="">Use Agent's own configuration</option>{data.routers.filter(item => item.enabled).map(item => <option key={item.id} value={item.id}>{item.name} · {item.host}:{item.port}</option>)}
      </select></label><label>Model ID, optional<input value={model} placeholder={router?.model || ''} onChange={e => setModel(e.target.value)} /></label></div>
    {requiresAnthropic && <p className="notice danger">Claude Code requires a router profile with Anthropic Messages enabled.</p>}
    {router?.authentication === 'bearer' && agent === 'claude' &&
      <label>Router bearer token<input required type="password" autoComplete="off" value={token} onChange={e => setToken(e.target.value)} /></label>}
    {router && agent !== 'claude' && <>
      <label>Router protocol<select value={manualProtocol} onChange={e => { setProtocol(e.target.value as Launch['manual_protocol']); setManual(false); }}>
        {router.openai_path !== null && <option value="openai">OpenAI Chat Completions</option>}
        {router.anthropic_path !== null && <option value="anthropic">Anthropic Messages</option>}
      </select></label>
      <label className="check notice"><input type="checkbox" checked={manual} onChange={e => setManual(e.target.checked)} />
        I configured this Agent for the selected protocol, endpoint and model. Deck supplies any registered environment bindings.</label>
    </>}
    <details><summary>Advanced launch options</summary>
      <label>Extra arguments, one per line<textarea value={extra} onChange={e => setExtra(e.target.value)} spellCheck={false} /></label>
      <label>Environment overrides, one NAME=value per line<textarea value={env} onChange={e => setEnv(e.target.value)} spellCheck={false} /></label>
      <small>These overrides are held for this launch only.</small>
    </details>
    <div className="row">{runtime && <small className="grow path">{runtime.executable.path}</small>}
      <button className="accent" disabled={!runtime || !!requiresAnthropic || (!!router && agent !== 'claude' && !manual)}>{resume ? 'Open conversation picker' : 'Start session'}</button></div>
  </form>;
}

