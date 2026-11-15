import { useState } from 'react';
import { api } from './api';
import type { ModelCatalog, ModelProtocol, ModelTest, Router } from './types';

export function RouterModels({ profile, token, task, onDefault }: {
  profile: Router;
  token: string;
  task: (work: () => Promise<void>) => Promise<void>;
  onDefault: (model: string) => void;
}) {
  const [protocol, setProtocol] = useState<ModelProtocol>(profile.openai_path !== null ? 'openai' : 'anthropic');
  const [catalog, setCatalog] = useState<ModelCatalog>();
  const [model, setModel] = useState(profile.model);
  const [result, setResult] = useState<ModelTest>();
  const [filter, setFilter] = useState('');
  const models = catalog?.models ?? [];
  const visibleModels = models.filter(id => id.toLowerCase().includes(filter.trim().toLowerCase()));
  const canRequest = !!profile.port && !!profile.name.trim();

  const refresh = () => task(async () => {
    setCatalog(undefined);
    setResult(undefined);
    const fresh = await api.listModels(profile, token, protocol);
    setCatalog(fresh);
    if (fresh.complete && !fresh.models.includes(model)) {
      setModel(fresh.models.includes(profile.model) ? profile.model : '');
    }
  });

  if (profile.openai_path === null && profile.anthropic_path === null) return null;
  return <section className="stack" aria-label="Router models">
    <h3>Router models</h3>
    <div className="row end">
      <label className="grow">Model API<select value={protocol} onChange={event => {
        setProtocol(event.target.value as ModelProtocol);
        setCatalog(undefined);
        setResult(undefined);
        setFilter('');
      }}>
        {profile.openai_path !== null && <option value="openai">OpenAI Chat Completions</option>}
        {profile.anthropic_path !== null && <option value="anthropic">Anthropic Messages</option>}
      </select></label>
      <button type="button" disabled={!canRequest} onClick={() => void refresh()}>Refresh models</button>
    </div>
    {catalog && <p className="notice" role="status">
      {catalog.message} · {models.length} {catalog.complete ? 'listed' : 'retrieved'}{catalog.http !== null && ` · HTTP ${catalog.http}`}
    </p>}
    {catalog?.complete && profile.model && !models.includes(profile.model) &&
      <p className="muted">Default model is absent from this API's latest list.</p>}
    {models.length > 0 && <>
      <label>Filter models<input type="search" value={filter} onChange={e => setFilter(e.target.value)} /></label>
      <label>Discovered models<select size={Math.min(8, Math.max(2, visibleModels.length))}
        value={visibleModels.includes(model) ? model : ''} onChange={e => { setModel(e.target.value); setResult(undefined); }}>
        {visibleModels.map(id => <option key={id} value={id}>{id}</option>)}
      </select></label>
      {!visibleModels.length && <p className="muted">No models match this filter.</p>}
    </>}
    <label>Model ID to test<input value={model} onChange={e => { setModel(e.target.value); setResult(undefined); }} /></label>
    <div className="row">
      <button type="button" disabled={!canRequest || !model.trim()} onClick={() => void task(async () => {
        setResult(undefined);
        setResult(await api.testModel(profile, token, protocol, model.trim()));
      })}>Test model</button>
      <button type="button" disabled={!model.trim() || model.trim() === profile.model} onClick={() => onDefault(model.trim())}>
        Use as default
      </button>
    </div>
    <small>Test model sends “Reply with OK.” to the selected API and may use model quota.</small>
    {result && <p className={`notice${result.responded ? '' : ' danger'}`} role="status">
      {result.message}{result.http !== null && ` · HTTP ${result.http}`} · {result.elapsed_ms} ms
    </p>}
  </section>;
}
