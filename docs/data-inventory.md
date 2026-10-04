# 可觀測資料盤點

盤點日期為 2026-10-04 至 2026-10-05, 依本輪工作區程式與隔離 fixture 核對來源, 解析, API 與介面. 下列範圍描述可取得的資料及明確限制, 不代表每個正式 provider 都已完成實機驗證

## 資料語意

- `null` 或未提供欄位表示來源沒有可用數值, 介面使用 `--`. 已確認的 `0` 與 `false` 保留原值
- Token 使用各對話最後讀取到的累計值, 不加總歷次快照. Cached input 已包含在 Input, 不再次加入 Total
- 額度使用來源最後回報的 `updated_at`, 視窗長度, 使用比例與 `resets_at`. 剩餘比例為 `100 - used_percent`, 到達重設時間後等待新的來源回報, 不自行推估已重設
- 排行預設 5 項與表格每頁 10 筆是顯示設定. 分頁不增加來源讀取量, 也不表示只收集這些筆數
- 來源讀取預算, 記憶體保存上限, API 回傳上限與來源本身的歷史總量分開理解. 統計只使用已載入且符合範圍的資料

## 來源與欄位

| 來源 | 可取得的欄位與用途 | 讀取及保存範圍 | 具體取得限制 |
| --- | --- | --- | --- |
| Codex session JSONL | Thread ID, 時間, Model, Reasoning, 執行設定, lifecycle, 最新 Token, 工具呼叫與回傳, literal 操作 metadata | 初次讀檔頭及檔尾, 後續增量讀取. 檔尾預設 1 MiB, 可依設定變更. 一般 refresh 共用 8 MiB 讀取預算, 包含增量讀取及 lifecycle / 錯誤歷史的分批回補. 預設追蹤 20 個最近檔案, 全部模式最多 5,000 個, 保留工具呼叫最多 50,000 筆, 總暫存上限 8 MiB | 不讀取完整對話內容作為監測資料. 檔案輪替, 上限及尚未回補的歷史可能造成缺漏. 檔案清單最多列出 20 個實際位置, 不是追蹤檔案上限 |
| Codex / ChatGPT catalog | 對話名稱, 建立及更新時間, 類型, 主機位置, 專案, Model, Reasoning, catalog 狀態, 排程關聯 | `local_thread_catalog` 最近 2,000 列, automation 關聯只查已選 Thread IDs, 分批彙整為每個 Thread 一列並分開報告查詢欄位與筆數, `state_*.sqlite` 以已選 Thread IDs 分批查詢, 每批最多 400 個 ID, 使用支援的最新 state DB | SQLite 使用唯讀連線, 只選需要的欄位. 缺少 schema 或欄位時無法補齊. 不讀取私人 prompt / response 欄位 |
| Session index 與 app state | index 補充對話名稱, app state 補充專案名稱與 membership, Dot Thread ID, artifact 類型與產生時間 | index 讀取最後 1 MiB. app state 檔案上限 16 MiB, Dot outputs 最多檢查 2,000 個, 回傳最近最多 500 個去重事件 | 大型或格式不支援的 app state 會回報健康狀態. 不回傳其他 app state 或私人路徑 |
| Lifecycle / Skills checkpoint | 已確認 running / completed, lifecycle 時間與 offset, Thread / Call / Skill 識別碼與讀取時間 | lifecycle 最多 1,000 個檔案項目, Skills 最多 500 筆, checkpoint 最多 512 KiB. 依穩定識別碼去重, 重啟後沿用有效快取 | 只保存已確認事件. Skills 讀取不等於實際使用. 未追蹤到的 Thread, 保存上限與快取不可用會限制可恢復資料 |
| 帳戶用量與額度快照 | limit ID, plan type, primary / secondary 視窗, used / remaining percent, reset time, credits balance / has_credits / unlimited | 從 session 的 `token_count.rate_limits` 選取最近回報的一份有效快照, 不另呼叫帳戶 API | 不等於所有帳戶或 provider 的即時總用量. 來源未回報的欄位維持缺值, 不能由對話 Token 推算費用或餘額 |
| MCP 設定及工具紀錄 | 動態來源, 啟用狀態, 用途, 分類與 tag, 工具, 操作, Thread / Call ID, 時間, 回傳狀態, 錯誤, 耗時, 識別碼與有單位的指標 | 未知來源可從設定或已觀察事件加入. 摘要以保留事件計算, API 事件列表最多 1,000 筆. 每次結果投影有欄位數與型別限制 | exec 辨識僅表示程式碼中出現呼叫位置. 混合工具回傳不當成個別 MCP 的結果. 不保留任意 payload, credentials 或無來源定義的數字 |
| MCP 專有指標 | 來源提供的 Token / credits / bytes / 延遲 / 重試 / 快取 / 驗證 / 文件 / 瀏覽器等有型別指標, 支援帶 `unit` 與 `value` 的欄位 | 共用結果投影解析 `metrics`, `statistics`, `usage`, `performance` 等結構, 對候選欄位及輸出數量設上限 | 自動投影只涵蓋可辨識單位與型別, 不保證所有來源任意巢狀欄位都會出現. 新格式仍需要 fixture 與明確投影規則 |
| MCP 被動連線觀測 | 最近回應時間, 尚待回應的直接呼叫數, 來源提供的連線 / 認證診斷錯誤 | 根據已載入工具事件與診斷紀錄, 不發送探測請求 | 有回應表示該時間曾收到回應, 不等於目前可即時連線. 沒有證據時省略連線 tag, 不消耗額度來確認 |
| Jev 本機 telemetry | 操作數, HTTP attempts / retries, Token 已知及缺值數, 傳輸大小, 平均延遲, status 分布, 最近 metadata 與通用指標 | 唯讀查詢所選 1 小時 / 24 小時 / 7 天 / 全部視窗. 聚合統計使用視窗內符合條件的資料, 最近 metadata 最多 80 列, 小時序列最多 168 個時間格, status 分組最多 40 個 | 80 列是最近明細查詢上限, 不是聚合統計的來源上限. metadata 超過 65,536 字元或格式不支援會略過, 每筆最多顯示 16 個 attempts. 來源未啟用或 DB 不可用時保留實際健康狀態 |
| Git / Skills / 驗證 / 檔案 / 網路 | literal Git 操作與 repository, Skills 文件讀取, test / build / lint, 檔案操作及位置, 網路工具與參考網址 | Git 回傳最近 500 筆, Skills 保留及回傳最多 500 筆, 驗證回傳最近 500 筆, 檔案回傳最近 1,000 筆. 網路事件使用 MCP 保留列表. 活動序列最多 10,080 個時間格, 工具序列最多 20,000 個工具時間格 | 工具回傳不等於命令成功. 各類摘要與表格可能有不同保存範圍, 不從最近明細推論所有歷史操作 |
| SQL / SQLite metadata 與按需內容 | SQL 操作類型, statement 類別, engine, 可確認的 DB 位置, 外層工具時間, 個別 SQL 時間, rows_affected / rows_returned, 來源與紀錄識別碼 | session literal 辨識與診斷 SQL 合併後回傳最多 500 筆. 點開已觀察的紀錄才讀取內容. session 內容限制在已追蹤檔案的設定檔尾範圍, SQL 先限制處理 65,536 字元, 經遮蔽後最多顯示 32,768 字元 | 不執行 SQL, 不讀取查詢結果. 舊紀錄仍有 metadata, 但移出檔尾或來源變更後可能無法取得內容. 個別 SQL 耗時及列數僅使用診斷來源提供的值, 不以外層工具時間替代 |
| Desktop / Core 診斷與 Log | severity, module, code, Thread / Call / Request / Trace ID, 檔案及 record ID, 錯誤摘要, 關聯 SQL | Desktop 最多追蹤 8 個檔案, 初次各檔取最近 256 KiB, 增量與歷史分別使用 1 MiB 預算. Core 每次增量及歷史查詢各最多 2,000 列. 錯誤歷史回補範圍 24 小時. 診斷錯誤與 Log 各保留最多 1,000 筆, SQL 診斷最多 500 筆 | 每次查詢上限不是 DB 總量上限. 不支援格式, 過長行, 截斷及尚待回補會影響範圍. Log 等級保留來源英文值 |
| 程式 runtime 與保存資訊 | 版本, 系統, Python, PID, 開啟 / 更新 / 運行時間, refresh / CPU 時間, 讀取量, HTTP 次數及錯誤, 回應及壓縮大小, 保存上限與來源健康狀態 | 效能樣本最多 360 筆, 狀態事件最多 200 筆, 程式 Log 最多 1,000 筆. journal 每份 64 KiB 並保留一份輪替檔. 錯誤 metadata checkpoint 最多 1,000 筆及 512 KiB | 效能樣本與狀態事件存在記憶體, 重啟後重新累積. journal 與錯誤 checkpoint 可跨重啟保存, 不包含完整對話或工具 payload |

