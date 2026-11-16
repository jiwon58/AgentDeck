import { useState } from 'react';
import { api, browse, environment, lines, readable } from './api';
import type { GjcHistoryMode, GjcPreset, GjcPreview, Launch, Overview, Session } from './types';

const fresh = (): GjcPreset => ({
  id: '', name: '', description: '', enabled: true, auto_config_enabled: true, revision: 0,
  created_at: 0, updated_at: 0, runtime_id: '', router_id: null, protocol: 'openai',
  provider_id: 'deck-router', model: '', context_tokens: 32768, max_output_tokens: 4096,
  directory_mode: 'managed', agent_path: '', history_mode: 'default', history_path: '',
  stream_timeout: { enabled: true, value: '900000' }, ui_language: { enabled: false, value: 'en' },
  model_preset_registry: { enabled: true, value: '1' }, variables: [],
});

const modes = [
  ['default', 'GJC default'], ['existing_workspace', 'Existing workspace'], ['custom', 'Custom directory'],
] as const;

function HistoryMode({ value, onChange, override = false }: {
  value: string; onChange: (mode: string) => void; override?: boolean;
}) {
  return <label>Session history<select value={value} onChange={event => onChange(event.target.value)}>
    {override && <option value="">Use preset</option>}
    {modes.map(([key, label]) => <option key={key} value={key}>{label}</option>)}
  </select></label>;
}

function Directory({ label, value, change, readOnly = false, required = true, onError }: {
  label: string; value: string; change: (path: string) => void; onError: (message: string) => void;
  readOnly?: boolean; required?: boolean;
}) {
  return <div className="row end">
    <label className="grow">{label}<input value={value} required={required} readOnly={readOnly} onChange={e => change(e.target.value)} /></label>
    <button type="button" disabled={readOnly} onClick={() => void browse(true).then(path => { if (path) change(path); }).catch(e => onError(readable(e)))}>Browse</button>
  </div>;
}

function Preview({ preview }: { preview: GjcPreview }) {
  const plan = preview.plan;
  return <details open className="notice">
    <summary>Launch preview · validated{plan.warnings.length ? ' with warnings' : ''}</summary>
    {plan.warnings.map(warning => <p key={warning}>{warning}</p>)}
    <dl className="preset-preview">
      <dt>Executable</dt><dd>{plan.executable}</dd>
      <dt>Runtime</dt><dd>{plan.version} · {plan.runtime_id}</dd>
      <dt>SHA-256</dt><dd>{plan.executable_sha256}</dd>
      <dt>Working directory</dt><dd>{plan.cwd}</dd>
      <dt>Router</dt><dd>{plan.endpoint || 'GJC configuration'} · {plan.protocol}</dd>
      <dt>Provider / model</dt><dd>{plan.provider_id || 'Default'} / {plan.model || 'Default'}</dd>
      <dt>Arguments</dt><dd><code>{JSON.stringify(plan.argv)}</code></dd>
      <dt>Environment</dt><dd><code>{JSON.stringify(plan.env, null, 2)}</code></dd>
      <dt>Agent directory</dt><dd>{plan.history.agent_directory || 'GJC default'}</dd>
      <dt>Session history</dt><dd>{modes.find(([key]) => key === plan.history.history_mode)?.[1]} · {plan.history.session_directory || 'Resolved by GJC'}</dd>
      <dt>models.yml</dt><dd>{plan.models_target || 'GJC configuration'}{plan.history.models_fingerprint && <><br />{plan.history.models_fingerprint}</>}</dd>
      {plan.managed_startup_config && <><dt>Managed startup</dt><dd>Turn off update checks when creating the initial GJC config.</dd></>}
    </dl>
  </details>;
}

