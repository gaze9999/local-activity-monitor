# 測試與建置

## 0.14.0 歷史分頁、代理關聯與 Context 更新

2026-10-11 00:02, Asia/Taipei, 受測來源為 `b0275e5` 之後的本機 0.14.0 工作目錄, `workbench-ui.json` 維持 `cf13a9417d752001ec388ab74a0f0fddab238066` / WBUI 0.9.0, 下方各節保留先前受測來源

| 來源 | SHA-256 前 16 字元 |
| --- | --- |
| `frontend/app.js` | `97a61bee47b153eb` |
| `frontend/style.css` | `e4e61c328b7edb5d` |
| `frontend/index.html` | `5281ed100f40ee9c` |
| `frontend/locales.json` | `4ce46e1098074cae` |
| `src/local_activity_monitor/collectors.py` | `7897e7fa6727974e` |

| 檢查 | 結果與範圍 |
| --- | --- |
| 歷史紀錄 | Edge 155.0.4283.45 的 `history-flow` 通過 SQL / SQLite、網路、MCP、錯誤、技能各自的頁內入口及分頁, 175 筆 MCP 紀錄以 cursor 逐頁讀取皆保留唯一 ID, 首末 / 前後頁、503 恢復、無效 cursor 400 與空資料隱藏表頭通過 |
| Context 顯示更新 | `context-refresh-flow` 核對修改段落與追加紀錄, 未變動紀錄保留 DOM、捲動區、展開狀態與捲動位置, 讀取失敗保留內容, 恢復及明細重建後的捲動位置通過 |
| 代理與用途 | 同一流程核對父對話 / 子代理入口及實際 thread ID, Goal / Plan 歷史標籤移至獨立「已觀察用途」欄位, 20 欄表頭與儲存格一致, 1366 / 390 / 320 px 無頁面水平溢位 |
| 明細回歸 | `context-agent-flow`、`context-layout-flow` 核對來源空 / 尚未記錄、遮蔽與請求取消、父子來源分開及三語系窄畫面 |
| 表格與更新 | `files-navigation-flow`、`partial-refresh-flow`、`tab-consistency-flow` 通過, 1,000 筆檔案來源只掛載 10 列, ID / Enter / Space / 零值 / 分頁 / 搜尋保留, 本次冷開啟合成樣本 567.4 ms |
| Python 與建置 | 前端建置 16/16 通過, Context / history store / monitor state / performance debug 合計 38 筆, 37 通過、1 因 Windows 未允許建立 symlink 而略過, Context byte offset 在追加及同長度修改後保留原紀錄身分 |

Context 來源讀取仍使用既有有界、按需端點, 本次增量處理在顯示層, 不新增常駐內容保存或 SSE 變更, 代理關聯入口使用已觀察的 parent_thread_id, 不將子對話訊息冒充父對話訊息, 換頁動畫以目前 pin 的 CSS 相容規則限制捲動範圍, 共用修正另由 WBUI 0.10.0 提供

測試使用自行建立的 `serve_loading_fixture.py`、隔離 SQLite 與合成訊息, 不讀取正式帳戶內容, 正式 8787 服務未重新啟動, 使用端 pin 未更換, 本輪未執行五平台包建置或 Firefox / WebKit

本次調整 `frontend/app.js`、`frontend/index.html`、`frontend/style.css`、`frontend/locales.json`、`src/local_activity_monitor/collectors.py`、`tests/history-flow.cjs`、`tests/test_context_detail.py`, 新增 `tests/context-refresh-flow.cjs`, 同步 `AI.md`、`docs/features.md`、`docs/usage.md`、`docs/maintenance.md` 與本驗證紀錄, 版本來源 `pyproject.toml`、`src/local_activity_monitor/__init__.py` 由已發布的 0.13.0 遞增至 0.14.0

### 本批交付檔案

