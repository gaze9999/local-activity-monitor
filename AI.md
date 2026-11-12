# AI entry

For human documentation, start at `docs/README.md`; application usage and settings stay separate from WBUI component tutorials.

Local Activity Monitor owns local source projections, collection, HTTP endpoints, application layout and preference persistence. Resolve the actual runtime from `pyproject.toml`, frontend from `frontend/`, and shared UI revision from `workbench-ui.json`. Follow owner-supplied local `AGENTS.md` when present. Read only the task's sources; preserve unrelated dirty changes and authorized data boundaries.

## Select the task path

| Task | Documentation | Implementation / focused check |
| --- | --- | --- |
| Data provenance, missing values or health | `docs/data-inventory.md`, `docs/architecture.md` | Matching collector / projection in `src/local_activity_monitor/`, source fixture tests, null / zero / false distinctions |
| Session JSONL framing, JSON decoding and token counters | `docs/maintenance.md#rust-session-掃描器` | `rust/session-core/`, `rust/session-scan/`, `src/local_activity_monitor/session_scan.py`, `src/local_activity_monitor/session_checkpoint.py`, `tests/test_session_scan.py` |
| Native SQLite history and checkpoints | `docs/maintenance.md#rust-歷史儲存模組` | `rust/history-store/`, `src/local_activity_monitor/history_store.py`, `tests/test_durable_history.py` |
| SQLite history schema upgrade and legacy keys | `docs/maintenance.md#rust-歷史儲存模組` | `rust/history-store/src/migration.rs`, `rust/history-store/src/timestamp.rs`, `rust/history-store/tests/migration.rs` |
| Legacy JSON history import and metadata projection | `docs/maintenance.md#rust-舊版-json-歷史匯入` | `rust/history-store/src/legacy.rs`, `rust/history-store/src/mcp.rs`, `rust/history-store/tests/legacy.rs` |
| Native HTTP entry, history routes and pinned assets | `docs/maintenance.md#rust-服務入口` | `rust/service/src/`, `rust/service/tests/http.rs`, `tests/test_http_contract.py` |
| Native session collection and snapshot fields | `docs/maintenance.md#rust-服務入口` | `rust/service/src/sessions/`, `rust/service/tests/http.rs`, `src/local_activity_monitor/collectors.py` |
| State catalog execution and parent metadata | `docs/data-inventory.md` | `rust/service/src/sessions/metadata.rs`, `rust/service/src/sessions/schedules.rs`, `src/local_activity_monitor/codex_metadata.py`, `tests/test_subagent_sources.py`; retain selected metadata only |
| Project snapshots and restricted detail | `docs/data-inventory.md` | `rust/service/src/sessions/projects.rs`, `rust/service/src/sessions/instructions.rs`, `rust/service/src/server.rs`, `src/local_activity_monitor/codex_metadata.py`, `src/local_activity_monitor/project_instructions.py`, `tests/test_projects.py`, `tests/test_instruction_sources.py`; expose folders and instruction contents only on demand for an observed project |
| Native direct MCP and Web observations | `docs/maintenance.md#rust-服務入口` | `rust/history-store/src/mcp.rs`, `rust/history-store/src/observations.rs`, `rust/service/src/sessions/mcp.rs`, `rust/service/tests/http.rs` |
| Static nested exec call recognition | `docs/maintenance.md#rust-服務入口` | `rust/session-core/src/invocations.rs`, `rust/service/src/sessions/project.rs`, `rust/service/tests/http.rs`, `src/local_activity_monitor/operation_records.py` |
| Native Git and validation command classification | `docs/maintenance.md#rust-服務入口` | `rust/session-core/src/commands.rs`, `rust/service/src/sessions/commands.rs`, `rust/service/tests/http.rs` |
| Native SQLite session classification and history | `docs/maintenance.md#rust-服務入口` | `rust/session-core/src/sqlite.rs`, `rust/session-core/src/sqlite/python.rs`, `rust/service/src/sessions/sqlite.rs`, `tests/test_sqlite_records.py` |
| Retained SQLite and MCP aggregates | `docs/maintenance.md#rust-服務入口` | `rust/history-store/src/observations.rs`, `rust/history-store/src/aggregate.rs`, `rust/service/src/sessions/history_view.rs`, `rust/history-store/tests/observation_view.rs` |
| Codex Core SQLite diagnostics | `docs/maintenance.md#rust-服務入口`, `docs/data-inventory.md` | `rust/service/src/sessions/core_logs.rs`, `rust/session-core/src/sqlite/diagnostic.rs`, `src/local_activity_monitor/error_records.py`, `rust/service/tests/http.rs` |
| Model API diagnostics | `docs/data-inventory.md`, `docs/maintenance.md#資料收集與保存` | Python projection and summary in `src/local_activity_monitor/api_records.py`, Rust Core projection in `rust/service/src/sessions/model_api.rs` and `rust/service/src/sessions/core_logs.rs`; Desktop input remains pending |
| Desktop diagnostic collection | `docs/data-inventory.md`, `docs/maintenance.md#資料收集與保存` | Python `diagnostic_roots`, `desktop_line` and `refresh_desktop` in `src/local_activity_monitor/error_records.py`; Rust `rust/service/src/sessions/desktop.rs`, `rust/service/src/sessions/core_logs.rs` and `rust/service/tests/http.rs`; platform and source statistics acceptance remain pending |
| Native error history and recent preview | `docs/maintenance.md#rust-服務入口`, `docs/data-inventory.md` | `rust/history-store/src/error_history.rs`, `src/local_activity_monitor/error_history.py`, `rust/service/tests/http.rs` |
| Tool-result errors | `docs/data-inventory.md`, `docs/maintenance.md#資料收集與保存` | Python `tool_error` and call ownership in `src/local_activity_monitor/error_records.py` and `collectors.py`; Rust `Call::complete` and unmatched output handling in `rust/service/src/sessions/project.rs`, `output_error` in `rust/history-store/src/mcp.rs`, and checkpoint transaction in `rust/service/src/sessions/checkpoint.rs` |
| Native session checkpoint restore and conflict handling | `docs/maintenance.md#rust-session-checkpoint` | `rust/service/src/sessions/checkpoint.rs`, `rust/history-store/src/checkpoint.rs`, `rust/service/tests/http.rs` |
| Native SSE and projected session logs | `docs/maintenance.md#rust-服務入口` | `rust/service/src/events.rs`, `rust/service/src/sessions/`, `rust/service/tests/http.rs`, `tests/test_http_contract.py` |
| Tabs, details, live updates or file-page delay | `docs/usage.md`, `docs/performance-report.md` | `frontend/app.js`, `frontend/style.css`, `files-navigation-flow.cjs`, `partial-refresh-flow.cjs`, `table-stream-interaction-flow.cjs` |
| Summary sources, custom cards or groups | `docs/card-library.md`, `docs/settings-format.md` | Summary / card layout helpers in `frontend/app.js`, `settings-cards-flow.cjs`, `settings-sources-flow.cjs`, stable IDs and keyboard / pointer cancel |
| Table columns or instructions layout | `docs/usage.md` | `dragColumn` / `moveColumn` / `renderTableColumns` and instructions detail in `frontend/app.js`, `table-column-flow.cjs`, `instructions-layout-flow.cjs`, `detail-tabs-flow.cjs` |
| Settings import or diagnostic sharing | `docs/settings-format.md` | Settings validation / export helpers in `frontend/app.js`, `diagnostic-settings-flow.cjs`, preserve complete local settings |
| Appearance, palettes, browser / Codex metadata | `docs/usage.md`, `docs/settings-format.md` | `appearance-controls-flow.cjs`, `card-presentation-flow.cjs`, `theme-api-flow.cjs`, pinned color tokens and modal cleanup |
| Cloud chats, dots or schedules | `docs/usage.md`, `docs/architecture.md` | `codex_cloud.py`, `desktop_activity.py`, `codex_schedules.py`, `test_codex_cloud.py`, `test_desktop_activity.py`, `cloud-activity-flow.cjs`, manual CLI queries vs cached activity |
| Opt-in account / public / OpenAI Platform queries | `docs/usage.md`, `docs/architecture.md` | `api_queries.py`, `public_api.py`, `openai_platform.py`, `server.py`, query fixture tests, `api-query-flow.cjs` and `decisions-platform-flow.cjs`, fixed URLs / disabled default / bounded cache |
| Debug, CPU or theme | `docs/usage.md`, `docs/features.md` | `performance_debug.py`, `monitor_state.py`, frontend metrics / appearance, `debug-flow.cjs`, `cpu-theme-flow.cjs` |
| Shared UI or pinned assets | `workbench-ui.json`, `docs/maintenance.md` | `tools/build_frontend.py`, `test_frontend_build.py`, selected revision's WBUI `AI.md` / declarations / API index |
| History / Context refresh / agent relationships | `docs/usage.md` | History / communication helpers in `frontend/app.js`, `collectors.py` context record offsets, `history-flow.cjs`, `context-refresh-flow.cjs`, `test_context_detail.py` |
| Feature status, candidates or delivery | `docs/features.md`, `docs/validation.md`, `README.md` | Distinguish implementation, focused fixture evidence, live acceptance and release authorization |