export function GjcPresetSettings({ data, refresh }: { data: Overview; refresh: () => Promise<void> }) {
  const [draft, setDraft] = useState<GjcPreset>();
  const [cwd, setCwd] = useState(data.preferences.last_cwd);
  const [preview, setPreview] = useState<GjcPreview>();
  const [validated, setValidated] = useState<{ id: string; revision: number; status: string }>();
  const [message, setMessage] = useState('');
  const [busy, setBusy] = useState(false);
  const runtimes = data.runtimes.filter(runtime => runtime.agent === 'gajae');
  const patch = <K extends keyof GjcPreset>(key: K, value: GjcPreset[K]) => {
    setDraft(current => current && { ...current, [key]: value });
    setPreview(undefined);
  };
  const task = async (work: () => Promise<void>) => {
    setMessage(''); setBusy(true);
    try { await work(); } catch (error) { setMessage(readable(error)); }
    finally { setBusy(false); }
  };
  const select = (preset: GjcPreset) => { setDraft(preset); setPreview(undefined); setMessage(''); };
  const references = draft ? data.sessions.filter(session => session.gjc?.preset_id === draft.id).length : 0;
  return <div className="stack">
    <div className="row"><h3 className="grow">GJC Presets</h3><button type="button" onClick={() => select(fresh())}>New preset</button></div>
    {message && <p className="notice danger" role="alert">{message}</p>}
    {data.gjc_presets.map(preset => {
      const runtime = runtimes.find(item => item.id === preset.runtime_id);
      const router = data.routers.find(item => item.id === preset.router_id);
      const status = !preset.enabled ? 'Disabled' : !runtime || (preset.router_id && !router?.enabled) ? 'Reference unavailable'
        : validated?.id === preset.id && validated.revision === preset.revision ? validated.status : 'Preview to validate';
      return <article className="notice" key={preset.id}>
        <div className="row"><strong className="grow">{preset.name}</strong><small>{status}</small></div>
        <p className="muted">{runtime?.version || 'Missing runtime'} · {router?.name || 'GJC settings'} · {preset.protocol} · {preset.provider_id || '—'} / {preset.model || '—'}<br />
          {preset.directory_mode} agent directory · {modes.find(([key]) => key === preset.history_mode)?.[1]} · {preset.auto_config_enabled ? 'Automatic' : 'Manual'}</p>
        <div className="row">
          <button type="button" disabled={busy} onClick={() => select(preset)}>Edit</button>
          <button type="button" disabled={busy} onClick={() => select({ ...preset, id: '', name: preset.name + ' (copy)', revision: 0 })}>Duplicate</button>
          <button type="button" disabled={busy} onClick={() => {
            const count = data.sessions.filter(session => session.gjc?.preset_id === preset.id).length;
            if (window.confirm('Delete this preset?' + (count ? ' ' + count + ' saved sessions reference it. Their files stay on disk.' : ''))) {
              void task(async () => { await api.deletePreset(preset.id, preset.revision); if (draft?.id === preset.id) setDraft(undefined); await refresh(); });
            }
          }}>Delete</button>
        </div>
      </article>;
    })}
    {draft && <form className="stack" onSubmit={event => {
      event.preventDefault();
      if (references && !window.confirm('Save a new revision? ' + references + ' saved sessions reference this preset. Active model configurations are protected until the terminals close.')) return;
      void task(async () => { select(await api.savePreset(draft)); await refresh(); });
    }}>
      <fieldset className="stack" disabled={busy}>
        <legend>General</legend>
        <label>Name<input required value={draft.name} onChange={e => patch('name', e.target.value)} /></label>
        <label>Description<input value={draft.description} onChange={e => patch('description', e.target.value)} /></label>
        <label className="check"><input type="checkbox" checked={draft.enabled} onChange={e => patch('enabled', e.target.checked)} />Enabled</label>
        <label className="check"><input type="checkbox" checked={draft.auto_config_enabled} onChange={e => patch('auto_config_enabled', e.target.checked)} />Automatically configure GJC</label>
      </fieldset>
      <fieldset className="stack" disabled={busy}>
        <legend>Runtime &amp; Router</legend>
        <label>Runtime<select required value={draft.runtime_id} onChange={e => patch('runtime_id', e.target.value)}>
          <option value="">Select an approved GJC runtime</option>
          {runtimes.map(runtime => <option key={runtime.id} value={runtime.id}>{runtime.version} · {runtime.name}</option>)}
        </select></label>
        <label>Router<select required={draft.auto_config_enabled} value={draft.router_id || ''} onChange={e => patch('router_id', e.target.value || null)}>
          <option value="">Use GJC settings</option>{data.routers.filter(router => router.enabled).map(router => <option key={router.id} value={router.id}>{router.name}</option>)}
        </select></label>
        <label>API protocol<select value={draft.protocol} onChange={e => patch('protocol', e.target.value as GjcPreset['protocol'])}>
          <option value="openai">OpenAI Chat Completions</option><option value="anthropic">Anthropic Messages</option>
        </select></label>
      </fieldset>
      <fieldset className="stack" disabled={busy || !draft.auto_config_enabled}>
        <legend>Model configuration</legend>
        <div className="two"><label>Provider ID<input required value={draft.provider_id} onChange={e => patch('provider_id', e.target.value)} /></label>
          <label>Model ID<input required value={draft.model} onChange={e => patch('model', e.target.value)} /></label></div>
        <div className="two"><label>Context tokens<input type="number" min={1} required value={draft.context_tokens} onChange={e => patch('context_tokens', Number(e.target.value))} /></label>
          <label>Maximum output tokens<input type="number" min={1} required value={draft.max_output_tokens} onChange={e => patch('max_output_tokens', Number(e.target.value))} /></label></div>
      </fieldset>
      <fieldset className="stack" disabled={busy || !draft.auto_config_enabled}>
        <legend>Agent directory</legend>
        <label>Mode<select value={draft.directory_mode} onChange={e => patch('directory_mode', e.target.value as GjcPreset['directory_mode'])}>
          <option value="default">GJC default</option><option value="managed">Managed per preset</option><option value="custom">Existing custom directory</option>
        </select></label>
        {draft.directory_mode === 'custom' && <Directory label="Agent directory" value={draft.agent_path} change={value => patch('agent_path', value)} onError={setMessage} />}
      </fieldset>
      <fieldset className="stack" disabled={busy || !draft.auto_config_enabled}>
        <legend>Session history</legend>
        <HistoryMode value={draft.history_mode} onChange={mode => patch('history_mode', mode as GjcHistoryMode)} />
        {draft.history_mode === 'custom' && <Directory label="History directory" value={draft.history_path} change={value => patch('history_path', value)} onError={setMessage} />}
      </fieldset>
      <fieldset className="stack" disabled={busy || !draft.auto_config_enabled}>
        <legend>Environment variables</legend>
        <small>Use these fields for non-secret values. Enter credentials when launching each session.</small>
        {([['stream_timeout', 'OpenAI stream idle timeout (ms)'], ['ui_language', 'Onboarding language'],
          ['model_preset_registry', 'Disable GJC model preset registry']] as const).map(([key, title]) =>
          <div className="row end" key={key}>
            <label className="check"><input type="checkbox" checked={draft[key].enabled} onChange={e => patch(key, { ...draft[key], enabled: e.target.checked })} />{title}</label>
            <input aria-label={title + ' value'} disabled={!draft[key].enabled} value={draft[key].value} onChange={e => patch(key, { ...draft[key], value: e.target.value })} />
          </div>)}
        {draft.variables.map((variable, index) => <div className="row" key={index}>
          <input aria-label="Enable variable" type="checkbox" checked={variable.enabled} onChange={e => patch('variables', draft.variables.map((row, i) => i === index ? { ...row, enabled: e.target.checked } : row))} />
          <input aria-label="Variable name" value={variable.key} onChange={e => patch('variables', draft.variables.map((row, i) => i === index ? { ...row, key: e.target.value } : row))} />
          <input aria-label="Variable value" disabled={!variable.enabled} value={variable.value} onChange={e => patch('variables', draft.variables.map((row, i) => i === index ? { ...row, value: e.target.value } : row))} />
          <button type="button" onClick={() => patch('variables', draft.variables.filter((_, i) => index !== i))}>Remove</button>
        </div>)}
        <button type="button" onClick={() => patch('variables', [...draft.variables, { enabled: true, key: '', value: '' }])}>Add variable</button>
      </fieldset>
      <div className="row"><button className="accent" disabled={busy}>Save preset</button><button type="button" onClick={() => setDraft(undefined)}>Close editor</button></div>
      <fieldset className="stack" disabled={busy || !draft.id}>
        <legend>Preview &amp; validation</legend>
        <small>Preview uses the last saved revision.</small>
        <Directory label="Working directory" value={cwd} change={value => { setCwd(value); setPreview(undefined); }} required={false} onError={setMessage} />
        <button type="button" disabled={!cwd} onClick={() => void task(async () => {
          setPreview(undefined);
          try {
            const result = await api.previewPreset(baseLaunch(draft, cwd));
            setPreview(result);
            setValidated({ id: draft.id, revision: draft.revision, status: result.plan.warnings.length ? 'Validated with warnings' : 'Validated' });
          } catch (error) {
            setValidated({ id: draft.id, revision: draft.revision, status: 'Validation failed' });
            throw error;
          }
        })}>Validate and preview</button>
        {preview && <Preview preview={preview} />}
      </fieldset>
    </form>}
  </div>;
}

