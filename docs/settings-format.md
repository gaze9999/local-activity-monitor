# 設定檔格式

設定 → 設定檔匯出 / 匯入 version 1 的完整偏好, 可跨瀏覽器, 電腦與 port 使用

全部設定還原預設會在目前頁面直接套用, 恢復外觀、語言、篩選、表格、圖表及來源設定, 不重新載入整頁. 後端重啟與前端資產版本分開判斷, 重新連線後套用已保存的來源設定, 保留目前分頁與篩選. 前端資產變動時才重新載入, 明細或設定視窗開啟時延後處理

外層包含 `application: "local-activity-monitor"`, `kind: "settings"`, `version: 1`, `exported_at`, `preferences`

| preferences 欄位 | 保存內容 |
| --- | --- |
| tab / tabOrder / subOrders | 主 Tab, 主 / 子 Tab 拖曳順序 |
| overview | 總覽圖表 order / hidden |
| cardVisibility | 各分頁卡片顯示, 以穩定識別碼保存 boolean |
| highlightOrder / cardOrders | 活動摘要與各區卡片順序 |
| conversationSource / diagnosticSource / activitySource / mcpSource | 對話, 錯誤與 Log, 專案活動與 MCP 選定子頁 |
| filterCollapsed | 各篩選區收合狀態, 最多 500 個 boolean 項目 |
| sectionCollapsed | 觀察來源等區塊的收合狀態, 最多 100 個 boolean 項目 |
| inputs / page | 搜尋, 篩選, 舊版來源 window, 對話頁碼 |
| appearance / locale | mode, theme, accent, accentColor, font, fontFamily, codeFont, codeFontFamily, reduceMotion, zh-TW / en / ja |
| display | options, ranking, table, heatmap, lines, mainSummary, subSummary |
| sourceWindows | global 來源紀錄範圍, 預設 24h, tabs 保留舊版匯入格式 |
| charts | 時間範圍, 長度 / 單位, 間隔, 項目數, 線數, 上限, statistics 與 shape |
| chartDefaultsVersion | 舊版圖表預設遷移標記, 接受值 1 以相容既有設定檔 |
| tables | size, page, sort, filters, hidden, columns, heatmap, heatmapCustom, open |
| summaries | 各摘要區的 count, order, hidden, titles, sources 與 custom |
| cardLayouts | 各卡片區的 custom, groups, assignments, order 與 hidden |
| contentMasking | 訊息、工具執行回覆、SQL、錯誤與 Context 的按需明細遮蔽, 預設 true |
| sqlMasking | 舊 SQL 遮蔽設定相容欄位, 與 contentMasking 同步 |
| tableSchema | 欄位相容版本, 目前為 10 |
| copy | 預設完整標籤 / tooltip → 自訂文字 |
| settings | debug_mode, activity_retention_days, max_files, track_all, observations, mcp_sources, mcp_categories, tool_descriptions, mcp_descriptions, mcp_tags |

來源紀錄開關由 MCP 管理, 不寫入可攜設定. 舊版外層的 boolean `recording` 可讀取但不套用. 拖曳開關每次載入預設關閉, 不放入可攜設定

## 驗證與套用

- contentMasking 為 boolean, 預設 true. 舊設定未提供此欄位時沿用 sqlMasking, 兩者都省略時啟用遮蔽. 同時提供時以 contentMasking 為準, 匯出保存兩欄相同值. SQL 分頁的舊開關也同步套用全域設定
- 切換內容遮蔽時立即清除目前明細及返回歷史中的本文快取, 按新模式重新讀取. 分頁 revision 包含遮蔽模式, 不混接不同模式的內容. 全部設定還原預設重新啟用遮蔽, 不重新載入整頁
- 內容遮蔽只控制支援 `mask=0|1` 的訊息與執行明細, 不改變文件選取及編輯規則, 也不解除隱藏推理、analysis 或加密內容的排除

