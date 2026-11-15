# Agent Deck

Agent Deck is a Windows x64 desktop terminal manager for Gajae-Code, Claude Code, Hermes Agent and custom CLI installations. It uses a local React interface, Rust and Tauri 2, xterm.js, Windows ConPTY via portable-pty, and bundled SQLite.

## Development

Install Windows MSVC C++ Build Tools with the Windows SDK, a current stable Rust toolchain, Node.js 22.12 or newer, and Microsoft Edge WebView2 Runtime. Run from this directory.

```powershell
npm ci
npm run tauri -- dev
```

```powershell
npm run build
cargo fmt --all -- --check
cargo clippy --locked --all-targets --features custom-protocol,test-fixture -- -D warnings
cargo test --locked --features custom-protocol,test-fixture
```

## Release and transfer

```powershell
npm ci
npm run tauri -- build --no-bundle -- --locked
.\packaging\New-Portable.ps1
```

Transfer the generated ZIP and matching SHA-256 file from `artifacts`. Check the archive hash, extract it, and start `AgentDeck.exe`. The target machine needs WebView2 and the Agent installations selected by the operator. Rust, Cargo and Node.js are build-time dependencies.

To build the optional Windows offline NSIS installer on the internet-connected build machine, use `npm run tauri -- build --bundles nsis -- --locked`. The configuration requests the official WebView2 offline installer.

The supplied Windows build was produced with Rust 1.100.0 `x86_64-pc-windows-gnullvm` and llvm-mingw because MSVC was unavailable on the build host. For that toolchain, use `cargo build --locked --release --features custom-protocol --bin agent-deck` after `npm run build`. Run the portable script as above. To bundle that binary with NSIS, use `npm run tauri -- bundle --bundles nsis --config packaging/tauri.gnullvm.json --no-binary-patching --ci`. This overlay places the dynamic WebView2 loader beside the installed application.

## Project layout

- `src/` holds React screens, the typed IPC client and the xterm.js integration.
- `src-tauri/src/` holds the Windows app, adapters, registry, router, SQLite storage and terminal manager.
- `src-tauri/migrations/` and `src-tauri/tests/` hold the schema and integration tests.
- `packaging/` holds the portable packaging script.

See [usage](docs/user-guide.md), [architecture](docs/architecture.md), [compatibility](docs/compatibility.md), [testing](docs/testing.md), [deployment](docs/enterprise-deployment.md), and [limitations](docs/limitations.md).

