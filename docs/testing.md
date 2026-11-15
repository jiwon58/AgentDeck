# Testing

```powershell
npm ci
npm run build
cargo fmt --all -- --check
cargo clippy --locked --all-targets --features custom-protocol,test-fixture -- -D warnings
cargo test --locked --features custom-protocol,test-fixture
```

The offline test fixture is `mock-agent.exe` in the Cargo build directory. It reports a version on stderr, prints its working directory with ANSI color, echoes input, streams output for `stream 1500`, and returns a selected exit code for `exit 7`. Test traffic is limited to local mock HTTP listeners on OS-assigned ports. Build-time dependency downloads are separate.

To automate the packaged WebView smoke test on a dedicated test desktop, use `node tests/integration/windows-smoke.mjs <agent-deck.exe> <mock-agent.exe>`. It uses WebView2’s localhost debugging endpoint for that child process, a temporary WebView profile, a system-only PATH and Chromium resolver rules that reject external hostnames. It writes temporary runtime, router and session records to the regular application database and attempts to remove those records at the end. The application may retain the test workspace as the last-used path. Optionally set `DECK_GJC_0102` and `DECK_GJC_0200` to approved official binaries to exercise each real TUI without inference.

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

