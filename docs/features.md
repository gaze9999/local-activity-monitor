# 功能與待實作項目

LAM 讀取本機 Codex 與 MCP 紀錄, 提供活動、用量、操作及系統監測. 共用元件由 Workbench UI 維護, LAM 管理資料來源、欄位意義與設定

## 已實作

| 功能 | 行為與範圍 |
| --- | --- |
| 啟動與程序管理 | CLI 建置前端、保留取得失敗時的有效資產, 啟動視窗關閉時結束其所屬子程序 |
| 循環記錄檔 | 啟動記錄檔與執行中診斷分別輪替, 記錄啟動、更新、前端錯誤及結束事件 |
| 模型 API 呼叫監測 | 讀取 Codex diagnostics 中的 HTTP、WebSocket、連線及重試事件, 依事件類型分開統計 |
| 本機即時更新 | 一個畫面共用一條 SSE, 更新通知後取得快照, 中斷時使用定時查詢 |
| 對話與代理關聯 | 對話、子代理程式及 Context 分頁, 提供代理訊息明細, 顯示父子角色與 Thread ID, Context 列可點選 |
| Token 速率 | 對話表格提供平均 Token / 秒, 使用最新累計 Token 除以已記錄工作時間 |
| MCP 活動 | 依設定與紀錄產生來源卡、呼叫趨勢及明細, 分開顯示直接呼叫、程式碼辨識位置與來源紀錄 |
| 工作操作 | Git、工作樹、驗證、Skills、SQL、網路及錯誤紀錄 |
| 來源檢查 | 顯示每個來源的讀取結果、最近檢查時間、檔案數及回補進度 |
| 歷史資料 | SQLite 保存有界 metadata, 分批回補與增量更新, 設定保留天數與清理過期摘要 |
| 本機排程 | 讀取本機自動化定義、狀態與執行紀錄 |
| 其他電腦活動 | 讀取 Codex 本機保存的遠端對話與 My dots 活動摘要, 列出電腦、近期活動與對話數 |
| 系統資訊 | 裝置、使用量與 GPU, 依實體顯示卡及介面識別去重 |
| 外觀設定 | 系統字型選單、字級、語言、主題、卡片順序、欄位順序及篩選偏好 |
| 動態內容 | 接近可視區域後載入內容, 長程式碼依可視範圍預先上色, 保留完整文字與捲動位置 |
| 共用互動 | 載入狀態、展開與收合、按鈕間距、標籤、下拉選單樣式及減少動態效果設定 |
| 螢幕配置 | 橫向、直向與手機可視寬度, 直向兩欄卡片、摘要與子頁籤分區、寬表格區域捲動及明細高度限制 |
| 啟動預載 | 啟動先讀取有界對話目錄, 活動紀錄在背景整理, 首輪完成前仍可閱讀對話列表 |
| 明細導覽 | 主要明細由列點選開啟, 列內連結提供不同紀錄或個別操作, My dots 活動、產出與電腦可查看已有資訊 |
| 表格顯示 | 預設顯示主要欄位, 保留欄位設定, 空資料隱藏表頭並顯示提示, 分頁導引置中且提供第一頁與最後一頁 |
| 樹狀明細 | 專案對話與技能檔案採可收合的階層, 預設收合, SVG 表示資料夾與檔案類型 |
| 檔案大小檢查 | 進入檔案頁自動檢查, 顯示期間每分鐘更新, 可手動重新檢查, 每次最多 100 個已觀察檔案 |

## 待實作與待驗證

