export type Agent = 'gajae' | 'claude' | 'hermes' | 'generic';
export const agents: Record<Agent, string> = { gajae: 'Gajae-Code', claude: 'Claude Code', hermes: 'Hermes Agent', generic: 'Generic CLI' };
export const agentKeys = Object.keys(agents) as Agent[];
export interface Candidate { path: string; sha256: string; architecture: string; launch_type: string }
export interface Runtime {
  id: string; agent: Agent; name: string; version: string; executable: Candidate;
  arguments: string[]; root: string; cwd_override: string | null; dependencies: string;
  router_environment: Record<string, string>;
  trusted: boolean; verified: boolean; last_verified_at: number | null; created_at: number;
}
export interface Router {
  id: string; name: string; scheme: string; host: string; port: number;
  openai_path: string | null; anthropic_path: string | null; model: string;
  authentication: 'none' | 'bearer'; enabled: boolean; allow_remote: boolean;
  created_at: number; updated_at: number;
}
export type WorkStatus = 'in_progress' | 'completed' | 'waiting_for_reply';
export interface Session {
  id: string; name: string; runtime_id: string; agent: Agent; version: string;
  executable: Candidate; cwd: string; router_id: string | null; endpoint: string | null;
  router_revision: number | null; model: string; created_at: number;
  resume_source_id: string | null;
  gjc?: GjcHistory | null;
  team: boolean;
  external_history?: GjcHistorySource | null;
  native_gjc?: { agent_directory: string; session_directory: string } | null;
}
export interface Preferences { font_size: number; scrollback: number; last_runtime: string; last_cwd: string }
export interface SessionLayout { grouped: boolean; pinned: string[] }
export interface Overview {
  app_version: string;
  team_available: boolean;
  roots: { id: string; agent: Agent; path: string }[];
  runtimes: Runtime[]; routers: Router[]; sessions: Session[]; preferences: Preferences; data_directory: string; log_file: string | null;
  gjc_presets: GjcPreset[];
  session_layout: SessionLayout;
}
export interface Launch {
  id: string; name: string; runtime_id: string; cwd: string; router_id: string | null; model: string;
  extra: string[]; env: Record<string, string>; token: string; manual_router: boolean;
  manual_protocol?: 'openai' | 'anthropic'; cols: number; rows: number;
  resume_from?: string;
  team?: boolean;
  yaml?: { preset: GjcYamlPreset; catalog: string[] };
  external_history?: GjcHistorySource;
  gjc?: { preset_id: string; preview_fingerprint: string; model?: string; stream_timeout?: string;
    history_mode?: GjcHistoryMode; history_path?: string };
}
export interface GjcYamlPreset {
  name: string; runtime_id: string; router_id: string; anthropic: boolean;
  models_template: string; config_template: string; provider: string; default_model: string;
  models: string[]; context: number; max_output: number; location: 'AppData' | 'Custom' | 'Workspace'; history_root: string;
}
export type GjcHistoryMode = 'default' | 'existing_workspace' | 'custom';
export interface GjcHistorySource { path: string; agent_directory: string; session_id: string }
export interface GjcHistoryEntry { source: GjcHistorySource; cwd: string; title: string; modified_at: number }
export interface GjcHistory {
  preset_id: string; preset_revision: number; agent_directory: string | null; session_directory: string | null;
  history_mode: GjcHistoryMode; models_fingerprint: string | null;
  history_disabled: boolean;
}
interface GjcToggle { enabled: boolean; value: string }
export interface GjcPreset {
  id: string; name: string; description: string; enabled: boolean; auto_config_enabled: boolean;
  revision: number; created_at: number; updated_at: number; runtime_id: string; router_id: string | null;
  protocol: ModelProtocol; provider_id: string; model: string; context_tokens: number; max_output_tokens: number;
  directory_mode: 'default' | 'managed' | 'custom'; agent_path: string; history_mode: GjcHistoryMode; history_path: string;
  stream_timeout: GjcToggle; ui_language: GjcToggle; model_preset_registry: GjcToggle;
  variables: (GjcToggle & { key: string })[];
}
export interface GjcPreview {
  fingerprint: string;
  team?: { executable: string; sha256: string; bridge: string } | null;
  plan: {
    history: GjcHistory; runtime_id: string; executable: string; executable_sha256: string; version: string;
    cwd: string; endpoint: string | null; provider_id: string; model: string; argv: string[];
    env: Record<string, string>; models_target: string | null; warnings: string[]; requires_token: boolean;
    protocol: ModelProtocol;
    managed_startup_config: boolean;
  };
}
export type TerminalEvent = { session_id: string; sequence_number: number } & (
  { event_type: 'terminal_output'; payload: number[] } |
  { event_type: 'terminal_exited'; payload: { status: string; code: number | null } } |
  { event_type: 'terminal_error'; payload: string } |
  { event_type: 'terminal_started' | 'terminal_resized'; payload: unknown }
);
export interface Probe { tcp: string; http: number | null; result: string; models: string[] }
export type ModelProtocol = 'openai' | 'anthropic';
export interface ModelCatalog { models: string[]; complete: boolean; http: number | null; message: string }
export interface ModelTest { responded: boolean; http: number | null; elapsed_ms: number; message: string }