Subagents, 專案及封存頁只呈現已載入 metadata, 父子關聯與專案對話數不代表全部帳戶資料. 最新狀態, 累計 Token 與額度不套用操作事件時間篩選, 時間未知的事件只在全部範圍顯示

專案資料庫最多讀取 2,000 個專案與 5,000 個 root 關聯, 每個專案最多 32 個設定資料夾. 同一 Project ID 的名稱與資料夾優先使用資料庫, 舊 app state 只補缺值. 封存狀態使用來源的明確 boolean, 不從對話狀態猜測. 專案 icon 與本地 / 雲端類型需來源實際提供, 缺少時保留文字與未知類型

工具統計在表格截斷前, 使用所選範圍內的完整保留呼叫計算次數, 涉及對話, 已回傳數, 耗時樣本, 平均與 P99, 並列出最近呼叫與回應時間. 程式碼辨識的內層工具沒有獨立時間時保留缺值. Skills 計數與 Dot 產出總數也先套用範圍再截斷明細, 不以表格筆數取代總數

監測效能樣本與執行事件依所選時間範圍顯示, 圖表平均與 P99 使用範圍內保留樣本. 最新更新耗時, runtime, 已執行時間與累計 HTTP / 讀取計數保留最近程式狀態. 一份快照的範圍投影不增加 session 或資料庫讀取

