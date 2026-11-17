import { useEffect, useState } from 'react';
import { api, browse, browseYaml, readable } from './api';
import type { GjcYamlPreset, Launch, ModelCatalog, Overview, Session } from './types';

export function GjcYaml({ data, onLaunch, previous }: { data: Overview; onLaunch?: (launch: Launch) => void; previous?: Session }) {
  const runtimes = data.runtimes.filter(runtime => runtime.agent === 'gajae' && runtime.trusted && runtime.verified &&
    ['0.10.2', '0.20.0'].includes(runtime.version));
  const routers = data.routers.filter(router => router.enabled);
  const [preset, setPreset] = useState<GjcYamlPreset>({
    name: '', runtime_id: previous?.runtime_id ?? runtimes[0]?.id ?? '', router_id: previous?.router_id ?? routers[0]?.id ?? '', anthropic: false,
    models_template: '', config_template: '', provider: 'deck-router', default_model: '', models: [],
    context: 0, max_output: 0, location: 'AppData', history_root: '',
  });
  const [saved, setSaved] = useState<Record<string, GjcYamlPreset>>({});
  const [catalog, setCatalog] = useState<ModelCatalog>();
  const [cwd, setCwd] = useState(previous?.cwd ?? data.preferences.last_cwd);
  const [token, setToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const [error, setError] = useState('');
  const profile = routers.find(router => router.id === preset.router_id);
  const update = <K extends keyof GjcYamlPreset>(key: K, value: GjcYamlPreset[K]) =>
    setPreset(previous => ({ ...previous, [key]: value }));
  useEffect(() => { void api.yamlPresets().then(setSaved).catch(reason => setError(readable(reason))); }, []);
  const task = async (work: () => Promise<void>) => {
    setBusy(true);
    setMessage('');
    setError('');
    try { await work(); } catch (reason) { setError(readable(reason)); }
    finally { setBusy(false); }
  };
  const protocol = preset.anthropic ? 'anthropic' : 'openai';
  const protocolAvailable = profile && (preset.anthropic ? profile.anthropic_path !== null : profile.openai_path !== null);
  const missing = catalog?.complete ? preset.models.filter(id => !catalog.models.includes(id)) : [];
  const defaultReady = !!catalog?.complete && catalog.models.includes(preset.default_model)
    && (!preset.models.length || preset.models.includes(preset.default_model));
  const ready = runtimes.some(runtime => runtime.id === preset.runtime_id) && protocolAvailable
    && (profile?.authentication !== 'bearer' || token.trim()) && defaultReady && !missing.length && cwd.trim();
  return <form className="stack" onSubmit={event => {
    event.preventDefault();
    if (!onLaunch || !catalog || !ready) return;
    onLaunch({ id: crypto.randomUUID(), name: preset.name.trim() || preset.default_model,
      cwd: cwd.trim(), runtime_id: preset.runtime_id, router_id: preset.router_id, model: preset.default_model,
      token, extra: [], env: {}, manual_router: false, cols: 80, rows: 24, yaml: { preset, catalog: catalog.models } });
    setToken('');
  }}>
    {error && <p className="notice danger" role="alert">{error}</p>}
    {message && <p className="notice" role="status">{message}</p>}
    <fieldset className="settings-body stack" disabled={busy}>
      <div className="two">
        <label>Saved YAML preset<select value="" onChange={event => {
          const stored = saved[event.target.value];
          if (stored) { setPreset(stored); setCatalog(undefined); setToken(''); }
        }}><option value="">Load a preset</option>{Object.keys(saved).sort().map(name => <option key={name}>{name}</option>)}</select></label>
        <label>YAML preset name<input value={preset.name} onChange={event => update('name', event.target.value)} /></label>
        <label>GJC runtime<select value={preset.runtime_id} onChange={event => update('runtime_id', event.target.value)}>
          <option value="">Select a verified runtime</option>{runtimes.map(runtime => <option key={runtime.id} value={runtime.id}>{runtime.version} · {runtime.name}</option>)}
        </select></label>
        <label>Router profile<select value={preset.router_id} onChange={event => {
          update('router_id', event.target.value); setCatalog(undefined); setToken('');
        }}><option value="">Select a Router</option>{routers.map(router => <option key={router.id} value={router.id}>{router.name}</option>)}</select></label>
      </div>
      {onLaunch && <PathField label="Working directory" value={cwd} onChange={setCwd} directory task={task} />}
      <PathField label="models.yml template" value={preset.models_template} onChange={value => update('models_template', value)} task={task} />
      <PathField label="config.yml template" value={preset.config_template} onChange={value => update('config_template', value)} task={task} />
      <small>Templates are optional. Deck writes generated YAML into a separate folder for each session.</small>
      <div className="two">
        <label>Provider ID<input value={preset.provider} onChange={event => update('provider', event.target.value)} /></label>
        <label>Router protocol<select value={protocol} onChange={event => { update('anthropic', event.target.value === 'anthropic'); setCatalog(undefined); }}>
          <option value="openai">OpenAI Chat Completions</option><option value="anthropic">Anthropic Messages</option>
        </select></label>
        <label>Fallback context tokens<input type="number" min="0" max="4294967295" value={preset.context} onChange={event => update('context', +event.target.value)} /></label>
        <label>Fallback output tokens<input type="number" min="0" max="4294967295" value={preset.max_output} onChange={event => update('max_output', +event.target.value)} /></label>
      </div>
      <small>Fallback limits apply when the Router and template omit a value.</small>
      {profile?.authentication === 'bearer' && <label>Temporary YAML Router credential<input type="password" autoComplete="off" value={token} onChange={event => { setToken(event.target.value); setCatalog(undefined); }} /></label>}
      <div className="row">
        <button type="button" disabled={!protocolAvailable || (profile?.authentication === 'bearer' && !token.trim())}
          onClick={() => void task(async () => {
            if (!profile) return;
            setCatalog(undefined);
            const result = await api.listModels(profile, token, protocol);
            if (!result.complete) throw new Error(result.message);
            setCatalog(result);
          })}>Refresh Router models</button>
        {catalog && <span role="status">{catalog.models.length} models</span>}
      </div>
      {catalog && <>
        <label>YAML default model<select value={preset.default_model} onChange={event => update('default_model', event.target.value)}>
          <option value="">Select a served model</option>
          {preset.default_model && !catalog.models.includes(preset.default_model) && <option value={preset.default_model} disabled>{preset.default_model} (unavailable)</option>}
          {catalog.models.map(id => <option key={id}>{id}</option>)}</select></label>
        <details><summary>Included models {preset.models.length ? `(${preset.models.length} selected)` : '(all served)'}</summary>
          <button type="button" onClick={() => update('models', [])}>Include all</button>
          <div className="yaml-models">
            {[...catalog.models, ...missing].map(id => <label className="check" key={id}>
              <input type="checkbox" checked={preset.models.includes(id)} onChange={event => update('models',
                event.target.checked ? [...preset.models, id] : preset.models.filter(model => model !== id))} />{id}{missing.includes(id) && ' (unavailable)'}
            </label>)}
          </div>
        </details>
      </>}
      <label>Conversation storage<select value={preset.location} onChange={event => update('location', event.target.value as GjcYamlPreset['location'])}>
        <option value="AppData">Deck app data</option><option value="Custom">Custom root per session</option><option value="Workspace">Working directory per session</option>
      </select></label>
      {preset.location === 'Custom' && <PathField label="Existing history root" value={preset.history_root} onChange={value => update('history_root', value)} directory task={task} />}
      {preset.location === 'Workspace' && <small>Uses .gjc/deck-sessions in the workspace. Check Git exclusions before storing conversations there.</small>}
      <div className="row">
        <button type="button" disabled={!preset.name.trim()} onClick={() => void task(async () => {
          await api.saveYamlPreset(preset);
          setSaved(await api.yamlPresets()); setMessage('YAML preset saved');
        })}>Save YAML preset</button>
        {onLaunch && <button className="accent" disabled={!ready}>Start YAML session</button>}
      </div>
    </fieldset>
  </form>;
}

function PathField({ label, value, onChange, directory, task }: {
  label: string; value: string; onChange: (value: string) => void; directory?: boolean;
  task: (work: () => Promise<void>) => Promise<void>;
}) {
  return <div className="row end">
    <label className="grow">{label}<input value={value} onChange={event => onChange(event.target.value)} /></label>
    <button type="button" onClick={() => void task(async () => {
      const path = await (directory ? browse(true) : browseYaml());
      if (path) onChange(path);
    })}>Browse</button>
  </div>;
}