- 文件、教學與 AI 入口: `AI.md`、`README.md`、`docs/README.md`、`docs/architecture.md`、`docs/card-library.md`、`docs/features.md`、`docs/maintenance.md`、`docs/performance-report.md`、`docs/settings-format.md`、`docs/usage.md`、`docs/validation.md`
- LAM 畫面、語系與樣式: `frontend/app.js`、`frontend/index.html`、`frontend/locales.json`、`frontend/style.css`
- 版本、來源清單與索引: `pyproject.toml`
- LAM 資料與診斷: `src/local_activity_monitor/__init__.py`、`src/local_activity_monitor/collectors.py`、`src/local_activity_monitor/monitor_state.py`、`src/local_activity_monitor/performance_debug.py`
- 測試與合成驗證: `tests/chart-copy-flow.cjs`、`tests/context-refresh-flow.cjs`、`tests/cpu-theme-flow.cjs`、`tests/debug-flow.cjs`、`tests/diagnostic-settings-flow.cjs`、`tests/files-navigation-flow.cjs`、`tests/history-flow.cjs`、`tests/partial-refresh-flow.cjs`、`tests/serve_loading_fixture.py`、`tests/settings-cards-flow.cjs`、`tests/test_context_detail.py`、`tests/test_monitor_state.py`、`tests/test_performance_debug.py`

## AI 文件入口

[AI.md](../AI.md) 按資料來源、畫面 / 更新、卡片 / 設定、診斷與固定 WBUI 版本提供實作及 focused 測試路徑. README、功能、卡片庫、設定格式與架構同步目前候選, 分清 LAM 已採用的 0.9.0 與相鄰 WBUI 未交付的新能力

本次只更新文件與本機指引, 不改 LAM runtime、資料庫或 SSE. 文件核對參照實際來源 / 型別 / pin、相對連結 / 程式碼區塊及本機 textlint / 人工語意複核, 下節保留先前行為測試的受測來源與證據

2026-10-10 文件整理另加入[文件索引](README.md), README 保留已實作功能與操作入口, 待辦及遠端設計仍留 docs. 14 份公開 Markdown 文件的連結 / 標題 / 程式碼區塊、本機 textlint 與人工複核, 不重跑 LAM 行為測試, 元件教學由 WBUI 的文件與合成流程驗證

本輪新增 `docs/README.md`, 調整 `README.md`、`AI.md` 與 `docs/validation.md`. 既有未提交的其他來源變更保留

## 2026-10-10 工作目錄驗證

受測來源為 `b0275e5` 之後的本機 `0.13.0` 未提交修改, Python 3.12.14、Node.js 22.19.0、Microsoft Edge 155.0.4283.45 headless. LAM 保持 `workbench-ui.json` 指定的 WBUI `cf13a9417d752001ec388ab74a0f0fddab238066` / 0.9.0, 未採用相鄰 WBUI 0.10.0 候選. 本輪未修改 SSE, 未重新啟動正式服務

受測檔案 SHA-256 前 16 字元:

| 檔案 | 來源 hash |
| --- | --- |
| `frontend/app.js` | `34fe264eb1a3aa07` |
| `frontend/style.css` | `685b5a78823c3c1b` |
| `frontend/index.html` | `25801082f9dcc830` |
| `frontend/locales.json` | `7d23da216121f392` |
| `monitor_state.py` | `d8214f61dec39654` |
| `performance_debug.py` | `4ccac60ef774341a` |

