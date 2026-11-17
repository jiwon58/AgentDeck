import type { Runtime } from './types';

export function teamRuntime(runtime?: Runtime): boolean {
  return !!runtime && runtime.agent === 'gajae' && runtime.version === '0.10.2' &&
    runtime.executable.architecture === 'x64' && runtime.verified && runtime.trusted;
}

export function NativeTeam({ available, checked, onChange }: {
  available: boolean; checked: boolean; onChange: (value: boolean) => void;
}) {
  return <div className="stack notice">
    <label className="check"><input type="checkbox" disabled={!available} checked={available && checked}
      onChange={event => onChange(event.target.checked)} />GJC Team (native Windows)</label>
    <small>{available
      ? 'Open an isolated pane session. Use $team inside GJC from a Git checkout.'
      : 'Available in the Windows portable package that includes psmux.'}</small>
  </div>;
}
