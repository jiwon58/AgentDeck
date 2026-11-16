# Gajae-Code session-status compatibility

Snapshot taken on 2026-11-15 from GitHub's paginated tags and releases lists. The repository exposed 178 Git tags and 156 releases. Of the tags, 168 were versioned stable or nightly tags and ten were working, backup, or review tags. The version inventory below includes releases and versioned source-only tags.

The audit examined the tagged host-status emitter and, where present, verified its three call sites in the interactive event controller and extension UI controller. “Native” means that source emits the three OSC 777 host status markers. “Opt-in” versions require the per-process GJC_HOST_STATUS=terax setting. A missing emitter means this host protocol was not found at the audited source path.

| Source versions | Result |
| --- | --- |
| v0.1.0 through v0.12.11 | No native emitter in the audit. For v0.10.2, Deck recognizes the activity, built-in question, and composer indicators in the live TUI frame. Other versions in this band display terminal process status. |
| v0.12.12 through v0.18.12, including the nightlies listed below | Native host markers emitted without the opt-in setting. |
| v0.19.0 through v0.20.0, including the listed v0.20.1 nightlies | Native host markers are opt-in. Deck sets the environment variable for GJC child processes. |

The markers report an active Agent run, a reply or approval selector that the Agent exposes through this protocol, or the end of its run. The finish marker does not establish success. The latest attention marker stays visible until the Agent reports another state or the terminal exits. Support in source does not establish delivery on every Windows ConPTY build, and individual binaries have not all been executed. Live Agent markers remain in memory. Previously saved manual marks are ignored.