Paths under the implementation column are relative to `frontend/`, `src/local_activity_monitor/` or `tests/` as named. Do not execute observed tool arguments, SQL or source code. Use bounded metadata fixtures, not production source bodies or credential files. Live account metadata queries require explicit task authorization and a supported read-only CLI.

## Compose with the actual WBUI revision

WBUI owns reusable presentation, interaction, state and disposal. LAM owns source IDs, queries, sorting, application pages, detail requests, latest-response decisions and storage. Verify the selected library exports before constructing options; an adjacent working tree or package version does not establish consumer adoption.

LAM selects the exact WBUI revision recorded in `workbench-ui.json` and its verified embedded assets; the adopted version and evidence are listed at the top of `docs/validation.md`. `subscribe(options, listener)` replaces listener-first calls. The default loader uses the embedded bundle; the local development selection has been removed after successful pin and asset synchronization. Loading requires WBUI 0.13.0 or later; packaging uses `build_frontend.py --verify-offline` to require compatible verified assets with matching frontend/pin revisions. Source splitting keeps WBUI's published loading entry unchanged.

LAM's existing table adapter initializes pagination before attaching rows, keeps the complete source and stable identities, and mounts only the current page. It is separate from WBUI `createTable`. Eligible sums / means / latest values use filtered retained rows and a revision-aware cache. Modal panes leave their final table unfolded. Paging and viewport-deferred charts are implemented; continuous virtual scrolling uses capability-gated `bindVirtualScroll` from the selected assets when enabled. LAM has not adopted the library's formula or optional calculation capabilities. Check the pinned API before selecting a separate integration; keep the existing classic shared loader separate from optional module profiles.

