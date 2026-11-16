# GJC presets and session history

## Set up a preset

1. Register an approved GJC runtime and an enabled Local LLM Router profile.
2. In **Settings → GJC Presets**, create a preset. Pick a protocol exposed by the router, a provider ID and a model ID. Use **Local LLM Router → Refresh models** to check the IDs currently served.
3. Select the agent directory and history mode and save the preset. Use **Validate and preview** on a saved revision to see its exact executable, arguments, target paths and non-secret environment.
4. In **New Session → GJC preset**, select the preset and working directory. Review the preview and start. Enter authentication only if required. Advanced fields can override the model, OpenAI idle timeout and history policy for this run.
5. Select a saved session and choose **Resume Gajae-Code conversation** to open the native picker with the saved workspace and history location. Select the existing conversation to continue. Deck attaches the terminal to the same saved row and GJC appends to the selected conversation. Resume requires the original approved runtime and preset. Sessions with history disabled omit this action.

**Managed** stores GJC data under the user's Deck App Data at `gjc/presets/<id>/agent`. Renaming a preset keeps this location. Duplicating it allocates a new ID and location. On initial setup, Deck seeds `config.yml` with `startup.checkUpdate: false` if the file is absent. **Custom** requires an existing absolute directory. **GJC default** follows the Windows home or inherited GJC agent directory. Use a separate managed directory when an existing `models.yml` contains user settings. In manual mode, configure the provider and history with GJC or supply explicitly reviewed arguments.

## Execution and storage

The existing Runtime Registry and Router Manager supply the approved runtime, digest and router revision. A separate SQLite migration stores editable `GjcPreset` templates and their revisions. Filesystem inspection resolves the working directory, agent directory and eligible history directory. The pure resolver takes those inputs and builds a `ResolvedGjcLaunch`. Preview and Start use that same function. Start reloads the current references, verifies the digest and rejects a stale fingerprint before writing files or starting a process. The immutable session snapshot records the preset revision, router revision, history and model configuration fingerprint. Older session records remain readable.

Environment precedence is GJC defaults, preset settings, allowed session overrides and explicitly entered per-launch variables. An explicit agent directory wins over the preset directory mode. The OpenAI idle timeout uses `GJC_OPENAI_STREAM_IDLE_TIMEOUT_MS`; the setting has no Anthropic effect. GJC 0.20.0 supports the optional registry disable toggle and first-use language hint (`en`, `ko`, `zh`, `ja`). The language hint applies during onboarding. Unknown dedicated environment settings are skipped with a preview warning. Put credentials in the launch password field. Deck injects a bearer credential through an environment reference in `models.yml` and omits it from the Preview, database, terminal launch diagnostics and backups.

The writer lock serializes preparation through process startup, including across Deck instances. Each running preset process holds a shared lease on its agent directory. An identical document is reused without changing its modification time. A different document requires an exclusive lease. Close the other sessions or duplicate the preset if the model or provider configuration conflicts. Deck backs up its previous document and validates the new temporary file before atomic replacement. A content signature identifies Deck-generated files, and edited or user-created documents are preserved. Deck leaves conversations and backups on disk when a preset is deleted.

## Version compatibility

| Release | GJC session root on Windows | Scope | Existing Workspace behavior |
| --- | --- | --- | --- |
| Below 0.10.2 | Depends on build | Unknown | GJC default; no automatic override |
| 0.10.2 | `<agent>/sessions` | Home/temp-relative or encoded absolute path | Use the verified legacy scope and insert `--session-dir` before Resume |
| 0.10.3–0.11.1 | `<agent>/sessions` in reviewed 0.11.0/0.11.1 source | Legacy in reviewed tags | GJC default; builds in this range have no binary golden test |
| 0.11.2 and later | `<agent>/sessions` in reviewed 0.11.2 and 0.20.0 source | v2 directory and managed binding | 0.20.0 computes the preview path and uses GJC native managed lookup. Other builds use the GJC default pending binary verification. |

Windows v2 identity comes from `GetFinalPathNameByHandleW` with `VOLUME_NAME_GUID`. The SHA-256 input is the GJC scope prefix, platform prefix and exact UTF-8 native identity. Lowercase, unpadded Base32 encodes the full digest. If native identity lookup fails, Deck uses GJC's default history policy and displays a warning. GJC's v2 native lookup performs binding validation; an explicit `--session-dir` would bypass that managed path. **Custom** history uses the explicit flag and requires an existing directory. A user-supplied history argument is respected, duplicates are rejected, and an explicit `--no-session` suppresses injection.

The Windows smoke test runs each supported GJC binary with its native default policy and checks that the resulting conversation file appears in the directory independently resolved by Deck. The 0.20.0 test also checks for the native binding file. Rust tests cover alternate Windows path spellings and a synthetic Volume GUID with a separately calculated SHA-256/Base32 result.

Automated model configuration is enabled for the reviewed GJC 0.10.2 and 0.20.0 runtime contracts. Use a manual preset or the existing Agent runtime form for other builds. Older Deck builds do not open the migrated database. Back up the database before upgrading if rollback is needed.

For in-house troubleshooting, find the diagnostic file in **Settings → Application & Offline Settings**. Look for `gjc.preview_failed`, `gjc.preview_stale`, `gjc.prepare_failed` or `gjc.models_live_conflict` when a launch is blocked. The dialog shows the user-facing error, and the log records the event and relevant ID without command arguments, tokens or terminal output.

Primary source: [0.10.2 models](https://github.com/Yeachan-Heo/gajae-code/blob/v0.10.2/docs/models.md) · [0.10.2 environment](https://github.com/Yeachan-Heo/gajae-code/blob/v0.10.2/docs/environment-variables.md) · [0.10.2 legacy storage](https://github.com/Yeachan-Heo/gajae-code/blob/v0.10.2/packages/coding-agent/src/session/session-manager.ts) · [0.11.2 managed scope](https://github.com/Yeachan-Heo/gajae-code/blob/v0.11.2/packages/coding-agent/src/session/internal/managed-session-scope.ts) · [0.20.0 managed scope](https://github.com/Yeachan-Heo/gajae-code/blob/v0.20.0/packages/coding-agent/src/session/internal/managed-session-scope.ts) · [0.20.0 native path identity](https://github.com/Yeachan-Heo/gajae-code/blob/v0.20.0/crates/pi-natives/src/path_identity.rs)
