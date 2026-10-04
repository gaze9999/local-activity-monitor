# 設定檔格式

設定頁的「設定檔」匯出 / 匯入整個程式的使用者偏好, 可跨電腦, 瀏覽器與 port 使用

## Version 1

外層欄位為 `application: "local-activity-monitor"`, `kind: "settings"`, `version: 1`, `exported_at`, `preferences` 與 boolean `recording`

| preferences 欄位 | 保存內容 |
| --- | --- |
| tab / tabOrder | 選定 Tab 與拖曳順序 |
| inputs / page | 搜尋, 分類 filter, Jev window, 對話頁碼 |
| appearance | mode, theme, accent, font |
| display | options, ranking, table |
| charts | 依 chart ID 保存時間區間, 長度 / 單位, 間隔, 顯示項目, 數值上限與圖表類型 |
| tables | 依 table ID 保存 size, page, sort, filters, hidden, columns |
| tableSchema | 表格欄位相容版本, 目前為 6 |
| copy | 原始完整介面文字 → 使用者文字 |
| settings | interval, max_files, track_all, observations, mcp_sources, mcp_categories, tool_descriptions |

`recording` 保存 Jev enabled 狀態, 使用目的電腦的本機設定與資料庫位置, 不搬移資料庫路徑

## 驗證與套用

- 設定檔最多 2 MiB, HTTP settings body 最多 256 KiB
- 字體 12 - 18 px, interval 1 - 3600 秒, max_files 1 - 5000
- display.options 1 - 200, 1 - 8 個不重複整數; ranking / table 必須是其中一項或 `all`
- charts 最多 100 個, tables 最多 500 個; 每張表的 columns / hidden 最多 100 個名稱
- chart 最近長度 1 - 365, 單位分鐘 / 小時 / 天, interval 1 / 5 / 15 分鐘或 1 / 6 / 24 小時; 顯示項目最多 200, 數值上限 0 - 1000000000, 自訂時間起點需早於終點
- tool_descriptions 最多 64 項, 每項 400 字元; copy 最多 500 項, 每項 400 字元; MCP 分類與開關各最多 64 項

匯入先驗證, 顯示套用範圍與 Jev enabled 狀態, 確認後呼叫後端並保存 localStorage, 最後重新載入. 取消不套用, JSON / 版本 / 數值無效時保留既有設定並顯示錯誤

匯入使用 `replace_customizations` 整批替換 MCP 自訂分類 / 開關及工具說明, 一般頁面編輯採增量更新. 前後端皆驗證可寫欄位, 不接受任意檔案路徑或 command

## 更新與換電腦

排序與欄位順序使用原始 header 名稱, 自訂文案不改變識別. 新欄位自動接到保存順序尾端, 移除的欄位忽略; 舊表格 index 依 tableSchema 遷移, 保存原始 header 的排序優先按 header 對應

舊設定 `shape: "pie"` 改成 `donut`; 趨勢支援 bar / line, 分布支援 bar / donut. 新 Tab 自動接在保存順序尾端; 沒有來源的 MCP / Jev / Web Tab 依實際可用性隱藏. 目的電腦不支援的觀察 key 或分類不送到後端, 新來源依現有設定與 session 自動發現

localStorage 依 origin 隔離. 後端設定存在目前程序, 網頁重新連線後套用已保存值; Jev enabled 使用 CODEX_HOME 的 opt-in 檔, 其他偏好需以設定檔攜帶

## 保存範圍

設定檔不包含 token 用量, 工具 / 對話事件, 原始 log, credentials, session 本文或 Jev payload. 使用者自行填寫的工具說明 / 介面文字, 搜尋文字及 filter 值會包含在設定檔, 分享前可檢查 JSON 預覽

活動紀錄與圖表圖片的匯出規劃見 [活動匯出規劃](export-plan.md)