## Verify and report

- At the start of each Python-to-Rust migration round, verify the live `main` tips for LAM and its workspace, review changes since the last recorded comparison, and port relevant changes to Rust before delivery. The workspace WBUI gitlink and LAM's consumer pin are separate inputs.
- Use `docs/validation.md` for actual test prerequisites, checked source hashes and fixture scope. Select the existing tests for changed behavior, do not repeat unrelated acceptance or infer runtime adoption
- Build only the selected WBUI revision with `python tools/build_frontend.py` when frontend assets changed. `--latest` / pin updates are separate owner decisions
- Use the configured Python interpreter with `PYTHONPATH=src` for focused `unittest` files. JavaScript syntax and runtime / page metric / usage projection checks are listed in README
- Browser flow files export functions accepting a Playwright Page. Use an already installed browser / tooling and `tests/serve_loading_fixture.py` with isolated demo data, inspect console / DOM / IDs / focus / cleanup, then stop only owned fixtures and browsers
- A background update may change identified values while retaining row order; focus, selection and drafts protect content. Keep latest-value coalescing, do not rebuild the page or remove text to pause animation
- Preserve local preferences. Diagnostic export is a deidentified whitelist copy with optional gzip / JSON fallback, not a portable settings backup or permission to transmit it
- Report checked source, changed filenames, actual results and remaining platform / live-data gaps. Preserve SSE, consumer pins, production services and Git delivery boundaries unless the owner explicitly changes scope

For ongoing work, restore goals separately from progress using the local checkpoint designated by `AGENTS.md`, if present. The distributed entry does not require private checkpoints or local agent files.
