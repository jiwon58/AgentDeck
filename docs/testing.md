# Testing

The session-tools smoke test searches and bookmarks retained output, verifies that read-only blocks typed and pasted input, compares two live terminals, checks reply navigation, and reopens the app to verify saved project grouping and pins.

Set `DECK_GJC_CHECK_STATUS=1` with both GJC executable paths to cover the 0.10.2 rendered indicators, the 0.20.0 native status protocol, and restoration of a real queued draft with Alt+Up. Run `node --experimental-strip-types tests/integration/terminal-keys.mjs` for the modifier-encoding checks. With internet access, `DECK_INSTALL_ONLY=1` and `DECK_PUBLISHER_DOWNLOADS=1` exercise the publisher download UI, inspect and run Claude Code's version command, validate Hermes bootstrap destination resolution, and reject a modified Hermes script.

The Windows smoke test also uses pointer events to click the terminal scrollbar track and drag its thumb. The GJC 0.20.0 resume test sends a follow-up after reopening Deck and checks that GJC appends it to the original conversation file and Deck keeps one history row.

Agent Deck 0.1.8 Windows verification adds GJC preset resolution and migration tests, a legacy scope vector suite, a v2 Volume GUID hash vector, and configuration lock, backup and unchanged-file tests. Build the WebView smoke binary with `--features custom-protocol,test-fixture` to isolate its database and logs in a temporary directory. The smoke covers preset creation, launch preview, blocked stale previews, concurrent launches with the same config, live config conflict, and mocked Claude/Hermes/Generic regressions. With `DECK_GJC_0102` and `DECK_GJC_0200` set to the verified release binaries, it also checks `--list-models`, OpenAI inference against loopback, native session directory parity and the built-in Resume picker. The 0.20.0 binary also sends an authenticated Anthropic Messages request to the local fixture. `DECK_GJC_PRESET_ONLY=1` runs the preset-focused subset.

```powershell
npm ci
npm run build
cargo fmt --all -- --check
cargo clippy --locked --all-targets --features custom-protocol,test-fixture -- -D warnings
cargo test --locked --features custom-protocol,test-fixture
```

The offline test fixture is `mock-agent.exe` in the Cargo build directory. It reports a version on stderr, prints its working directory with ANSI color, echoes input, streams output for `stream 1500`, and returns a selected exit code for `exit 7`. Test traffic is limited to local mock HTTP listeners on OS-assigned ports. Build-time dependency downloads are separate.

To automate the WebView smoke test, use `node tests/integration/windows-smoke.mjs <agent-deck.exe> <mock-agent.exe>` with a `test-fixture` build. It uses WebView2’s localhost debugging endpoint for the child process, a temporary WebView profile and database, a system-only PATH, and Chromium resolver rules that reject external hostnames. Optionally set `DECK_GJC_0102` and `DECK_GJC_0200` to approved official binaries to exercise each real TUI against the loopback fixture.

The diagnostic log checks cover Settings path display, persisted nonzero exit and router status codes, restore validation failure across a restart, and omission of the temporary token, session name, executable path and terminal output. A Rust unit test exercises log rotation and retention.

The shortcut smoke checks that the toolbar's **?** guide opens and closes with Escape. It exercises Ctrl+T, Ctrl+W, Ctrl+Tab and Ctrl+Shift+Tab on Deck controls and confirms those chords reach the focused terminal element without triggering Deck actions.

The session-list smoke covers automatic status changes from all three GJC host markers, including markers from a background tab, a neutral state before the first marker, and widening the terminal when the list is collapsed. The optional live GJC 0.20.0 test uses a temporary model profile and the loopback mock HTTP server to exercise an actual `ask` selector, its response-needed marker, and the final run-ended marker. It also restarts Deck, selects the original conversation in GJC's native picker, and checks that a follow-up request contains the restored conversation context. Set `DECK_GJC_0200` and `DECK_GJC_CHECK_STATUS=1` to run it.

The Windows x64 release passed all six Rust integration tests, strict Clippy, rustfmt, TypeScript and the Vite production build. The extracted portable executable passed the WebView smoke test, including three concurrent sessions, ANSI color, a router port update without mutating existing snapshots, persisted session metadata and a missing executable diagnostic. The NSIS build embedded the WebView2 offline runtime installer and loader, and omitted the mock executable. Full host network isolation and a clean install without WebView2 still require a disconnected Windows test machine.

Windows desktop smoke procedure:

1. Build the desktop app and fixture. Copy the fixture into three folders under a temporary test root, including a folder with Korean text and spaces. Name the copies `gjc.exe`.
2. Scan the root; verify that search does not run the fixtures. Explicitly approve each. Use separate version-probe arguments such as `--mock-version=0.10.2` and `--mock-version=0.20.0` for distinct labels.
3. Start three sessions, switch among them, enter distinct lines, resize the window, and verify colors, echo, streaming and exit code. Close an active tab after confirming and exit the app after confirming remaining sessions.
4. Restart the app and check saved sessions. Rename a trusted fixture and check the restart diagnostic. Restore the filename and change the file to verify digest invalidation.
5. On an isolated Windows PC or VM with WebView2 already installed, block internet access, extract the portable ZIP, launch and repeat the fixture smoke. The optional installer requires a second test on a clean isolated system without WebView2.

Record real Agent smoke tests separately from mock and router results. For each real version, record executable hash, supported launch flags, provider transport and tested environment controls.

The router models checks cover paginated Anthropic catalogs, exact model IDs on inference requests, and a running OpenAI-compatible router whose catalog changes between refreshes. The WebView smoke checks that a removed model disappears from the list and its inference test reports HTTP 404.