| 檢查 | 結果與範圍 |
| --- | --- |
| 檔案頁 | `files-navigation-flow.cjs` 通過隱藏 / 可見頁首次只掛載 10 列、完整 1,000 筆來源、排行、分頁、Enter / Space 對應目前 ID、metadata 零值、搜尋換頁及保留 DOM. 900 px 直向、390 / 320 px 無頁面溢位 |
| 設定與卡片 | `settings-cards-flow.cjs` 通過來源 / 自訂卡片與群組、排序及取消、穩定 ID、Goal / Plan 實際觀察、回補完成 / 未完成文案、120 px 載入及五種寬度 |
| 更新 | `stable-refresh-flow.cjs`、`partial-refresh-flow.cjs`、`table-stream-interaction-flow.cjs` 通過 DOM / 焦點 / 頁碼保留、hover 數值局部更新、保留列順序、焦點保護、離開套用最新值、背景暫停動畫、系統減少動畫及來源更新後恢復 |
| 資料與布局 | `chart-copy-flow.cjs`、`loading-layout-flow.cjs` 通過合成錯誤類型 / Log 等級、空資料文案、帳戶零值 / false、卡片不重疊、頁尾位置及三種寬度 |
| CPU / 主題 | `cpu-theme-flow.cjs` 通過 CPU 零值與缺值分開、棕色主題 / 共用 tokens 及直向布局 |
| 診斷設定副本 | `diagnostic-settings-flow.cjs` 通過自訂名稱 / 路徑 / 數值 ID 去識別化、gzip 往返、JSON 備用方式, 本機設定完整保留. 合成樣本 18,121 bytes 壓縮為 662 bytes |
| Debug | `debug-flow.cjs` 通過主設定開關、路徑、21 筆隔離紀錄、32 MiB 上限、無收集 heartbeat、關閉停止記錄及隱藏面板. 三語系各 1366 / 390 / 320 px 無溢位 |
| 來源與建置 | 前端建置與 `test_frontend_build.py` 16/16、`test_performance_debug.py` 8/8、`test_monitor_state.py` 18/18 通過, JavaScript runtime / 頁面指標 / 用量投影 focused checks 通過 |

瀏覽器使用 `tests/serve_loading_fixture.py` 的 demo threads 與隔離 SQLite, 不讀取正式帳戶或實際來源內容. 同一瀏覽器流程可將各 `tests/*-flow.cjs` 匯出的函式交給 Playwright Page 執行, fixture 服務及瀏覽器由測試建立者清理. 具體效能樣本見[效能紀錄](performance-report.md)

上述結果涵蓋合成資料及指定操作, 實機來源、Firefox / WebKit、輔助技術、完整 WCAG 與長時間 GC 後記憶體尚未驗證. 不把相鄰 WBUI 候選的檢查當成此固定版本已採用的新能力

## 原始碼檢查

需要 Python 3.10+ 與已準備的 WBUI 資產. 建置頁面後以 checkout 的 `src` 執行測試

Windows PowerShell:

```powershell
python tools/build_frontend.py
$env:PYTHONPATH = "src"
python -m unittest discover -s tests -v
```

macOS / Linux:

```sh
python3 tools/build_frontend.py
PYTHONPATH=src python3 -m unittest discover -s tests -v
```

測試使用暫存 metadata、SQLite 與來源 fixture, 涵蓋資料投影、差異讀取、checkpoint、內容遮蔽、來源快取、HTTP 邊界、啟動及建置回復

Windows 的來源更新與閒置 I/O 比較使用 `tools/benchmark_source_updates.py`, 先完成啟動整理再量測. 舊版從指定 Git 提交擷取, 新版凍結目前原始碼, 使用相同 12 檔合成資料, 每組獨立 home, 不開啟 HTTP、帳戶 API、硬體查詢或 Debug. 舊版預設 10 秒輪詢, 另以已滿 5 分鐘的 fixture 狀態檢查閒置模式. 兩版交換順序執行, 程序 I/O 計數先以 256 KiB 讀寫校準, 結果保存於 `.local/source-update-io-performance.json`

```powershell
python -X utf8 -B tools/benchmark_source_updates.py --baseline a1cc7fe --duration 65 --runs 2
python -X utf8 -B tools/benchmark_source_updates.py --baseline a1cc7fe --case active --duration 35 --runs 2
```

`active` 模式由獨立程序每秒追加一筆資料, 最後保留 15 秒讓兩版讀完相同內容, 核對最新 Token 值. 產生資料的程序 I/O 與 CPU 排除於量測, 結果保存於 `.local/source-update-io-active-performance.json`

