# Agent Deck Native

Extract the native Windows x64 portable ZIP and launch `AgentDeck.exe`. The build uses egui and the Windows graphics stack through eframe. It uses local Windows fonts for Hangul fallback and requires a Windows display driver with OpenGL support. WebView2, Node.js, Rust, WSL and tmux are unnecessary on the target. Verify the SHA-256 before transfer. Both editions share the per-user SQLite database, including runtimes, Router profiles, saved sessions and Router YAML presets.

## Start a GJC session

1. In **Settings**, inspect the GJC executable, review its SHA-256 and explicitly trust it. If necessary, add a Router profile with its actual URL components and protocol.
2. In **New session**, select a verified GJC 0.10.2 or 0.20.0 runtime and enter the working directory. Enable **Generate GJC YAML from a Router preset**.
3. Enter absolute paths to the optional YAML templates. Select the provider ID you want to update, refresh the Router model list and choose a default model. Select a subset or include all served models. Save the preset by name to reuse it.
4. Supply verified fallback token limits for models that lack a value in the Router response or template. Enter a bearer credential if required. Select where GJC should store the conversation and start the session.

Deck fetches the catalog again before a new launch and asks for a fresh selection if the IDs changed. It uses documented Router metadata when available, then template values, then the entered fallback values. It reads vLLM's `max_model_len` on OpenAI-compatible listings and Claude's `max_input_tokens` and `max_tokens` on Anthropic-compatible listings. For OpenAI-compatible listings that only return IDs, the template and preset provide the limits. Listing APIs do not establish chat or tool-call compatibility. [vLLM model listing implementation](https://docs.vllm.ai/en/latest/api/vllm/entrypoints/openai/models/serving/) · [Claude Models API](https://platform.claude.com/docs/en/api/models/list)

Each new session gets its own generated `models.yml` and `config.yml`. Deck updates the selected provider, default model role and startup update setting. It preserves the other parsed YAML fields and leaves the source files untouched. The generated YAML points to an environment variable for the bearer credential. Known inline credential fields in template files are rejected. Review templates for any private values stored under custom keys.

## Resume conversations

New conversations can be stored in Deck app data, in a chosen root under a per-session ID, or in the workspace under `.gjc/deck-sessions/<id>`. Workspace storage may be visible to Git. A generated session retains its settings paths so that a later resume uses the same history and GJC configuration. Re-enter a temporary Router credential when needed.

Use **GJC history** to find a record in an existing agent directory or enter the full `.jsonl` path and inspect it. Close any independent GJC process that still has the conversation open. Deck uses GJC's native resume command with the original record and working directory. Older WebView records with no explicit history source can be opened through this page by pointing to their GJC agent directory or record.

## Terminal and verification

The native process uses the shared Rust ConPTY manager and feeds raw chunks directly into Alacritty's parser. It renders the active tab and updates state for hidden tabs, with bounded PTY buffers and 3,000 scrollback lines per session. Rendering is driven by input and output events. Local logs omit terminal text and credentials.

This edition covers direct PTY sessions, GJC Router presets, and opening GJC history. Team, WebView-only convenience tools and automatic installer UI are deferred. Native key, rendering and IME behavior can vary by OS and keyboard setup. Use the WebView edition if an unported feature is needed.
