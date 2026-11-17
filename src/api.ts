import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { Agent, Candidate, GjcPreset, GjcPreview, Launch, ModelCatalog, ModelProtocol, ModelTest, Overview, Probe, Router, Runtime, SessionLayout, TerminalEvent } from './types';
import type { GjcHistoryEntry, GjcYamlPreset } from './types';

export const api = {
  overview: () => invoke<Overview>('overview'),
  pinSession: (id: string, pinned: boolean) => invoke<SessionLayout>('set_session_pin', { id, pinned }),
  groupSessions: (grouped: boolean) => invoke<SessionLayout>('set_session_grouping', { grouped }),
  downloadAgent: (agent: 'claude' | 'hermes', directory: string) =>
    invoke<{path: string; sha256: string; version: string | null}>('download_agent', { agent, directory }),
  installHermes: (sha256: string) =>
    invoke<{candidates: Candidate[]; warnings: string[]}>('install_hermes', { sha256 }),
  savePreset: (preset: GjcPreset) => invoke<GjcPreset>('save_gjc_preset', { preset }),
  deletePreset: (id: string, revision: number) => invoke<void>('delete_gjc_preset', { id, revision }),
  previewPreset: (launch: Launch) => invoke<GjcPreview>('preview_gjc_preset', { launch }),
  yamlPresets: () => invoke<Record<string, GjcYamlPreset>>('yaml_presets'),
  saveYamlPreset: (preset: GjcYamlPreset) => invoke<void>('save_yaml_preset', { preset }),
  managedGjcHistory: (id: string) => invoke<{ entries: GjcHistoryEntry[]; limited: boolean }>('list_managed_gjc_history', { id }),
  gjcHistory: (agentDirectory: string) => invoke<{ entries: GjcHistoryEntry[]; limited: boolean }>('list_gjc_history', { agentDirectory }),
  inspectGjcHistory: (path: string, agentDirectory: string) => invoke<GjcHistoryEntry>('inspect_gjc_history', { path, agentDirectory }),
  scan: (agent: Agent, path: string) => invoke<{ candidates: Candidate[]; warnings: string[] }>('scan', { agent, path }),
  inspect: (path: string) => invoke<Candidate>('inspect_executable', { path }),
  register: (request: { agent: Agent; name: string; candidate: Candidate; arguments: string[]; probe_args: string[];
    root: string; cwd_override: string | null; dependencies: string; approved: boolean; probe: boolean;
    router_environment: Record<string, string> }) =>
    invoke<Runtime>('register_runtime', { request }),
  remove: (type: 'root' | 'runtime' | 'router' | 'session', id: string) => invoke<void>(`delete_${type}`, { id }),
  router: (profile: Router) => invoke<Router>('save_router', { profile }),
  probe: (profile: Router, token: string) => invoke<Probe>('probe_router', { profile, token }),
  listModels: (profile: Router, token: string, protocol: ModelProtocol) =>
    invoke<ModelCatalog>('list_router_models', { profile, token, protocol }),
  testModel: (profile: Router, token: string, protocol: ModelProtocol, model: string) =>
    invoke<ModelTest>('test_router_model', { profile, token, protocol, model }),
  preferences: (fontSize: number, scrollback: number) => invoke<void>('save_preferences', { fontSize, scrollback }),
  importSettings: (fontSize: number, scrollback: number, profiles: unknown[]) =>
    invoke<void>('import_settings', { fontSize, scrollback, profiles }),
  start: (launch: Launch) => invoke<import('./types').Session>('start_session', { launch }),
  close: (id: string) => invoke<void>('close_terminal', { id }),
  resolve: (id: string) => invoke<void>('resolve_session', { id }),
  drain: (id: string) => invoke<TerminalEvent[]>('terminal_events', { id }),
  write: (id: string, data: string) => invoke<void>('write_terminal', { id, data }),
  resize: (id: string, cols: number, rows: number) => invoke<void>('resize_terminal', { id, cols, rows }),
  quit: () => invoke<void>('quit'),
};
export const browse = (directory: boolean) => open({ directory, multiple: false,
  filters: directory ? undefined : [{ name: 'Windows executable', extensions: ['exe'] }] });
export const browseGjcHistory = () => open({ multiple: false, filters: [{ name: 'GJC session', extensions: ['jsonl'] }] });
export const browseYaml = () => open({ multiple: false, filters: [{ name: 'YAML template', extensions: ['yml', 'yaml'] }] });
export function lines(value: string) { return value.split(/\r?\n/).filter(line => line !== ''); }
export function environment(value: string) {
  return Object.fromEntries(lines(value).map(line => {
    const split = line.indexOf('=');
    if (split <= 0) throw new Error('Enter environment overrides as NAME=value, one per line.');
    return [line.slice(0, split).trim(), line.slice(split + 1)];
  }));
}
export const readable = (error: unknown) => error instanceof Error ? error.message : String(error);

