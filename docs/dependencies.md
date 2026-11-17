# Direct dependency review

Versions are recorded in Cargo.lock and package-lock.json. Cargo and npm install the development dependencies on the build machine.

| Dependency | Reason | License |
| --- | --- | --- |
| Tauri and dialog plugin | Desktop runtime, folder selection and native confirmations | MIT / Apache-2.0 |
| React and React DOM | UI and rendering | MIT |
| xterm.js, Fit and Search addons | ANSI, terminal layout and in-memory terminal output search | MIT |
| Vite, TypeScript, React type declarations | Build and type checking | MIT / Apache-2.0 |
| portable-pty | Cross-platform API for ConPTY and Unix PTY | MIT |
| rusqlite | Local SQLite, with bundled SQLite | MIT / public domain SQLite |
| reqwest | Bounded direct HTTP(S) router probes through rustls | MIT / Apache-2.0 |
| serde and serde_json | Typed IPC and stored records | MIT / Apache-2.0 |
| uuid | Independent installation, router and session IDs | MIT / Apache-2.0 |
| sha2 | Executable identity verification | MIT / Apache-2.0 |
| dunce | Windows-compatible canonical paths | MIT / Apache-2.0 |
| windows-sys | Job Object calls with the official Win32 bindings | MIT / Apache-2.0 |
| Tokio | Blocking-work dispatch | MIT |
| wait-timeout | Version-probe timeout | MIT / Apache-2.0 |
| tempfile | Isolated integration fixtures and per-session Team registry | MIT / Apache-2.0 |
| base64 | Preserves the native worker prompt through psmux's respawn arguments | MIT / Apache-2.0 |
| psmux 3.4.0 | Optional Windows-native pane multiplexer for GJC Team | MIT |
| eframe / egui | Windows native UI and local rendering | MIT / Apache-2.0 |
| alacritty_terminal | ANSI/VT parsing, terminal grid and scrollback | Apache-2.0 |
| serde_yaml_ng | Structured GJC template parsing and serialization | MIT |
| dirs | Windows app data paths | MIT / Apache-2.0 |
| rfd | Windows native file dialogs | MIT |

These are upstream libraries and official platform bindings. Review the locked dependency tree and upstream notices when preparing an enterprise release. The icon and UI assets ship locally. The application uses Windows system fonts and ships no downloaded font.

psmux is maintained separately and ships only in explicit Team-enabled portable packages. The package contains the publisher's unmodified x64 executable and its license. Deck's small compatibility helper forwards GJC commands to that executable within the session registry. [psmux 3.4.0 source and license](https://github.com/psmux/psmux/tree/v3.4.0)

