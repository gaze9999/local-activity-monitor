# AI entry

For human documentation, start at `docs/README.md`; application usage and settings stay separate from WBUI component tutorials.

Local Activity Monitor owns local source projections, collection, HTTP endpoints, application layout and preference persistence. Resolve the actual runtime from `pyproject.toml`, frontend from `frontend/`, and shared UI revision from `workbench-ui.json`. Follow owner-supplied local `AGENTS.md` when present. Read only the task's sources; preserve unrelated dirty changes and authorized data boundaries.

## Select the task path

| Task | Documentation | Implementation / focused check |
| --- | --- | --- |
| Data provenance, missing values or health | `docs/data-inventory.md`, `docs/architecture.md` | Matching collector / projection in `src/local_activity_monitor/`, source fixture tests, null / zero / false distinctions |
| Tabs, details, live updates or file-page delay | `docs/usage.md`, `docs/performance-report.md` | `frontend/app.js`, `frontend/style.css`, `files-navigation-flow.cjs`, `partial-refresh-flow.cjs`, `table-stream-interaction-flow.cjs` |
| Summary sources, custom cards or groups | `docs/card-library.md`, `docs/settings-format.md` | Summary / card layout helpers in `frontend/app.js`, `settings-cards-flow.cjs`, stable IDs and keyboard / pointer cancel |
| Settings import or diagnostic sharing | `docs/settings-format.md` | Settings validation / export helpers in `frontend/app.js`, `diagnostic-settings-flow.cjs`, preserve complete local settings |
| Debug, CPU or theme | `docs/usage.md`, `docs/features.md` | `performance_debug.py`, `monitor_state.py`, frontend metrics / appearance, `debug-flow.cjs`, `cpu-theme-flow.cjs` |
| Shared UI or pinned assets | `workbench-ui.json`, `docs/maintenance.md` | `tools/build_frontend.py`, `test_frontend_build.py`, selected revision's WBUI `AI.md` / declarations / API index |
| History / Context refresh / agent relationships | `docs/usage.md` | History / communication helpers in `frontend/app.js`, `collectors.py` context record offsets, `history-flow.cjs`, `context-refresh-flow.cjs`, `test_context_detail.py` |
| Feature status, candidates or delivery | `docs/features.md`, `docs/validation.md`, `README.md` | Distinguish implementation, focused fixture evidence, live acceptance and release authorization |

Paths under the implementation column are relative to `frontend/`, `src/local_activity_monitor/` or `tests/` as named. Do not execute observed tool arguments, SQL or source code. Use bounded metadata fixtures, not production source bodies, credentials or real account probes.

## Compose with the actual WBUI revision

WBUI owns reusable presentation, interaction, state and disposal. LAM owns source IDs, queries, sorting, application pages, detail requests, latest-response decisions and storage. Verify the selected library exports before constructing options; an adjacent working tree or package version does not establish consumer adoption.

LAM's current pin selects WBUI 0.9.0. WBUI 0.10.0's row activation / selection APIs, new parsers and bullet chart have not been adopted. Do not use them through the old assets or change the pin without owner authorization. Source splitting keeps WBUI's published loading entry unchanged.

LAM's existing table adapter initializes pagination before attaching rows, keeps the complete source and stable identities, and mounts only the current page. It is separate from WBUI `createTable`. Paging and viewport-deferred charts are implemented; continuous virtual scrolling remains WBUI planning work. Numerical precision, formula AST and 3D are also library candidates, not LAM capabilities.

## Verify and report

- Use `docs/validation.md` for actual test prerequisites, checked source hashes and fixture scope. Select the existing tests for changed behavior, do not repeat unrelated acceptance or infer runtime adoption
- Build only the selected WBUI revision with `python tools/build_frontend.py` when frontend assets changed. `--latest` / pin updates are separate owner decisions
- Use the configured Python interpreter with `PYTHONPATH=src` for focused `unittest` files. JavaScript syntax and runtime / page metric / usage projection checks are listed in README
- Browser flow files export functions accepting a Playwright Page. Use an already installed browser / tooling and `tests/serve_loading_fixture.py` with isolated demo data, inspect console / DOM / IDs / focus / cleanup, then stop only owned fixtures and browsers
- A background update may change identified values while retaining row order; focus, selection and drafts protect content. Keep latest-value coalescing, do not rebuild the page or remove text to pause animation
- Preserve local preferences. Diagnostic export is a deidentified whitelist copy with optional gzip / JSON fallback, not a portable settings backup or permission to transmit it
- Report checked source, changed filenames, actual results and remaining platform / live-data gaps. Preserve SSE, consumer pins, production services and Git delivery boundaries unless the owner explicitly changes scope

For ongoing work, restore goals separately from progress using the local checkpoint designated by `AGENTS.md`, if present. The distributed entry does not require private checkpoints or local agent files.
