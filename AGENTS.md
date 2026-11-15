# Agent Deck implementation rules

- Keep Rust registry, adapters, storage, router and terminal concerns separate from frontend rendering.
- Require explicit executable approval, absolute paths and digest verification. Pass arguments separately.
- Persist metadata only. Keep terminal output and launch credentials in memory.
- Keep session snapshots immutable. A restarted process gets a new snapshot.
- Use documented adapter settings and explain version-specific compatibility gaps.
- Maintain local-only assets and never introduce implicit downloader, telemetry, or shell capabilities.