- 設定檔最多 2 MiB, HTTP 設定 body 最多 256 KiB
- font / codeFont 12 - 24 px, codeFont 可省略或為 null 以隨介面縮放, 最小 12 px, max_files 1 - 5000. JSON 需提供有效整數, 頁面輸入四捨五入
- 舊設定的 interval 1 - 3600 秒及 idle_minutes 0 - 1440 通過格式驗證後忽略, 新匯出省略這兩個欄位. 收集由來源變更觸發, 不設定輪詢或閒置秒數
- debug_mode 為 boolean, 預設 false. 在主設定切換, 設定匯出 / 匯入與還原預設均保存此欄位. 開啟才顯示診斷面板並追加有界效能記錄, 關閉停止追加, 既有記錄保留
- activity_retention_days 0 - 3650, 預設 90, 0 不自動刪除歷史. 保存 SQL、網路、MCP、技能與錯誤的已整理紀錄, 設定與摘要一起保存, 重啟後沿用. 縮短天數分批清理過期紀錄, 快取、查詢、API 與畫面仍保留各自的讀取限制, 不以快取筆數限制 SQLite 歷史保存
- max_files / track_all 保留舊版匯入與 API 相容格式, 不再限制 session 選取, 設定畫面不提供這兩項. 所有來源分批差異讀取, 每輪讀取量仍有上限
- display.options 1 - 200, 最多 8 個不重複整數. ranking / table 必須是其中一項或 `all`
- mainSummary 1 - 8, 預設 4. subSummary 0 - 8, 預設 0 不顯示子分頁摘要, 個別摘要區可覆寫數量, 名稱與順序. 子頁 count 可為 0, 主頁 count 仍需至少 1
- appearance.reduceMotion 為 boolean, 預設 false. true 立即停用介面動畫與轉場, 保存於瀏覽器及可攜設定, 舊設定省略時沿用 false. 系統的減少動態效果設定也會停用動畫
- theme 提供 sand / workbench / neutral, 工作台保留目前配色, 名稱依語言顯示. 舊 steam / slate 設定改用 workbench
- accent 提供 theme / green / blue / orange / custom, 預設 theme 隨主題與明暗模式調整. accentColor 為六位十六進位色碼, 如 `#66c0f4`, custom 時必填. fontFamily 為本機字型清單, 最多 200 字元, 以逗號分開, 空字串使用預設字型, 字型未安裝時由瀏覽器使用後續或系統字型. 不下載字型, 不接受 URL 或 CSS 宣告
- codeFontFamily 與 fontFamily 使用相同的本機字型清單驗證, 空字串採系統預設, 兩者獨立保存
- observations.codex_cloud 為 boolean, 預設 false, 控制 Codex CLI 雲端對話的自動查詢, 關閉時可手動查詢, 與 metadata 開關控制的 Dots / 排程快取分開
- observations.public_openai / public_codex 為 boolean, 預設 false, 分別控制 OpenAI 狀態與 Codex 最新正式版本的自動查詢, 關閉時可手動查詢
- sourceWindows.global 接受 1h / 24h / 7d / all, 統一控制來源範圍. 舊版 tabs 最多 64 項, 匯入保留格式驗證, 顯示使用 global
- charts 最多 256 項, tables 最多 500 項. 每張表 columns / hidden 最多 100 個欄名, heatmap / heatmapCustom / open 為 boolean. display.heatmap 提供全域預設, heatmapCustom 記錄個別覆寫, 全域 switch 立即套用到沿用設定的表格. 套用全部經確認後清除個別覆寫, open 省略時展開
- 圖表最近長度 1 - 365, 單位分鐘 / 小時 / 天, interval 1 / 5 / 15 分鐘或 1 / 6 / 24 小時. top 最多 200, maximum 0 - 1000000000, 自訂起點需早於終點
- observations.codex_account 為 boolean, 預設 false, 控制選配官方帳戶的自動唯讀查詢, 關閉時可手動查詢, usage 停用時一併停止查詢
- shape 支援 bar / line / column / stacked / pie / donut / area. 卡片庫列出各圖表可用的形式, 趨勢可切換折線與長條圖, 固定形式的總覽卡片保留其預設
- tool_descriptions / mcp_descriptions / mcp_tags 各最多 64 項, 說明每項 400 字元. 標籤每來源最多 4 個, 各 40 字元, 空陣列還原自動標籤. copy 最多 500 項, 每項 400 字元. MCP 分類與開關各最多 64 項
- 主 Tab order 最多 50 項, 子頁 / 卡片順序 map 最多 100 個區域, 每區最多 100 個 ID. 無效或不存在的 ID 忽略

