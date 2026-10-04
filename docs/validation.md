# 驗證紀錄

## 2026-10-04

本輪在 Windows, Python 3.14.7 與 Codex in-app browser 驗證 checkout 的 source; UI 使用隔離的 85 個 session fixture, 未變更正式 Jev 設定. 測試先設定 PYTHONPATH=src, 避免載入 .venv 內較舊的已安裝版本

| 檢查 | 結果 |
| --- | --- |
| Python unittest | 76 項通過, 1.087 秒; 包含 counters, 分類, 來源開關, MCP namespace, Reasoning / Model, 執行欄位篩選, 檔案與 Skill 路徑邊界, HTTP / CSP, 重複啟動與 watcher, 錯誤資料篩選 / 讀取上限 / 權限與 schema 失敗, lifecycle 分輪回查, 雲端 catalog 缺值與新增欄位 |
| JavaScript | Node 語法檢查通過 |
| Git diff | 空白錯誤檢查通過 |
| 表格 | 全部篩選結果先排序再分頁, 升降冪, 頁碼 Enter, 每頁數量, 全部對話分批載入, 欄位多選 / 拖曳 / 鍵盤排序與保存; 欄位移動後仍以原始欄位排序 |
| 設定與說明 | 搜尋, 儲存與回饋, 單一入口 / 兩個 tab, 零碎文案排除, 確認還原 / 取消保留, 外觀與偏好恢復, Tab 拖曳 / 方向鍵排序, 顯示數量自訂, 設定 JSON 匯出內容 / 檔案選取讀入 / 確認匯入與取消, 舊 pie 設定改為 donut |
| 明細 | 統一對話入口, 子 Tab, 返回上一個明細 / 還原位置, 檔案 metadata, MCP 來源, Skill README / 子目錄文件 / Python 檔案資訊與工具用途; Python 選取後無原始碼或 pre, 文件 credential 遮蔽; popup 外側與 Esc 關閉, sticky 導覽獨立於內容捲動 |
| Reasoning | 欄位, 升冪, 動態等級篩選與重新載入保留; fixture 提供 high / low / future-level, 明細包含 Provider / context window / 審核模式 / Git metadata |
| 圖表 | 按鈕循環切換長條 / 折線或長條 / 環圈, 專屬設定, 時間與數值刻度, 滑鼠 / 鍵盤提示; 上限截短時仍提示實際值. 排行省略刻度, 自動上限依資料最大值填滿 |
| 網路 | 工具紀錄與參考合併, 表格只顯示網址數量, 明細顯示 2 個 fixture 網址與開啟按鈕; 工具以單行名稱與來源呈現 |
| 排版 | 1265 / 1440 px 桌面與 390 px viewport, 最大 18 px 字體; 工具 / Skills 單一圖表填滿整列, 總覽依卡片高度排列並分開執行位置 / 活動來源, 全選 17 欄時自動改分欄資料列, 手機欄寬實測約 291 px, 所檢查內容沒有橫向溢出 |
| 錯誤與狀態 | 右上快速入口, 錯誤 / 警告篩選, 相關對話與返回, 錯誤觀察 switch 停用 / 啟用與套用回饋; 程式狀態顯示有上限的效能樣本與事件, 測試頁 console 無 error / warning |
| 相容語法 | Python 3.10 AST 語法檢查; 實際執行 runtime 為 Python 3.14.7 |

README 的 5 張範例圖使用隔離 fixture, 已檢視總覽, 對話明細, 錯誤紀錄, 程式狀態與設定, 沒有使用正式私人對話資料

## 本機唯讀核對

正式頁面已顯示目前這串對話為執行中, 狀態來源為 session lifecycle, 耗時持續更新. 該工作開始事件曾距尾端約 33.8 MiB, 超出初始尾端; 回查按每輪 8 MiB 共用預算分次讀取, 找到事件後停止

Codex App log 有 2026-10-04 03:32:58 (Asia/Taipei) 的對話送出失敗事件. 純 ChatGPT 對話的本機 catalog 未提供 Model, Reasoning, async status 與 token; 頁面保留缺值並說明來源, 不填入推估值

## Fixture 效能

85 個 session, 171 筆檔案操作; 初次整理 86.05 ms. 後續無新增資料時, 整理加 snapshot 深複製 10 輪的中位數 36.38 ms, 最大 39.55 ms, 額外 session 讀取量為 0 bytes

另一次啟用 tracemalloc 的記憶體檢查, Python 配置峰值 2.31 MiB; 此數值不包含整個程序 RSS. Snapshot JSON 為 392217 bytes, gzip 為 32417 bytes. 計時與記憶體檢查分開執行; 這組資料尚不足以證明 5000 個 session 的常駐效能

## 執行環境與驗證範圍

本機 HTTP 回應會經 AdGuard 修改, 曾出現間歇性連線重設; 已將當前 CSS / JS 內嵌至同一份 HTML 並核對 CSP, 使用 HTTP/1.1 連線重用與 timeout. 後續測試頁面正常載入; 保留原有網路防護

已執行的自動化與 UI 操作以隔離 fixture 為主; 5000 個 session, 原生 macOS / Linux launcher, live provider 與付費 API 未執行本輪驗證. Tab / 欄位拖曳完成後的位置與保存已驗證, 拖曳途中提示以程式碼檢查, 沒有截取中途畫面

設定 JSON 匯出內容與匯入操作已驗證, IAB 的 download 等待未取得實體下載檔, 頁面保留可複製 JSON. 活動資料與圖片匯出仍保留在規劃文件
