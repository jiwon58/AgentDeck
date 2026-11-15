# Architecture

A single Tauri window hosts React and xterm.js. The Rust backend owns discovery, trust, SQLite and live PTYs. SQLite stores settings and immutable launch snapshots. The in-memory terminal manager owns live process handles. Restart creates a new process using the current profile rather than reconnecting to a previous PTY.

Implementation order: establish the shell and migration; verify the mock PTY; connect discovery and trust; add adapters and router checks; verify concurrent sessions; package the Windows release.

Terminal delivery uses a bounded per-session queue, sequenced Tauri notifications and explicit draining. The frontend subscribes before launching and drains only after the previous xterm write completes. Queue overflow is reported and the parser reset. No terminal transcript is persisted.

Only absolute native executables are launched. Registry scanning does not spawn. The version probe and launch recheck the approved executable digest. Manually supplied interpreter arguments and dependent scripts are part of the command the operator approves. Their contents are outside the executable digest.

