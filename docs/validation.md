# 驗證紀錄

## 2026-10-04 至 2026-10-05, 0.2.2

Windows, Python 3.14.7 與 Codex in-app browser. 使用隔離的 85 個 session, 11 個 MCP 來源, Jev 與 SQL 示範資料. 沒有使用正式對話內容或改寫正式來源設定

- 125 項 unittest 通過, 1 項大型 HTTP debug 測試因標準函式庫對照服務的傳輸逾時而跳過. 新增 7 項來源測試, 覆蓋實際 schema / 路徑 / 讀取量, 缺少及不支援格式, 未知 telemetry 來源, 0 值, 停用狀態, MCP 暫停讀取不使快照失敗, 來源資訊不額外讀取檔案內容與不包含私人欄位. 重現並修正低頻 MCP 回應 / 工具 / 紀錄狀態被 1,000 筆表格上限遮掉, automation 查詢未限定範圍與未報告讀取量, checkpoint 0 筆無法區分讀取失敗
- Node 語法, Python 3.10 AST 與文件相對連結通過. en / ja 的 1321 個 key 一致, 新增來源描述保留日文標點
- 依最新決定, 實際確認新偏好為 14 px, 有效的 16 px 自訂偏好保留, 還原設定後實際回到 14 px, 排行 5 項, 表格每頁 10 筆. 所有主頁表格在資料充足時顯示 10 筆. MCP 排行的 5 / 10 / 20 / 全部位於齒輪內, 原先卡片上的數量選單已移除. 實際切換全部及重整後保留選擇
- 各主頁的來源說明初始收合且未建立詳細 DOM, 標題持續顯示最近更新時間. 各來源全寬排列且獨立展開, 更新後保留各自展開狀態. Catalog 實際顯示 85 列主查詢, automation_runs / automations 各 1 列與 85 個選取 Thread IDs, checkpoint 分列載入 / 保存狀態. 展開後顯示實際位置, 欄位 / 觀察項目, 程式上限, 讀取結果與保留量. 單一 MCP 頁不列其他來源的 telemetry. Jev 範圍從 24 小時切換為 1 小時後, 來源區塊顯示實際範圍及 0 筆明細 / 趨勢點
- 實際切換 zh-TW / en / ja, 檢查設定與詳細來源文案. 1280 px 桌面與 390 px 窄螢幕沒有頁面橫向溢出. 390 px 使用 18 px 字級時, 圖表設定採單欄且 modal 沒有橫向溢出, 長來源路徑可換行. 保留原有表格容器內捲動
- 工具頁新增統計表格, 直接呼叫與程式碼辨識分列, 實際確認每頁 10 筆, 數值排序與篩選為程式碼辨識後的 2 筆紀錄. 1280 / 390 px 逐一檢查 11 個主頁完成渲染後的表格筆數與 SVG 文字邊界, 沒有頁面橫向溢出或 SVG 文字越界
- Tag 使用共用分類色與狀態色, 已完成 / 已設定 / 正常使用成功色, 缺值使用中性色, 保留文字. 修改後的測試頁面沒有新增 console error

textlint MCP 兩次檢查 8 份 Markdown, 台灣規則提示從 20 項降至 8 項, 剩下是 inline code 邊界的斷句誤判, 已逐項核對. 新增資料盤點文件回傳 0 項提示. 繁中 UI literals 與英文語系檔抽取文字的通用規則回傳 0 項提示, 不代表英文完整文法檢查. 日文全部字串抽成段落時產生大量標籤句尾誤報, 保留短標籤, 人工核對非句尾規則並修正冗詞與標點. 針對來源, 額度, 傳輸, 未分類, 平均及 P1 等 6 段完整 tooltip 再使用日文規則檢查, 回傳 0 項提示

大型回應 debug 使用大於 64 KiB 的合成內容, 比較標準 HTTP 服務與實際 handler 的未壓縮 / gzip 完整性. 此環境的標準服務先發生 TimeoutError, 因此 handler 大型回應驗證跳過, 不能記為通過. 既有本機服務的 /api/instance 正常, /api/snapshot 回應標頭可取得, 內容接收逾時, 完整即時資料盤點受此限制. 沒有據此修改壓縮策略或停止既有服務

重跑 debug: Windows 設定 `$env:PYTHONPATH="src"` 後, 執行 `python -m unittest discover -s tests -p test_http_snapshot.py -v`. 其他系統設定 `PYTHONPATH=src` 再執行相同測試. 此測試只啟動與關閉自己的 loopback 合成服務