Primary sources: [Version tags](https://github.com/Yeachan-Heo/gajae-code/tags), [Published releases](https://github.com/Yeachan-Heo/gajae-code/releases), [v0.12.12 emitter](https://github.com/Yeachan-Heo/gajae-code/blob/v0.12.12/packages/coding-agent/src/modes/utils/host-status.ts), [v0.19.0 emitter](https://github.com/Yeachan-Heo/gajae-code/blob/v0.19.0/packages/coding-agent/src/modes/utils/host-status.ts), [v0.20.0 event controller](https://github.com/Yeachan-Heo/gajae-code/blob/v0.20.0/packages/coding-agent/src/modes/controllers/event-controller.ts), [v0.20.0 input selector](https://github.com/Yeachan-Heo/gajae-code/blob/v0.20.0/packages/coding-agent/src/modes/controllers/extension-ui-controller.ts).

| Tag | Distribution | Host markers in tagged source |
| --- | --- | --- |
| v0.20.1-nightly.20261108030927.43590872198.gb4127889c4a8 | GitHub release | Native, opt-in |
| v0.20.1-nightly.20261108030927.43424588966.gb4127889c4a8 | GitHub release | Native, opt-in |
| v0.20.1-nightly.20261108030927.43222623203.gb4127889c4a8 | GitHub release | Native, opt-in |
| v0.20.1-nightly.20261108030927.43016219422.gb4127889c4a8 | GitHub release | Native, opt-in |
| v0.20.1-nightly.20261108030927.42818741338.gb4127889c4a8 | GitHub release | Native, opt-in |
| v0.20.0 | GitHub release | Native, opt-in |
| v0.19.4-nightly.20261103080408.42367091106.gd4ecb353a215 | GitHub release | Native, opt-in |
| v0.19.4-nightly.20261103080408.42216225023.gd4ecb353a215 | GitHub release | Native, opt-in |
| v0.19.4-nightly.20261103080408.42027983820.gd4ecb353a215 | GitHub release | Native, opt-in |
| v0.19.4-nightly.20261103080408.41632716544.gd4ecb353a215 | GitHub release | Native, opt-in |
| v0.19.3 | GitHub release | Native, opt-in |
| v0.19.3-nightly.20261030094347.41301593972.gf536b1010d4e | GitHub release | Native, opt-in |
| v0.19.3-nightly.20261030094347.41192844712.gf536b1010d4e | GitHub release | Native, opt-in |
| v0.19.2 | GitHub release | Native, opt-in |
| v0.19.2-nightly.20261028085024.40673953840.g86cbbabff393 | GitHub release | Native, opt-in |
| v0.19.1 | Source tag | Native, opt-in |
| v0.19.1-nightly.20261022091658.40484287217.g789b4b74f150 | GitHub release | Native, opt-in |
| v0.19.1-nightly.20261022091658.40080034760.g789b4b74f150 | GitHub release | Native, opt-in |
| v0.19.1-nightly.20261022091658.39755804883.g789b4b74f150 | GitHub release | Native, opt-in |
| v0.19.0 | GitHub release | Native, opt-in |
| v0.18.12 | GitHub release | Native |
| v0.18.11 | GitHub release | Native |
| v0.18.10 | GitHub release | Native |
| v0.18.9 | GitHub release | Native |
| v0.18.8 | GitHub release | Native |
| v0.18.7 | GitHub release | Native |
| v0.18.6 | GitHub release | Native |
| v0.18.5 | GitHub release | Native |
| v0.18.4 | Source tag | Native |
| v0.18.3 | Source tag | Native |
| v0.18.2 | Source tag | Native |
| v0.18.1 | GitHub release | Native |
| v0.18.0 | GitHub release | Native |
| v0.17.8-nightly.20260925162607.36311381126.g5c5231418930 | GitHub release | Native |
| v0.17.8-nightly.20260925162607.36232709894.g5c5231418930 | GitHub release | Native |
| v0.17.7 | GitHub release | Native |
| v0.17.7-nightly.20260924124649.36119772135.gbf9d3ec15717 | GitHub release | Native |
| v0.17.6 | GitHub release | Native |
| v0.17.5 | GitHub release | Native |
| v0.17.4 | GitHub release | Native |
| v0.17.3 | Source tag | Native |
| v0.17.2 | GitHub release | Native |
| v0.17.1 | GitHub release | Native |
| v0.17.0 | Source tag | Native |
| v0.16.8-nightly.20260913102120.34830784995.g9da99cdd708c | GitHub release | Native |
| v0.16.7 | GitHub release | Native |
| v0.16.7-nightly.20260907153739.34332369022.gf6b7c475d24f | GitHub release | Native |
| v0.16.7-nightly.20260907153739.34207192107.gf6b7c475d24f | GitHub release | Native |
| v0.16.7-nightly.20260907153739.34749835198.gf6b7c475d24f | GitHub release | Native |
| v0.16.6 | GitHub release | Native |
| v0.16.5 | Source tag | Native |
| v0.16.4 | GitHub release | Native |
| v0.16.3 | GitHub release | Native |
| v0.16.2 | Source tag | Native |
| v0.16.1 | GitHub release | Native |
| v0.16.0 | GitHub release | Native |
| v0.15.6 | GitHub release | Native |
| v0.15.5 | GitHub release | Native |
| v0.15.4 | GitHub release | Native |
| v0.15.3 | GitHub release | Native |
| v0.15.2 | GitHub release | Native |
| v0.15.1 | Source tag | Native |
| v0.15.1-nightly.20260823084028.32811019483.g28e375925cf5 | GitHub release | Native |
| v0.15.1-nightly.20260823084028.32692335596.g28e375925cf5 | GitHub release | Native |
| v0.15.1-nightly.20260822171644.32619118143.gfceb3c8da757 | GitHub release | Native |
| v0.15.0 | GitHub release | Native |
| v0.14.2 | GitHub release | Native |
| v0.14.2-nightly.20260818150120.32333814007.gd49c40a6c3c0 | GitHub release | Native |
| v0.14.2-nightly.20260818150120.32217566985.gd49c40a6c3c0 | GitHub release | Native |
| v0.14.1 | GitHub release | Native |
| v0.14.1-nightly.20260817042404.31996516416.g84d59968b9cb | GitHub release | Native |
| v0.14.0 | GitHub release | Native |
| v0.13.3 | GitHub release | Native |
| v0.13.2 | GitHub release | Native |
| v0.13.1 | GitHub release | Native |
| v0.13.0 | GitHub release | Native |
| v0.12.22-nightly.20260809150741.31359856973.gd89dc9da6ecf | GitHub release | Native |
| v0.12.21 | GitHub release | Native |
| v0.12.20 | GitHub release | Native |
| v0.12.19 | GitHub release | Native |
| v0.12.18 | GitHub release | Native |
| v0.12.17 | Source tag | Native |
| v0.12.16 | GitHub release | Native |
| v0.12.16-nightly.20260807082749.31161807807.ga878e808e4a6 | GitHub release | Native |
| v0.12.16-nightly.20260807070805.31156495595.g91c4eff67ce2 | GitHub release | Native |
| v0.12.15 | GitHub release | Native |
| v0.12.14 | GitHub release | Native |
| v0.12.13 | GitHub release | Native |
| v0.12.12 | GitHub release | Native |
| v0.12.11 | GitHub release | Emitter not found |
| v0.12.10 | GitHub release | Emitter not found |
| v0.12.9 | Source tag | Emitter not found |
| v0.12.8 | GitHub release | Emitter not found |
| v0.12.7 | GitHub release | Emitter not found |
| v0.12.6 | GitHub release | Emitter not found |
| v0.12.5 | GitHub release | Emitter not found |
| v0.12.4 | GitHub release | Emitter not found |
| v0.12.3 | Source tag | Emitter not found |
| v0.12.2 | GitHub release | Emitter not found |
| v0.12.1 | GitHub release | Emitter not found |
| v0.12.0 | GitHub release | Emitter not found |
| v0.11.11 | GitHub release | Emitter not found |
| v0.11.10 | GitHub release | Emitter not found |
| v0.11.9 | GitHub release | Emitter not found |
| v0.11.8 | GitHub release | Emitter not found |
| v0.11.7 | GitHub release | Emitter not found |
| v0.11.6 | GitHub release | Emitter not found |
| v0.11.5 | GitHub release | Emitter not found |
| v0.11.4 | GitHub release | Emitter not found |
| v0.11.3 | GitHub release | Emitter not found |
| v0.11.2 | GitHub release | Emitter not found |
| v0.11.1 | GitHub release | Emitter not found |
| v0.11.0 | GitHub release | Emitter not found |
| v0.10.2 | GitHub release | Emitter not found |
| v0.10.1 | GitHub release | Emitter not found |
| v0.10.0 | GitHub release | Emitter not found |
| v0.9.6 | GitHub release | Emitter not found |
| v0.9.5 | GitHub release | Emitter not found |
| v0.9.4 | GitHub release | Emitter not found |
| v0.9.3 | GitHub release | Emitter not found |
| v0.9.2 | GitHub release | Emitter not found |
| v0.9.1 | GitHub release | Emitter not found |
| v0.9.0 | GitHub release | Emitter not found |
| v0.8.2 | GitHub release | Emitter not found |
| v0.8.1 | GitHub release | Emitter not found |
| v0.8.0 | GitHub release | Emitter not found |
| v0.7.11 | GitHub release | Emitter not found |
| v0.7.10 | GitHub release | Emitter not found |
| v0.7.9 | GitHub release | Emitter not found |
| v0.7.8 | GitHub release | Emitter not found |
| v0.7.7 | GitHub release | Emitter not found |
| v0.7.6 | GitHub release | Emitter not found |
| v0.7.5 | GitHub release | Emitter not found |
| v0.7.4 | GitHub release | Emitter not found |
| v0.7.3 | GitHub release | Emitter not found |
| v0.7.2 | GitHub release | Emitter not found |
| v0.7.1 | GitHub release | Emitter not found |
| v0.7.0 | GitHub release | Emitter not found |
| v0.6.5 | GitHub release | Emitter not found |
| v0.6.4 | GitHub release | Emitter not found |
| v0.6.3 | GitHub release | Emitter not found |
| v0.6.2 | GitHub release | Emitter not found |
| v0.6.1 | GitHub release | Emitter not found |
| v0.6.0 | GitHub release | Emitter not found |
| v0.5.4 | GitHub release | Emitter not found |
| v0.5.3 | GitHub release | Emitter not found |
| v0.5.2 | GitHub release | Emitter not found |
| v0.5.1 | GitHub release | Emitter not found |
| v0.5.0 | GitHub release | Emitter not found |
| v0.4.5 | GitHub release | Emitter not found |
| v0.4.4 | GitHub release | Emitter not found |
| v0.4.3 | GitHub release | Emitter not found |
| v0.4.2 | GitHub release | Emitter not found |
| v0.4.1 | GitHub release | Emitter not found |
| v0.4.0 | GitHub release | Emitter not found |
| v0.3.2 | GitHub release | Emitter not found |
| v0.3.1 | GitHub release | Emitter not found |
| v0.3.0 | GitHub release | Emitter not found |
| v0.2.5 | GitHub release | Emitter not found |
| v0.2.4 | GitHub release | Emitter not found |
| v0.2.3 | GitHub release | Emitter not found |
| v0.2.2 | GitHub release | Emitter not found |
| v0.2.1 | GitHub release | Emitter not found |
| v0.2.0 | GitHub release | Emitter not found |
| v0.1.3 | GitHub release | Emitter not found |
| v0.1.2 | GitHub release | Emitter not found |
| v0.1.1 | GitHub release | Emitter not found |
| v0.1.0 | GitHub release | Emitter not found |