function baseLaunch(preset: GjcPreset, cwd: string): Launch {
  return {
    id: crypto.randomUUID(), name: preset.name, runtime_id: preset.runtime_id, cwd,
    router_id: preset.router_id, model: '', extra: [], env: {}, token: '', manual_router: false,
    cols: 80, rows: 24, gjc: { preset_id: preset.id, preview_fingerprint: '' },
  };
}

export function GjcPresetLaunch({ data, previous, resume, runningPaths, onLaunch, onSettings }: {
  data: Overview; previous?: Session; resume?: boolean; runningPaths: string[];
  onLaunch: (launch: Launch) => void; onSettings: () => void;
}) {
  const presets = data.gjc_presets.filter(preset => preset.enabled);
  const [id, setId] = useState(previous?.gjc?.preset_id ?? presets[0]?.id ?? '');
  const [cwd, setCwd] = useState(previous?.cwd ?? data.preferences.last_cwd);
  const [name, setName] = useState(previous?.name ?? '');
  const [model, setModel] = useState('');
  const [timeout, setTimeout] = useState('');
  const [history, setHistory] = useState('');
  const [historyPath, setHistoryPath] = useState('');
  const [extra, setExtra] = useState('');
  const [env, setEnv] = useState('');
  const [token, setToken] = useState('');
  const [preview, setPreview] = useState<GjcPreview>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const preset = presets.find(item => item.id === id);
  const runtime = data.runtimes.find(item => item.id === preset?.runtime_id);
  const router = data.routers.find(item => item.id === preset?.router_id);
  const canPreview = preset && runtime && (!preset.router_id || router?.enabled);
  const plan = (preset: GjcPreset): Launch => ({
    ...baseLaunch(preset, cwd), token, name: name.trim() || preset.name,
    resume_from: resume ? previous?.id : undefined, extra: lines(extra), env: environment(env),
    gjc: { preset_id: preset.id, preview_fingerprint: preview?.fingerprint || '',
      model: model || undefined, stream_timeout: timeout || undefined,
      history_mode: !resume && history ? history as GjcHistoryMode : undefined,
      history_path: !resume && history === 'custom' ? historyPath : undefined },
  });
  return <form className="stack" onSubmit={event => {
    event.preventDefault();
    if (!preset || !canPreview) return;
    setError('');
    try {
      const launch = plan(preset);
      if (preview) { onLaunch(launch); return; }
      setBusy(true);
      void api.previewPreset({ ...launch, token: '' }).then(setPreview).catch(error => setError(readable(error))).finally(() => setBusy(false));
    } catch (error) { setError(readable(error)); }
  }}>
    {error && <p className="notice danger" role="alert">{error}</p>}
    {resume && <p className="notice">Choose the conversation to continue in the GJC picker. This terminal stays attached to the saved session. Enter your router credential again if required.</p>}
    <fieldset className="stack" disabled={busy}>
      <label>GJC preset<select required value={id} disabled={resume} onChange={e => {
        setId(e.target.value); setPreview(undefined); setToken(''); setModel(''); setTimeout(''); setHistory(''); setHistoryPath('');
      }}>
        <option value="">Select a preset</option>
        {presets.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
      </select></label>
      {!canPreview && <div className="notice row"><span className="grow">Select an enabled preset with an available runtime and router.</span><button type="button" onClick={onSettings}>Open Settings</button></div>}
      <Directory label="Working directory" value={cwd} change={value => { setCwd(value); setPreview(undefined); }} readOnly={resume} onError={setError} />
      {runningPaths.some(path => path.toLowerCase() === cwd.toLowerCase()) && <p className="notice">An active session uses this directory. Concurrent edits can conflict.</p>}
      {preset && runtime && <small>{runtime.version} · {router?.name || 'GJC settings'} · {preset.model || 'Default model'}</small>}
      <details><summary>Advanced session overrides</summary>
        {!resume && <label>Session name<input value={name} placeholder={preset?.name} onChange={e => setName(e.target.value)} /></label>}
        {preset?.auto_config_enabled && <>
          <label>Model ID<input value={model} onChange={e => { setModel(e.target.value); setPreview(undefined); }} /></label>
          <label>Stream idle timeout (ms)<input type="number" min={1} value={timeout} onChange={e => { setTimeout(e.target.value); setPreview(undefined); }} /></label>
          {!resume && <HistoryMode override value={history} onChange={mode => { setHistory(mode); setPreview(undefined); }} />}
          {!resume && history === 'custom' && <Directory label="History directory" value={historyPath} change={value => { setHistoryPath(value); setPreview(undefined); }} onError={setError} />}
        </>}
        <label>Non-secret arguments, one per line<textarea value={extra} onChange={e => { setExtra(e.target.value); setPreview(undefined); }} /></label>
        <label>Non-secret environment overrides, one NAME=value per line<textarea value={env} onChange={e => { setEnv(e.target.value); setPreview(undefined); }} /></label>
      </details>
      {preview && <Preview preview={preview} />}
      {preview?.plan.requires_token && <label>Router credential<input type="password" autoComplete="off" value={token} onChange={e => setToken(e.target.value)} required /></label>}
      <div className="row">{preview && <button type="button" onClick={() => setPreview(undefined)}>Refresh preview</button>}
        <button className="accent" disabled={!canPreview}>{preview ? (resume ? 'Open conversation picker' : 'Start session') : 'Preview launch'}</button>
      </div>
    </fieldset>
  </form>;
}
