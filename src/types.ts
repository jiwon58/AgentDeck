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
}
export interface Preferences { font_size: number; scrollback: number; last_runtime: string; last_cwd: string }
export interface Overview {
  app_version: string;
  roots: { id: string; agent: Agent; path: string }[];
  runtimes: Runtime[]; routers: Router[]; sessions: Session[]; preferences: Preferences; data_directory: string; log_file: string | null;
}
export interface Launch {
  id: string; name: string; runtime_id: string; cwd: string; router_id: string | null; model: string;
  extra: string[]; env: Record<string, string>; token: string; manual_router: boolean;
  manual_protocol: 'openai' | 'anthropic'; cols: number; rows: number;
  resume_from?: string;
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