比較未提交的原始碼時, 先保存修改前的來源, 再以 `--baseline-source` 選取該副本. 副本與輸出均放在 checkout 的 `.local`, 每組結果保存兩版 SHA-256, 並另記首次整理的 CPU、耗時與 I/O

```powershell
python -X utf8 -B -c "from pathlib import Path; from tools.benchmark_source_updates import freeze_sources; print(freeze_sources(Path('.local/before-source')))"
# 修改原始碼後執行
python -X utf8 -B tools/benchmark_source_updates.py --baseline-source .local/before-source --case active --duration 35 --runs 2 --output .local/optimized-active.json
python -X utf8 -B tools/benchmark_source_updates.py --baseline-source .local/before-source --duration 65 --runs 2 --output .local/optimized-idle.json
```

`aggregate_many` 核對同一唯讀交易的時間範圍、來源篩選、空結果與獨立副本, 並模擬讀取期間另一個 writer 新增資料. 錯誤與對話狀態檢查相同輸入略過、完整保存、offset 變更及失敗重試

`v0.11.0` 交付前固定 WBUI `v0.7.0` 的提交 `19a81a7`, 兩組資產 manifest 與完整 SHA 相符. 本機原生環境執行歷史、時間範圍、保存、HTTP、前端建置與資產檢查, 67 項中 65 項通過, 2 項因無法建立 symlink 與大型 HTTP 標準函式庫基準逾時略過. 三個 Node 使用端檢查通過. 正式固定資產方式的合成服務另通過四類表格換頁與 23 種明細流程, 包含三語、四種寬度、連點、取消、焦點與長網址提示

`v0.12.0` 交付前在 Windows、Python 3.12.14 執行完整 474 項測試, 471 項通過, 2 項因無法建立 symlink、1 項因大型 HTTP 標準函式庫基準逾時略過. 固定 WBUI `19a81a7` 的前端建置、資產檢查、JavaScript 語法及三個 Node 使用端檢查通過. Edge 154.0.4258.62 的隔離合成服務通過 12 類流程, 涵蓋 SSE、重連、還原設定、載入、啟動預載、互動期間保留表格、Debug、歷史圖表、換頁、摘要與直螢幕. 分頁 SVG 的水平與垂直中心偏移均為 0, 各按鈕高度一致, 未出現頁面程式錯誤. 正式資料的長時間效能及五平台原生包仍依後續驗收流程核對

`test_activity_history.py` 核對超過 1 MiB 的未變更輸入不重寫、完整 SQLite 保存與有界畫面資料、失敗後重試、快照獨立複製及即時保存期限篩選

`test_history_store.py` 檢查分區共用、舊資料匯入、版本升級備份、遷移失敗回復、寫入鎖定及不支援格式. `test_process_lifecycle.py` 檢查終止訊號、父程序控制串流關閉、Windows 隱藏子程序清理與資料保留

JavaScript 檢查需要 Node.js 22+:

```sh
node --check frontend/app.js
node tests/test_runtime_ui.mjs
node tests/test_page_metrics.mjs
node tests/test_usage_projection.mjs
```

## 瀏覽器流程

`tests/serve_loading_fixture.py` 建立隔離的合成 Codex home、session、SQLite 及工具紀錄, 啟動後輸出 loopback URL

```sh
python tests/serve_loading_fixture.py
```

各 `.cjs` 檔匯出接受 Playwright `page` 的流程, 對上述 fixture 執行:

| 流程 | 內容 |
| --- | --- |
| `loading-layout-flow.cjs` | 初次載入、延後更新、503、重試、缺值、零值、帳戶及卡片布局 |
| `appearance-layout-flow.cjs` | 主題、強調色、字型及偏好保存 |
| `font-resize-flow.cjs` | 字級、字型與視窗縮放 |
| `distribution-feedback-flow.cjs` | 分布圖、讀值及更新狀態 |
| `output-detail-flow.cjs` | 原文、內容分頁、取消與明細清理 |
| `detail-tabs-flow.cjs` | 23 種明細的概要首分頁與單一頂端導覽, 保留歷史資料查詢分類, 檔案執行內容延後讀取、遮蔽、快取、重試、取消與快速返回, 編輯草稿、三語四種畫面尺寸及背景更新後的長網址圖例提示 |
| `parser-summary-motion-flow.cjs` | 結構解析、摘要、減少動畫及設定匯入 |
| `console-sql-flow.cjs` | console 呈現、SQL 外層工具回覆、複製與捲軸 |
| `context-agent-flow.cjs` | 代理訊息、公開 Context 摘要、遮蔽切換、舊設定相容性及三語空紀錄 / 整理中提示 |
| `context-layout-flow.cjs` | 父子 Context、對話明細 Context 延後讀取與快取、列點選、欄位重排後的入口、展開中斷、長程式碼、窄螢幕明細與三語兩行狀態列 |
| `tab-consistency-flow.cjs` | 八個主頁及各子頁的卡片、表格、欄位、設定相容與四種寬度 |
| `model-api-flow.cjs` | 模型 API 事件分類、明細、缺值與來源 |
| `reactive-motion-adapter-flow.cjs` | 卡片、資料列與欄位插入 / 移出, 保留節點與目標計數 |
| `history-flow.cjs` | SQLite 歷史總數、游標分頁、錯誤後重試 |
| `pagination-motion-flow.cjs` | 對話、一般表格、歷史與說明分頁的左右滑動、快速連點、SVG 按鈕置中與一致高度、減少動畫及四種畫面寬度的溢出 / 按鈕重疊 |
| `history-chart-flow.cjs` | 完整保存範圍統計與總覽副本獨立範圍 |
| `reconnect-flow.cjs` | 中斷後自動重連與保留畫面 |
| `restart-reset-flow.cjs` | 後端更新及還原預設不重新載入整頁 |
| `event-stream-flow.cjs` | SSE 快照與 Log、重複及晚到資料、後端重啟與重連、連線關閉 |
| `debug-flow.cjs` | Debug 開關、監測程式 Tab 與明細、心跳不觸發來源收集、停止記錄及三語排版 |
| `stream-burst-flow.cjs` | 接受 page 與 CDP session, 1200 筆快照與獨立 / 混合 Debug 事件合併渲染、最新資料與回收後 heap / DOM 保留量 |
| `idle-interaction-flow.cjs` | 明確暫停與最新快照合併、hover / 焦點 / 選取 / touch / 失焦保護、診斷回應、無效網址、來源更新後恢復、狀態到期、背景切換及檔案資訊檢查頻率 |
| `idle-cadence-flow.cjs` | 隔離瀏覽器時鐘核對 60 秒的計時回呼、手動暫停後停止計時、裝置取樣時間與四種寬度 / 三語系排版, 計時次數不代表實機 CPU 改善幅度 |
| `partial-refresh-flow.cjs` | hover 更新數值並保留列身分、順序、篩選結果及分頁, 焦點保留內容, 新增 / 移除延後套用, 失焦與系統減少動畫 |
| `mcp-refresh-flow.cjs` | MCP 摘要更新保留卡片、欄位與標題、柔和光條、連續更新逐影格的可見性與高度、單一明細入口及三種寬度 / 三語系布局 |
| `stable-refresh-flow.cjs` | 總覽捷徑、額度、已展開來源、裝置欄位的焦點與文字選取、官方帳戶欄位、MCP 來源卡與表格、互動期間的欄位變動、文件列表及圖表保留 / 更新 |
| `table-stream-interaction-flow.cjs` | SSE 更新期間保留滑鼠與鍵盤目標, 離開後套用最新排序, 對話及 SQL 表格 |
| `followup-layout-flow.cjs` | 空圖表恢復、精簡欄位、Context 順序、摘要、列明細、空資料提示及首尾頁 |
| `startup-preload-flow.cjs` | 首輪活動整理前的對話目錄與完整結果切換, 使用隔離回應 |
| `viewport-detail-flow.cjs` | 截圖比例、不同字級與語系、兩行狀態、摘要與子頁籤分區、My dots 明細 |