實際來源位置, 讀取健康狀態, 已選欄位, 當次行數與上限由 snapshot 的來源資料提供. 頁尾預設收合, 展開只整理已載入 metadata, 不再次掃描來源

## 按需明細

- SQL 內容由已觀察紀錄 ID 綁定來源, 無法取得時保持缺值, 不允許任意檔案或任意查詢
- Jev 呼叫送出與回傳內容只在點開已觀察呼叫時讀取, 遮蔽可辨識 credentials. 混合 exec 回傳不當成 Jev 單一結果
- 專案資料夾由已載入來源設定按需取得. Global / Project AGENTS.md 只從選定根目錄讀取, 最多 32 個根目錄, 每份最多 64 KiB, 不走訪任意檔案. 拒絕非絕對路徑, 網路路徑與 symlink, 遮蔽可辨識 credentials. 不加入 snapshot / checkpoint
- Git 明細綁定已觀察的 Thread / Call / 操作, 按既有檔尾與讀取預算取得指令及工具回傳. 不執行內容, 多個操作共用回傳時保留外層回傳範圍, 不推論個別命令成功
- Skills 文件只在點開已辨識文件時讀取, 檔案清單最多 200 個, 走訪最多約 1,000 個項目. 單份文件最多讀取 256 KiB 並顯示前 32,768 字元. 重啟保存的 Skill 事件不保存文件路徑與內容

## 本輪修正與驗證

下列項目已修正並以重現測試驗證, 結果見 [驗證紀錄](validation.md)

| 項目 | 已確認影響 | 已完成修正 |
| --- | --- | --- |
| MCP 摘要被表格截斷資料覆蓋 | 低頻來源最後回應落在全域最近 1,000 筆之外時, connection 可能退回 unconfirmed | 在截斷前計算完整保留事件摘要, 合併來源診斷, 不從表格列表重算 |
| Automation 額外讀取未列入來源說明 | Catalog 頁尾未反映 automation_runs / automations 的查詢欄位與讀取量 | 只查已選 Thread IDs, 分批彙整, 分開回報每個查詢 |
| Checkpoint 缺少載入健康狀態 | 空快取無法區分沒有紀錄與讀取失敗, 難以追查重啟計數缺漏 | 保留實際讀取與寫入健康狀態, 直接投影到來源說明 |
| 時間範圍與明細上限順序 | 近期事件可能在截斷前被舊資料排除, 監測圖表仍含範圍外樣本 | 工具, Skills, Dot, Log 與監測樣本先依範圍投影, 最新狀態與累計數保持來源語意 |
| MCP 紀錄旗標只顯示特定來源 | 未知 MCP 的 false 旗標與已知零指標可能遺失 | 共用 boolean / 有型別指標投影, 保留零值, 多個旗標, 確認時間與各自健康狀態 |
| 新舊專案同 ID 名稱相互覆蓋 | 舊 app state 可能蓋掉資料庫中的新名稱 | 已確認的 Project ID 對應優先使用資料庫名稱與 roots, 舊來源只補缺值 |

盤點採程式核對及隔離 fixture. 最終本機測試共 169 項, 168 項通過, 1 項大型 HTTP 對照傳輸測試因本機環境逾時跳過. 回歸測試涵蓋來源上限, 各時間範圍, 未知 MCP 旗標, 專案與封存欄位, 按需指示文件及 Git / Skills 內容邊界, 結果見驗證紀錄. 未對正式 provider 執行全面 runtime 驗證, 未呼叫會消耗額度的連線探測

本機既有監測服務的識別端點有回應, 較大型快照的回應內容接收逾時. 獨立 Python 標準 HTTP 服務也重現大型內容傳輸失敗, 目前無法確認這份正式快照的即時完整性. 正式快照未完整解析或保存, 隔離 fixture 的 API 與 UI 驗證不替代這項檢查

## 程式依據

- [Session, Jev 與按需內容](../src/local_activity_monitor/collectors.py)
- [Catalog, index 與 app state](../src/local_activity_monitor/codex_metadata.py)
- [Lifecycle / Skills checkpoint](../src/local_activity_monitor/thread_state.py)
- [用量與額度投影](../src/local_activity_monitor/usage_records.py)
- [MCP 動態來源, 結果投影與摘要](../src/local_activity_monitor/mcp_records.py)
- [SQL metadata 與內容遮蔽](../src/local_activity_monitor/sqlite_records.py)
- [診斷讀取與錯誤摘要](../src/local_activity_monitor/error_records.py)
- [Runtime 與 journal](../src/local_activity_monitor/monitor_state.py)
- [錯誤 checkpoint](../src/local_activity_monitor/error_history.py)
- [API, 合併資料與動態來源範圍](../src/local_activity_monitor/server.py)