| 項目 | 現況 | 完成條件 |
| --- | --- | --- |
| 其他電腦的完整排程定義 | 本機快取可取得活動摘要, 尚無排程定義與下次執行時間的讀取入口 | 串接 Codex 已授權的唯讀介面, 以電腦與排程識別碼核對定義、狀態及下次執行時間 |
| My dots 電腦即時連線狀態 | 已列出近期活動電腦, 本機摘要未提供目前連線證據 | 取得可用的連線狀態與檢查時間, 區分近期活動與目前連線 |
| TOML、XML、CSV / TSV 結構預覽 | 原文可讀取, WBUI 尚未內建這些格式的資料解析 | 依格式規格加入有界解析, 核對跳脫、多行、錯誤與原文保留 |
| SQL AST、完整 YAML 與 Markdown | 已有 SQL 排版、YAML 子集及 Markdown 預覽 | 依使用情境補齊 dialect、節點、source map 與完整格式規格 |
| 共用元件候選 | WBUI 已列出 sortable list、列拖曳、跨容器拖放、圖表縮放及面板編輯等候選 | 按 [WBUI 元件盤點](https://github.com/gaze9999/workbench-ui/blob/b2824dc6a478e6eb00e8b4f246b4ab5255781936/docs/component-coverage.md) 與所屬資料介面驗收 |
| macOS / Linux 字型與啟動實機驗證 | 字型讀取已分別使用 CoreText / fontconfig, 本次在 Windows 驗證 | 在對應系統核對字型名稱、啟動、視窗關閉與記錄檔輪替 |
| 其他瀏覽器的選單與動畫 | 共用樣式含原生選單及支援 `base-select` 的瀏覽器, 本次以 Edge 驗證 | 在目標瀏覽器核對選單、鍵盤、焦點、動畫中斷及內容載入 |
| 遠端 LAM 存取、配對及登入 | 設計文件保留候選介面, 尚未實作 | 另行確認部署、HTTPS、權限及配對規格後實作, 見 [遠端存取設計](remote-access.md) |
| 更多程式碼 parser | WBUI 已有格式註冊與可視範圍上色, 尚未串接 TextMate grammar 或語意 token provider | 保留原文、逐行狀態、上下預載與 Worker 取消, 核對 grammar 授權、執行成本及格式樣本 |
| 一般檔案內文 | 一般檔案目前顯示操作與本機 metadata, 技能與 MCP 文件已有按需內文 | 沿已觀察檔案識別建立受限唯讀入口, 保留路徑邊界、重新解析點、檔案型態與容量檢查, 分批讀取、遮蔽與原文切換 |
| 檔案大小分批續查 | 每次取得前 100 個不同檔案, 超出部分顯示已檢查 / 總數 | 定義穩定游標、變更失效、分批預算與取消, 不在每輪重新掃描全部檔案 |

## SSE 評估

目前使用本機 SSE 傳送快照版本通知, 畫面收到後取得完整快照, 中斷時由定時查詢補上. 本輪維持此行為, 尚未改成分區推送

| 資料 | 候選方式 | 需要核對 |
| --- | --- | --- |
| 整理階段、回補進度、來源讀取結果 | 小型狀態事件 | 狀態擁有者、事件合併、斷線後補最新值 |
| CPU、記憶體、GPU 使用量 | 最新樣本事件 | 取樣頻率、背景頁面限流、歷史樣本仍由查詢取得 |
| 對話、工具、Token 與活動統計 | 區域失效通知, 再取得該區資料 | 範圍與版本、去重、只更新目前使用的區域 |
| Context、工具回覆、文件與檔案內文 | 保持按需查詢 | 閱讀權限、遮蔽、容量與分批讀取, 不放入廣播事件 |

## UI 元件對照

依 [PrimeNG Table](https://primeng.dev/table)、[HeroUI Table](https://beta.heroui.com/docs/components/table) 與 [Nuxt UI Table](https://github.com/nuxt/ui/blob/v4/src/runtime/components/Table.vue)、[Tree](https://github.com/nuxt/ui/blob/v4/src/runtime/components/Tree.vue) 的官方介面核對, 本輪補齊空資料、列動作、分頁邊界、樹狀圖示與載入狀態. 元件數量不作為 LAM 的實作清單

後續優先評估大型表格的可視範圍呈現、階層導覽、組合搜尋與欄位條件篩選, 其餘候選沿 [WBUI 元件盤點](https://github.com/gaze9999/workbench-ui/blob/b2824dc6a478e6eb00e8b4f246b4ab5255781936/docs/component-coverage.md) 管理

程式碼上色可參考 VS Code 的 [TextMate 語法上色](https://code.visualstudio.com/api/language-extensions/syntax-highlight-guide) 與 [語意上色](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide). TextMate 依 grammar 分詞, 語意 token 由能分析專案的 provider 提供, 兩者與 TOML、CSV 等結構解析分開評估

## 驗收重點

各分頁共用載入、排序、篩選、返回與明細生命週期, 依資料用途保留專用操作. Context 列開啟目前對話的 Context, 代理訊息按鈕開啟該對話訊息, 父對話連結開啟父對話, 欄位重排與背景更新後仍須保持此對應

效能比較分開記錄回補與無回補、冷啟動與增量更新的耗時、CPU 及讀取量. 測試入口見 [測試與建置](validation.md), 資料上限與量測方式見 [效能與資料上限](performance-report.md)

## 實作與驗證入口

| 檔案 | 用途 |
| --- | --- |
| `frontend/app.js` | 資料投影、預載呈現、來源共用計數、列明細、表格預設、空資料、首尾頁與圖表載入後重排 |
| `frontend/index.html`, `frontend/style.css` | 摘要與子頁籤分區、工作樹檢查與紀錄分區、兩行狀態、直向卡片與置中分頁 |
| `frontend/locales.json` | 英文與日文欄位、狀態與操作文字 |
| `src/local_activity_monitor/codex_metadata.py`, `collectors.py` | 有界目錄查詢與活動資料投影 |
| `src/local_activity_monitor/server.py` | 啟動預載與 HTTP 錯誤回應, 保留本文請求的關閉保護 |
| `docs/architecture.md`, `performance-report.md`, `settings-format.md`, `usage.md`, `validation.md`, `features.md` | 架構、實測、設定、操作、驗證及功能狀態 |
| `tests/followup-layout-flow.cjs`, `viewport-detail-flow.cjs`, `tab-consistency-flow.cjs` | 圖表恢復、主要互動、橫向與直向排版、語系及所有分頁 |
| `tests/startup-preload-flow.cjs`, `serve_loading_fixture.py`, `test_startup_preload.py` | 啟動目錄的畫面、HTTP 時序、查詢上限及停用後不讀取 |
| `tests/history-flow.cjs`, `test_activity_details.py`, `test_event_stream.py` | 歷史首尾頁、literal 驗證辨識、HTTP 回應與串流邊界 |
| Workbench UI `src/workbench-ui.js`, `workbench-ui.css`, `workbench-ui.d.mts` | 共用 Tree SVG 圖示、表格空資料與首尾頁, 含型別與樣式 |
| Workbench UI `docs/usage.md`, `component-coverage.md`, `tests/tree-flow.cjs`, `table-boundary-flow.cjs` | 共用操作規格、元件候選、樹狀鍵盤及表格邊界驗證 |