匯入先驗證並顯示套用範圍, 確認後呼叫後端, 保存 localStorage 並重新載入. 取消或無效內容保留既有設定. 匯入以 `replace_customizations` 替換自訂來源分類, 觀察開關, 標籤與說明. 一般編輯採差異更新. 前後端只接受可寫欄位, 不接受任意檔案位置或 command

全部設定還原預設以後端 `default_settings` 為基準: 來源變更時收集, 歷史保存 90 天, 所有 session 分批差異讀取, 各來源的程式預設開關, 空自訂 map, 前端預設顯示 / 排序 / 外觀 / 語言. 需先確認, 已有活動紀錄依套用後的保存策略處理

前端新預設為排行榜 5 項, 表格每頁 10 筆, 介面字級 15 px, 所有數量選擇在各卡片 / 表格齒輪內. 已保存的有效自訂值保留, 省略欄位時使用預設

## 相容與保存範圍

欄位排序使用原始 header, 自訂文字不改變識別. 新欄位或 Tab 接到既有順序尾端, 已移除項目忽略, tableSchema 處理舊欄位 index. 原 pie 設定現在直接顯示圓餅圖, donut 顯示環圈圖

localStorage 依 origin 隔離. 後端觀察設定保存於目前程序, 網頁重新連線後套用瀏覽器值. 來源紀錄狀態維持該來源設定, Jev 本機 telemetry 使用 CODEX_HOME 的 opt-in 檔. 設定檔不含活動紀錄, 原始 Log, credentials, session 本文或 Jev payload. 自訂說明, 搜尋與 filter 文字會包含在 JSON

來源紀錄範圍同時套用列表與操作統計, 來源時間缺值只在全部範圍顯示. 最新對話狀態, 累計 Token 與帳戶額度保留來源值. 舊版有效 window 值可遷移, 字級與系統字型選單即時套用並保存, 自訂色碼在確認輸入後套用, 無效內容保留已套用的值

cardVisibility 最多 500 項, 保留已有的明確顯示選擇. 省略時套用各分頁預設, 摘要項目使用 summaries 的配置, 總覽使用 overview 的配置. tableSchema 7 新增對話快取命中率欄位, 舊版排序 index 依原欄位移位

多線圖的 display.lines 與 charts.lines 接受 3 / 5 / 10, 預設 3. display.mainSummary 接受 1 - 8, 預設 4, subSummary 接受 0 - 8, 預設 0. summaries 最多 100 個區域, count 在主頁接受 1 - 8, 子頁接受 0 - 8. 省略 count 時沿用全域數量, 新指標接到既有順序尾端

## 自訂摘要與卡片布局

