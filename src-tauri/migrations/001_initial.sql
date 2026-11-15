CREATE TABLE agent_roots (id TEXT PRIMARY KEY, document TEXT NOT NULL);
CREATE TABLE runtime_installations (id TEXT PRIMARY KEY, document TEXT NOT NULL);
CREATE TABLE router_profiles (id TEXT PRIMARY KEY, document TEXT NOT NULL);
CREATE TABLE session_definitions (id TEXT PRIMARY KEY, document TEXT NOT NULL);
CREATE TABLE application_settings (id TEXT PRIMARY KEY, document TEXT NOT NULL);
PRAGMA user_version = 1;