`test_startup_preload.py` 核對 state 目錄查詢的 2,000 筆上限、session 讀取量為 0、未選取 prompt / credential 欄位、停用後不讀取及完整更新時間尚未產生. 瀏覽器模擬回應驗證呈現, 原生 fixture 可加 `--startup-delay 35`, 核對 HTTP 在整理前提供目錄, 整理後提供完整快照

## 更新後的正式資料驗收

更新後端後重新啟動服務, 再重新載入頁面. `/api/activity` 的 `code_revision` 與快照的 `backend_revision` 識別後端工作階段, `frontend_revision` 識別前端資產, 兩者分別核對. 使用 `tools/watch.py` 時, 確認其所屬後端已完成重新啟動

| 項目 | 驗收內容 |
| --- | --- |
| 來源計數 | 頁首可讀取 / 啟用數與來源 Log 共用讀取器清單, 停用列另計, 檢查時間與讀取結果相符 |
| 驗證與 MCP | 從已觀察操作核對分類、工具回傳、時間及耗時, 空資料時追查來源、篩選範圍與回補進度 |
| 父子 Context | Context 列開啟目前對話內容, 對話列表列開啟明細後切入 Context 才讀取, 切換分頁保留內容, 代理訊息與父對話入口維持對應, 核對更新、欄位重排與取消要求 |
| My dots 與排程 | 核對桌面快取中的活動及電腦, 即時連線與完整遠端排程仍列待實作 |
| 裝置資訊 | 依實體顯示卡識別核對數量, 在對應平台驗證字型、取樣時間與使用量 |
| 效能 | 固定資料、設定及取樣條件, 比較冷啟動、回補、增量與長時間更新, 分開記錄 CPU、耗時、讀取量及快照大小 |

`test_http_snapshot.py` 的大型 HTTP 本文案例會先檢查標準函式庫 HTTP 基準, 基準失敗時跳過該案例. `test_history_store.py` 與 `test_plugins.py` 的符號連結案例在無法建立連結時跳過. 驗收結果需列出實際跳過原因, 大型本文另以目標瀏覽器檢查 gzip 與未壓縮回應

macOS / Linux 實機啟動與字型、Firefox / WebKit、觸控、輔助技術及平台下載包的待驗收項目見[功能清單](features.md)

## 套件建置

建置相依列在 `tools/requirements-build.txt`, 專案 package metadata 由 `pyproject.toml` 管理

```sh
python -m pip install -r tools/requirements-build.txt
python tools/build_frontend.py
python -m build
python tools/build_release.py
```

Wheel 與 sdist 輸出至 `dist/`, 原生測試包位於 `.local/package-tests/`. 原生包由對應作業系統與處理器建置, `tools/smoke_release.py` 以暫存 home 及隨機 loopback 連接埠核對頁面、CSP、語系、版本、snapshot 與檔案存取邊界

## 平台 CI

[Build CLI packages](../.github/workflows/release.yml) 由 workflow_dispatch 啟動, 建置 Windows x64、macOS x64 / ARM64、Linux x64 / ARM64

每個平台先取得 `workbench-ui.json` 的完整 SHA, 準備 `_workbench/` 並建置 `_web/`, 再執行 unittest、原生建置與服務 smoke check. Source job 另產生 wheel、sdist 與含離線資產的原始碼 zip

所有套件通過後產生 `SHA256SUMS.txt`, 檔案保存在 Actions artifacts. 0.x 使用 minor tag, 1.0.0 起可透過 `publish` 附加至草稿 Release. 版本與下載紀錄可從 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml)查詢