- summaries.order / hidden 各最多 48 個不重複識別碼, 接受內建指標序號 0 - 31 或 `custom-<UUID>` 字串. titles 每區最多 48 個名稱, 各 80 字元, sources 每區最多 48 個來源覆寫, 接受空字串、已註冊的指標來源或有效的動態來源 ID
- summaries.custom 每區最多 16 項, cardLayouts.custom 每區最多 32 項, 皆為 `{id,title,source}`. ID 必須是唯一的 `custom-<UUID>`, title 最多 80 字元, source 接受目前 `metricSources` 的來源或有效的摘要 / 欄位 / 統計 / 表格來源 ID, 來源暫時不存在時保留選擇及缺值
- cardLayouts 最多 100 個區域. groups 每區最多 32 項 `{id,title,collapsed?}`, collapsed 為 boolean, ID 唯一, 名稱不可空白且最多 80 字元. assignments 最多 500 項, 值為空字串或此區已存在的群組 ID
- cardLayouts.order / hidden 各最多 500 個不重複卡片 ID. 區域 / 卡片 / 群組 key 接受英文字母、數字、`_`、`.`、`:`、`-`, 長度 1 - 100, 沿現有驗證拒絕無效資料
- 搜尋、排序、群組及更新保留穩定 ID, 匯入不以新 UUID 取代已有 ID. 原始設定 version 仍是 1, 不要求舊檔提供新增欄位

動態來源 ID 使用 `summary:<區域>:<序號>`、`field:<卡片>:<序號>`、`stat:<圖表>:<序號>` 或 `table:<表格>:<欄位序號>:sum|mean|latest`, 原欄位及摘要順序決定序號, 顯示排序不改變來源識別

## 診斷與分享副本

一般 `kind:"settings"` 匯出完整偏好供還原, 自訂名稱、ID、搜尋及設定不被去識別化. 「匯出診斷設定」另產生 `kind:"diagnostic-settings"` / version 1, 只保存白名單的數值、boolean、內建外觀 / 來源選項及配置數量

診斷副本移除路徑、自訂名稱 / ID、搜尋、字型名稱、文字覆寫及工具名稱. 表格 / 圖表 / 卡片區域改用陣列, 不把來源物件的數值 key 當成可公開索引. 支援 CompressionStream 時輸出 `.json.gz`, 否則使用 `.json`, 本機完整偏好不變. 這份副本不供匯入還原, 分享與傳送由使用者另行操作

欄位白名單與驗證維護於 `frontend/app.js` 的 diagnosticConfiguration 與設定匯入流程. 隱私 / gzip 往返及自訂卡片驗收見[驗證紀錄](validation.md), 任務查找見 [AI 入口](../AI.md)

overview.order / hidden 各最多 500 個卡片 ID. 總覽副本以 overview-copy- 前綴保存獨立圖表設定, 統計卡使用 -statistics 後綴. 匯出保存卡片配置與摘要偏好, 活動資料仍依來源按需取得

來源變更通知決定收集與推送時機. 舊設定檔的 `interval` 欄位可讀取, API 驗證其原有範圍後略過, 新匯出不保存此欄位. 圖表的 `interval` 仍表示資料聚合區間

## API 與呈現偏好

settings.api_intervals 接受七個固定來源 codex_account、codex_cloud、public_openai、public_codex、platform_models、platform_usage、platform_costs, 每值為 60–86,400 的整數秒, 省略時預設 300. observations 的對應 boolean 控制自動查詢, 關閉時仍提供各頁查詢按鈕

preferences.presentation 保存 numberNotation (standard / compact / scientific / engineering)、tagGroups、tagOutline、badgeGroups、badgeOutline、virtualScroll、notificationBell、inputEnabled, 後七項為 boolean. numberNotation 預設 standard, 分群、虛擬捲動、鈴鐺及輸入預設 false, 外框預設 true

notificationPosition 接受 top / middle / bottom 與 start / center / end 的九種組合, 預設 bottom-end. notificationHistory 接受 0–1,000 整數, 預設 100. keyboardBindings / gamepadBindings 接受 up、down、left、right、confirm、back, 鍵盤為最多 60 字元的有效 modifier + event.code, 手把為 0–31 整數, 完整預設合併後不可重複. 無效匯入拒絕, 無效已保存 binding 回復預設, 舊設定可省略新欄位

完整設定匯出保留這些偏好, 匯入經同一驗證後重新載入, 還原預設一併重設. 診斷副本只保存內建選項、boolean 與上限, 移除使用者按鍵字串及自訂值, 通知歷史本身不保存於設定