效能比較使用相同 85 個 session fixture, 修改前程式取自 ae8bdc4, 每次初始化後量測 20 次 refresh + snapshot. 最終程式的兩組量測改變先後順序, 更新耗時中位數為修改前 66.8 / 66.9 ms, 修改後 97.4 / 60.2 ms, CPU 中位數為修改前 62.5 ms, 修改後 62.5 / 46.9 ms. 最後一組冷啟動各為單次樣本, 修改前 209 ms / 125 ms CPU, 修改後 199 ms / 156 ms CPU. 波動不足以判定加速或退步. session 增量讀取皆為 0 bytes, 最後一次 snapshot gzip 大小從 34,596 增至 36,495 bytes, 增量約 1.9 KiB, 來自來源與摘要 metadata. 未重跑 5000 個 session 的效能測試

設計依據與後續驗收規則見 [維護與驗收](maintenance.md). 版本與套件驗證依既有 release workflow 執行, 各平台結果見 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml), 實際發佈產物見 [v0.2.2](https://github.com/gaze9999/local-activity-monitor/releases/tag/v0.2.2). v0.2.1 的已發佈驗證保留如下

## 2026-10-04, 0.2.1

Windows, Python 3.14.7 與 Codex in-app browser. 沿用隔離 session / Jev fixture, 新增可提供 credits 的未知 MCP 與 SQL 診斷紀錄. 沒有讀取正式對話內容或改寫來源設定

- 118 項 unittest 通過, 2.528 秒. 包含按需 SQL 身份 / 路徑 / HTTP 邊界, literal 解析, 多筆 INSERT 憑證遮蔽, 內容上限與 snapshot 不保存 SQL
- Python 3.10 AST, en / ja 1264 個 key, README 與治理文件連結通過. 三個角色 TOML 符合必要欄位及 sandbox 設定, 保留 model / effort 繼承. 本機角色檔解析不代表目前對話已重新載入
- 實際頁面確認 SQL 內容按需顯示 SELECT 1, MCP 使用明細涵蓋 11 個來源, 舊排行設定更新為前 5 名, 5 / 10 / 20 / 全部可直接選擇並保存
- 觀察來源位於四張摘要卡下方, 預設收合, 展開選擇可在重整後保留. 卡片, 排行及 modal 的內部連結定位來源頁首. 未觀察到連線狀態時不顯示尚未確認 tag
- 動態模型篩選實際只保留選定模型, 重整後保留. error / info 與 xhigh 使用英文及不同顏色. 其餘等級的顏色以共用 CSS 規則檢查
- 主頁排序關閉時不顯示拖曳提示, 開啟後顯示, 關閉後隱藏. modal 開關維持獨立. 實際切換 en / ja, 檢查數量單位, 設定分類與分頁控制, 修正保留舊語言及錯誤單位
- 1280 px / 390 px 與來回縮放檢查 MCP 卡片, 數量選單, 來源摘要及監測卡片, 沒有頁面橫向溢出或卡片重疊. 多欄配置固定欄位順序, 不因卡片高度變更交換左右. 桌面表格保留容器內橫捲
- 測試頁面沒有新增 console error / warn. 這次沒有重跑 5000 個 session 效能測試或實際指標拖曳手勢

## 2026-10-04, 0.2.0

本機使用 Windows, Python 3.14.7 與 Codex in-app browser. UI 使用隔離的 85 個 session fixture 與 4 筆 Jev telemetry, 預設載入 20 個近期檔案. 範例圖片只包含示範資料. 未變更正式 MCP 或 Jev 設定

| 檢查 | 結果 |
| --- | --- |
| Python unittest | 114 項通過, 3.941 秒. 涵蓋 collector metadata, lifecycle 分輪回補與 checkpoint, Skills 去重與重啟, 錯誤保留 / 遮蔽 / 上限, SQL literal 辨識, 用量快照, 未知 MCP 指標與 tags, 並行呼叫狀態, Jev 未知操作與損壞欄位隔離, HTTP / CSP / 來源設定 |
| JavaScript | Node 語法檢查通過, 已載入的測試頁面沒有新增 console error |
| Python 相容語法 | 16 個後端與建置入口通過 Python 3.10 AST 語法檢查, 本機實際執行版本為 3.14.7 |
| 設定 | 數量四捨五入, 更新最小 1 秒, MCP 自訂標籤匯入 / 匯出 / 還原, 舊設定的 recording=false 不覆寫來源, 全部還原保留來源紀錄狀態 |
| 導覽與排序 | Skills 獨立主 Tab, MCP 動態來源子頁, Log 子頁, 單列捲動導覽. Modal 與主頁排序開關分開, Alt + 方向鍵可移動並保存 |
| 表格 | 主頁與子頁依資料來源呈現, 預設每頁最多 20 筆, 篩選預設收合. 對話搜尋 / 清除與分頁保留, 名稱入口與整列入口不重複 |
| 圖表 | 直條 / 堆疊有刻度與單位, 標籤按寬度整組傾斜與省略, tooltip 保留完整文字. 圓餅 / 環圈資訊另列. 額度 / 分類沒有平均 / P99 / P1, 耗時 / 趨勢保留有效統計 |
| MCP | 來源按設定與工具紀錄發現, 未知來源有共用操作與指標區塊. 本機呼叫 / 回傳 / 傳輸失敗提供被動連線狀態, 不送出 MCP 探測呼叫 |
| 明細 | 錯誤的 HTTP code, Request / Trace / Call ID 與關聯對話. MCP HTTP 嘗試, 已取得的 credits / tokens / 大小. 缺值為 --, 已確認沒有錯誤為無, 已確認的零計數為 0 |
| 排版 | 1265 px 桌面與 390 px viewport, 14 / 18 px 字體. 所檢查圖表, 監測卡片與 modal 沒有橫向溢出. 單位避開峰值, 日期 / tooltip / modal 排序開關保留在範圍內 |

圖表與卡片拖曳的開關, 區域邊界與指標處理以程式碼檢查, 排序與保存以鍵盤操作驗證. 本輪沒有實際操作指標拖曳手勢

## 需求回顧

- 總覽依重要性預設顯示 12 個區塊, 帳戶用量與剩餘額度分開, 相同資料預設只保留一種圖表. 活動摘要獨立排序
- 設定改為直接分類, MCP 來源可搜尋與編輯用途 / 標籤. 來源紀錄狀態由來源決定, 沒有重複的 Jev 開關
- MCP 依目前取得的欄位顯示專有指標與本機紀錄, 未知操作 / 狀態保留. 只有全部來源頁顯示完整來源卡片, 資料來源說明只留在各頁
- 文字精簡, 以台灣用語與半形標點呈現. 必要解釋置於 tooltip, modal 關閉不重新彈出原提示. 成功通知短暫浮動, 還原預設只在有自訂值時顯示
- 對話狀態與 Skills 有上限的保存與回補. SQL 只計可辨識的工具操作與診斷, 不把來源未記錄的 Codex 內部 SQLite 讀寫當成零次使用

## Fixture 效能

85 個 session, 170 筆檔案操作, 4 筆 Jev 紀錄. 初次整理 513.53 ms. 後續無新增資料時, 整理與 snapshot 深複製共 20 輪, 平均 41.65 ms, P99 46.05 ms, P1 39.12 ms, 中位數 41.54 ms, 最大 46.09 ms. 額外 session 讀取量為 0 bytes

同批增量程序 CPU 時間平均 40.62 ms, P99 46.88 ms. 另行啟用 tracemalloc 的 Python 配置峰值為 2.4 MiB, 此值不包含程序 RSS. Snapshot JSON 412183 bytes, gzip 35187 bytes. 這些結果來自指定 fixture, 尚未驗證 5000 個 session 的常駐效能

## 可攜套件驗證

[release workflow](../.github/workflows/release.yml) 在 Windows x64, macOS x64 / ARM64, Linux x64 / ARM64 建置. 每個原生 job 使用 Python 3.12 執行 unittest, 包含 Python runtime 與第三方授權文件, 再以空白暫存 home 啟動成品, 核對 HTTP 頁面, CSP, 語系資源, 版本, snapshot 與檔案存取邊界. 通過後產生原生壓縮檔, wheel, sdist, source archive 與 SHA256SUMS

建置只從該 workflow 的 Git commit 取檔, 不包含本機 .local, 狀態資料庫或私人活動紀錄. 所有 job 通過才附加到 draft release. 原生建置的實際結果見 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml)

本機獨立 HTTP smoke check 遇到 AdGuard 修改回應後讀取逾時, 因此本機結果不能確認原生套件啟動. 本機 UI 已在瀏覽器驗證, 原生套件啟動由各平台的隔離 CI 檢查

## 其他驗證範圍

本輪沒有呼叫 live provider 或付費 API, MCP 連線狀態由本機紀錄取得. macOS / Linux 桌面雙擊與簽章提示未在本機操作. 設定 JSON 內容與匯入已驗證, IAB download 等待未取得實體下載檔, 頁面仍提供可複製 JSON
