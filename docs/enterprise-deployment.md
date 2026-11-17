# Enterprise deployment

**Install Agents** is an explicit online operation. Claude Code uses Anthropic's release CDN and verifies the Windows binary against its release manifest. Hermes uses the Nous Research Windows bootstrap and displays the downloaded script's SHA-256 before execution. The official bootstrap can fetch additional dependencies and update the user PATH. Choose the install destination and confirm execution on a network where those services are permitted. On an isolated host, register already provisioned runtimes through **Agent Runtimes**. [Claude Code setup](https://code.claude.com/docs/en/setup) · [Hermes Windows install](https://hermes-agent.nousresearch.com/docs/user-guide/windows-native)

Build and verify the application on an internet-connected Windows x64 development machine. Copy the portable ZIP and its matching checksum through the organization’s normal transfer process. Use `Get-FileHash -Algorithm SHA256 -LiteralPath <archive>` and compare to the accompanying checksum. Extract the archive and start AgentDeck.exe. Executables in the MVP build are unsigned unless the organization signs them during packaging. Recompute hashes after signing.

Portable Agent Deck uses the installed Microsoft Edge WebView2 Runtime. Verify Evergreen WebView2 on the target. If missing, transfer the approved x64 Evergreen Standalone Installer separately, or use Agent Deck's NSIS offline installer if built. NSIS is configured with Tauri’s `offlineInstaller` mode. For a GNU/LLVM build, keep the packaged `WebView2Loader.dll` beside the executable. Copy any separately approved Agent packages and their prerequisites. [Tauri Windows installer](https://v2.tauri.app/distribute/windows-installer/) · [WebView2 distribution](https://developer.microsoft.com/microsoft-edge/webview2/).

Application data is in the Tauri per-user application data directory, normally `%APPDATA%\local.agentdeck.desktop`. Settings shows the resolved location. The portable ZIP contains no personal settings. Deleting the extracted folder or uninstalling leaves the per-user database. Exit Agent Deck before backing up or manually removing its data directory. Agent-owned settings and conversations are stored by each Agent.

For GJC Team, transfer the Team-enabled portable ZIP and keep the full `team` folder beside the application. Approve the packaged psmux and Deck helper under endpoint policy and make Git for Windows and PowerShell available. psmux uses an isolated temporary registry and authenticated loopback connections to manage panes. Deck verifies the packaged psmux digest at Team launch. Re-signing psmux changes its digest and requires a corresponding reviewed build. The NSIS configuration currently packages the base application. [psmux v3.4.0 release](https://github.com/psmux/psmux/releases/tag/v3.4.0)

## Local diagnostic logs

After reproducing an issue, collect `agent-deck.log` and, when present, `agent-deck.log.1`. Open **Settings → Application & Offline Settings** to copy the resolved path. On Windows the default is `%LOCALAPPDATA%\local.agentdeck.desktop\logs\agent-deck.log`. If the normal log directory is unavailable or startup fails before resolving it, check `%TEMP%\AgentDeck\logs\agent-deck.log`. For failures before the executable starts, such as a missing loader DLL, check the Windows Application event log.

Files are UTF-8 JSON Lines and rotate at 1 MiB. Each record has a UNIX timestamp in milliseconds, severity, event, run ID and process ID. Some events also have an internal UUID, status or numeric OS, SQLite, HTTP or process exit code. For example, `terminal.spawn_failed` marks process launch failure, and `router.probe_result` with code `401` indicates an authentication response. Include the local reproduction time and what you clicked when reporting an issue.

```powershell
Get-Content -LiteralPath "$env:LOCALAPPDATA\local.agentdeck.desktop\logs\agent-deck.log" -Encoding UTF8 |
    ConvertFrom-Json |
    Where-Object { $_.level -in 'WARN', 'ERROR' } |
    Select-Object -Last 30
```

Agent Deck does not write terminal input/output, command-line arguments, paths, router URLs, environment values, HTTP bodies or credentials to these diagnostic files. Agents may produce separate logs under their own configuration. Follow your organization’s handling rules for the files.

Deck has no telemetry or updater integration. Connection checks and model catalog refreshes contact the operator-selected router, bypass proxies, and decline redirects. A separate **Test model** action sends a short inference request to the selected model and can consume router quota. The local WebView may be subject to separate Microsoft runtime policies.

Agent processes and explicitly registered shells run with the operator’s permissions. Review launcher dependencies, Agent project permissions, provider fallback, native plugins, marketplace features, hooks and MCP tools. GJC’s launch-time version check can be disabled in its reviewed configuration using the documented `startup.checkUpdate` option. The v0.20.0 signed model preset registry can be disabled with `GJC_MODEL_PRESET_REGISTRY_DISABLED=1` in the session environment. Claude’s adapter passes its documented automatic updater and nonessential traffic settings. Review Claude managed settings, plugins and connectors with enterprise policy. Review Hermes provider, gateway, auxiliary models and installer/update ownership for the deployed installation. Use approved egress policy if the Agent must only reach the internal router. Agent Deck itself does not enforce Agent network traffic.

