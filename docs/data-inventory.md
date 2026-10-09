# 可取得資料盤點

資料由本機紀錄、設定與唯讀 SQLite 提供, 下列欄位供 collector、API 與介面使用

回補 metadata 保存於 `CODEX_HOME/monitoring/lam-history.sqlite3`, activity / errors / threads 分區各自以交易更新. 既有 JSON 首次讀取時經欄位投影後匯入並保留原檔. 大小上限計算投影資料, SQLite 檔案另有頁面及索引空間. 程式 journal、來源紀錄與遷移備份在 CLI 結束後保留

## 資料語意

- `null` 或未提供欄位表示來源沒有可用數值, 介面使用 `--`. 已確認的 `0` 與 `false` 保留原值
- Token 使用各對話最後讀取到的累計值, 不加總歷次快照. Cached input 已包含在 Input, 不再次加入 Total
- 平均 Token / 秒使用對話最新累計 Token 除以已記錄工作秒數, 工作時間包含工具執行與等待. 缺少 Token 或正數工作時間時顯示 `--`
- 額度使用來源最後回報的 `updated_at`, 視窗長度, 使用比例與 `resets_at`. 剩餘比例為 `100 - used_percent`, 到達重設時間後等待新的來源回報, 不自行推估已重設
- 排行預設 5 項與表格每頁 10 筆是顯示設定. 分頁不增加來源讀取量, 也不表示只收集這些筆數
- 來源讀取預算, 記憶體保存上限, API 回傳上限與來源本身的歷史總量分開理解. 統計只使用已載入且符合範圍的資料

## 來源與欄位

| 來源 | 可取得的欄位與用途 | 讀取及保存範圍 | 具體取得限制 |
| --- | --- | --- | --- |
| Codex session JSONL | Thread ID, 時間, Model, Reasoning, 執行設定, lifecycle, 最新 Token, 工具呼叫與回傳, literal 操作 metadata | 掃描全部可取得的 session 檔案, 由檔頭分批讀取, 後續依已讀取位置做差異讀取. 每份來源每輪最多 1 MiB, 一般 refresh 共用 8 MiB 讀取預算, 包含差異讀取及 lifecycle / 錯誤歷史的分批回補. 不以追蹤檔案數量篩選來源, 記憶體保留工具呼叫最多 50,000 筆, 總暫存上限 8 MiB | 不讀取完整對話內容作為監測資料. 檔案輪替, 上限及尚未回補的歷史可能造成缺漏. 檔案清單最多列出 20 個實際位置, 不是追蹤檔案上限 |
| Codex / ChatGPT catalog | 對話名稱, 建立及更新時間, 類型, 主機位置, 專案, Model, Reasoning, catalog 狀態, 排程關聯 | `local_thread_catalog` 最近 2,000 列, automation 關聯只查已選 Thread IDs, 分批彙整為每個 Thread 一列並分開報告查詢欄位與筆數, `state_*.sqlite` 以已選 Thread IDs 分批查詢, 每批最多 400 個 ID, 使用支援的最新 state DB | SQLite 使用唯讀連線, 只選需要的欄位. 缺少 schema 或欄位時無法補齊. 不讀取私人 prompt / response 欄位 |
| Session index 與 app state | index 補充對話名稱, app state 補充專案與 membership、Dot 產出、遠端對話與近期活動電腦 | index 讀取最後 1 MiB. app state 檔案上限 16 MiB, Dot outputs 最多檢查 2,000 個, 回傳最近最多 500 個去重事件. 遠端摘要每個來源最多 1,000 筆, 活動快取最多 200 組, 每組最多 1,000 筆, 電腦最多 100 個 | 排程定義與目前連線狀態需另有讀取入口. 只回傳所選 metadata |
| Lifecycle / Skills checkpoint | 已確認 running / completed, lifecycle 時間與 offset, Thread / Call / Skill 識別碼與讀取時間 | 記憶體工作集合最多 1,000 個 lifecycle 項目、500 筆 Skills 與合計 512 KiB, SQLite 持久化不依筆數刪除. Skills 活動預設保存 90 天, 設定 0 時不自動刪除, 最新 lifecycle checkpoint 屬於來源恢復狀態, 不套用活動期限 | 只保存已確認事件. Skills 統計檔案讀取事件. 來源資料缺漏與快取不可用會限制可恢復資料 |
| 帳戶用量與額度快照 | limit ID, plan type, primary / secondary 視窗, used / remaining percent, reset time, credits balance / has_credits / unlimited | 從 session 的 `token_count.rate_limits` 選取最近有效快照. 選配官方帳戶查詢使用已登入 Codex CLI, 至少快取 60 秒 | 欄位及時間依最近來源回報, 未提供時保持缺值 |
| MCP 設定及工具紀錄 | 動態來源, 啟用狀態, 用途, 分類與 tag, 工具, 操作, Thread / Call ID, 時間, 回傳狀態, 錯誤, 耗時, 識別碼與有單位的指標 | 未知來源可從設定或已觀察事件加入. 摘要以保留事件計算, API 事件列表最多 1,000 筆. 每次結果投影有欄位數與型別限制 | exec 辨識僅表示程式碼中出現呼叫位置. 混合工具回傳不當成個別 MCP 的結果. 不保留任意 payload, credentials 或無來源定義的數字 |
| MCP 專有指標 | 來源提供的 Token / credits / bytes / 延遲 / 重試 / 快取 / 驗證 / 文件 / 瀏覽器等有型別指標, 支援帶 `unit` 與 `value` 的欄位 | 共用結果投影解析 `metrics`, `statistics`, `usage`, `performance` 等結構, 對候選欄位及輸出數量設上限 | 自動投影只涵蓋可辨識單位與型別, 新格式依欄位型別與投影規則擴充 |
| MCP 被動連線觀測 | 最近回應時間, 尚待回應的直接呼叫數, 來源提供的連線 / 認證診斷錯誤 | 根據已載入工具事件與診斷紀錄, 不發送探測請求 | 顯示最近回應時間與診斷狀態, 尚無觀測時省略連線 tag |
| Jev 本機 telemetry | 操作數, HTTP attempts / retries, Token 已知及缺值數, 傳輸大小, 平均延遲, status 分布, 最近 metadata 與通用指標 | 唯讀查詢所選 1 小時 / 24 小時 / 7 天 / 全部視窗. 聚合統計使用視窗內符合條件的資料, 最近 metadata 最多 80 列, 小時序列最多 168 個時間格, status 分組最多 40 個 | 80 列是最近明細查詢上限, 不是聚合統計的來源上限. metadata 超過 65,536 字元或格式不支援會略過, 每筆最多顯示 16 個 attempts. 來源未啟用或 DB 不可用時保留實際健康狀態 |
| Git / Skills / 驗證 / 檔案 / 網路 | literal Git 操作與 repository, Skills 文件讀取, test / build / lint, 檔案操作及位置, 網路工具與參考網址 | Git 回傳最近 500 筆, Skills 保留及回傳最多 500 筆, 驗證回傳最近 500 筆, 檔案回傳最近 1,000 筆. 網路事件使用 MCP 保留列表. 活動序列最多 10,080 個時間格, 工具序列最多 20,000 個工具時間格 | 操作結果使用明確狀態或 exit code, 摘要依完整保留事件計算 |
| Git 工作樹 | 已載入專案與 Codex 管理目錄的工作樹名稱, 分支, HEAD, detached / locked / prunable 狀態, 目錄可用性, 專案與對話關聯, 檢查時間 | 唯讀執行 `git worktree list --porcelain -z`, 最多 100 個來源根目錄及 500 筆工作樹, 每次 Git 輸出最多 1 MiB, 全輪 Git 查詢預算 5 秒, 快取 30 秒. Codex 管理目錄只檢查兩層目錄及 Git 標記 | 對話關聯只使用已載入 Thread 的來源工作目錄, 不推論未載入對話. 完整工作樹位置只在已觀察 ID 的明細回傳, 不讀取工作檔內容, 不抓取遠端或清理工作樹. 最新狀態不套用活動時間範圍 |
| 瀏覽器執行環境 | 目前頁面的瀏覽器名稱與回報版本、語言與時區 | 使用目前瀏覽器提供的 `navigator` 與 Intl 資訊, 只在前端顯示 | 未回報的資訊維持缺值, 不列舉其他已安裝瀏覽器, 不呼叫高熵 client hints, 不送至後端或保存 |
| SQL / SQLite metadata 與按需內容 | SQL 操作類型, statement 類別, engine, 可確認的 DB 位置, 外層工具時間, 個別 SQL 時間, rows_affected / rows_returned, 來源與紀錄識別碼 | session literal 辨識與診斷 SQL 合併後回傳最多 500 筆. 點開已觀察的紀錄才讀取內容. session 內容限制在已追蹤檔案的設定檔尾範圍, 完整取得的 SQL 經遮蔽後分頁, 每頁 32,768 字元, 以 revision 核對後續內容 | 不執行 SQL, 不讀取查詢結果. 舊紀錄仍有 metadata, 但移出檔尾或來源變更後可能無法取得內容. 個別 SQL 耗時及列數僅使用診斷來源提供的值, 不以外層工具時間替代 |
| Desktop / Core 診斷與 Log | severity, module, code, Thread / Call / Request / Trace ID, 檔案及 record ID, 錯誤摘要, 關聯 SQL | Desktop 最多追蹤 8 個檔案, 初次各檔取最近 256 KiB, 差異與歷史分別使用 1 MiB 預算. Core 每次差異及歷史查詢各最多 2,000 列. 錯誤歷史回補範圍 24 小時. 診斷錯誤與 Log 各保留最多 1,000 筆, SQL 診斷最多 500 筆 | 每次查詢上限不是 DB 總量上限. 不支援格式, 過長行, 截斷及尚待回補會影響範圍. Log 等級保留來源英文值 |
| Codex 模型 API 診斷 | 呼叫 / 連線 / 重試分類, Model / Provider, 傳輸方式, API 路徑, HTTP status, Request / Trace / Turn ID, 來源耗時與單次 Token | 共用 Desktop / Core 診斷讀取預算, 只投影已辨識模組的固定欄位, 每筆最多讀取 8,192 字元, 記憶體保留最近 1,000 筆去重事件, 重啟後分批重讀來源最近 24 小時 | 不修改來源紀錄設定, 不保存請求 / 回應本文. WebSocket 送出成功不代表回應完成, 連線與重試不計入呼叫數, 缺少單次 Token 不從累計用量推估 |
| 程式 runtime 與保存資訊 | 版本, 系統, Python, PID, 開啟 / 更新 / 運行時間, refresh / CPU 時間, 讀取量, HTTP 次數及錯誤, 回應及壓縮大小, 保存上限與來源健康狀態 | 效能樣本最多 360 筆, 狀態事件最多 200 筆, 程式 Log 最多 1,000 筆. journal 每份 64 KiB 並保留一份輪替檔. 錯誤 metadata 記憶體工作集合最多 1,000 筆及 512 KiB, SQLite 歷史預設保存 90 天, 0 表示不自動刪除, 不依筆數刪除 | 效能樣本與狀態事件存在記憶體, 重啟後重新累積. journal 與錯誤 checkpoint 可跨重啟保存, 不包含完整對話或工具 payload |

子代理程式與專案頁只呈現已載入 metadata, 父子關聯與專案對話數依已載入範圍計算. 最新狀態, 累計 Token 與額度不套用操作事件時間篩選, 時間未知的事件只在全部範圍顯示

專案資料庫最多讀取 2,000 個專案與 5,000 個 root 關聯, 每個專案最多 32 個設定資料夾. 同一 Project ID 的名稱與資料夾優先使用資料庫, 舊 app state 只補缺值. 封存狀態使用來源的明確 boolean, 不從對話狀態猜測. 專案 icon 與本地 / 雲端類型需來源實際提供, 缺少時保留文字與未知類型

工具統計在表格截斷前, 使用所選範圍內的完整保留呼叫計算次數, 涉及對話, 已回傳數, 耗時樣本, 平均與 P99, 並列出最近呼叫與回應時間. 程式碼辨識的內層工具沒有獨立時間時保留缺值. Skills 計數與 Dot 產出總數也先套用範圍再截斷明細, 不以表格筆數取代總數

監測效能樣本與執行事件依所選時間範圍顯示, 圖表平均與 P99 使用範圍內保留樣本. 最新更新耗時, runtime, 已執行時間與累計 HTTP / 讀取計數保留最近程式狀態. 一份快照的範圍投影不增加 session 或資料庫讀取

實際來源位置, 讀取健康狀態, 已選欄位, 當次行數與上限由 snapshot 的來源資料提供. 頁尾預設收合, 展開只整理已載入 metadata, 不再次掃描來源

## 按需明細

### Context 與代理訊息

一般 snapshot 與 checkpoint 只保存代理通訊的 action、sender、target、task_name、方向、Thread / Call ID、呼叫索引及來源時間, 不保存 message、prompt、Context 或回覆本文. 主 / 子代理關係仍依來源提供的 parent_thread_id 與代理資訊, 不從訊息文字猜測關聯或確認送達

點開代理訊息時, `/api/codex/agent-message` 只讀取已觀察 Thread / Call / index 對應的傳送參數與工具回覆. 共用的外層回覆不當成個別代理已成功回覆. 接收訊息若只有已記錄標頭而沒有 Call ID, 介面改顯示該對話近期可取得的 Context, 明確標示不是單筆傳送內容

`/api/codex/context` 按需讀取最多 1 MiB 的來源片段, 回傳最近最多 32 則可見訊息. 指定 Call ID 時只選該呼叫之前的片段, 未指定時使用對話近期來源. 只呈現 user / assistant 訊息與明確的公開思考摘要, 排除 analysis、完整推理與加密推理內容. 缺少記錄時保留 `not_recorded`, 索引仍在回補時保留 `pending`, 不由 `fork_turns` 推論完整模型 Context 或所有繼承內容

內容明細預設遮蔽可辨識的敏感欄位, 使用者可在「內容與隱私」關閉. 設定只改變按需明細的呈現, 不修改來源或歷史快照. 關閉遮蔽仍保留來源選取、讀取範圍與公開摘要限制

- SQL 內容由已觀察紀錄 ID 綁定來源, 工具回傳依同一 Call ID 按需讀取並遮蔽 credentials, 不寫入 snapshot 或歷史資料庫. 共用回傳標示為外層工具回覆, 不推論個別 SQL 結果, 不重新執行查詢. 無法取得時保持缺值, 不允許任意檔案或任意查詢
- Jev 呼叫送出與回傳內容只在點開已觀察呼叫時讀取, 遮蔽可辨識 credentials. 混合 exec 回傳不當成 Jev 單一結果
- 專案資料夾由已載入來源設定按需取得. Global / Project AGENTS.md 只從選定根目錄讀取, 最多 32 個根目錄, 每份最多 64 KiB, 不走訪任意檔案. 拒絕非絕對路徑, 網路路徑與 symlink, 遮蔽可辨識 credentials. 不加入 snapshot / checkpoint
- Git 明細綁定已觀察的 Thread / Call / 操作, 按既有檔尾與讀取預算取得指令及工具回傳. 不執行內容, 多個操作共用回傳時保留外層回傳範圍, 不推論個別命令成功
- 工作樹明細綁定最近檢查已取得的 ID, 回傳完整位置與已載入關聯對話. 拒絕任意位置與未觀察 ID. 停用 Codex 或工作樹監測後不再讀取 Git 或管理目錄, 管理設定維持唯讀
- 工作樹查詢超出整輪時間預算時保留已取得結果, 下輪從未完成的根目錄繼續. 各列保留自己的檢查時間, 不將尚未更新的列標成最新資料
- Skills 文件只在點開已辨識文件時讀取, 檔案清單最多 200 個, 走訪最多約 1,000 個項目. 單份文件最多讀取 256 KiB, 取得文字以分批建立 DOM 的方式完整顯示. 重啟保存的 Skill 事件不保存檔案路徑與內容

## 程式依據

- [Session, Jev 與按需內容](../src/local_activity_monitor/collectors.py)
- [Catalog, index 與 app state](../src/local_activity_monitor/codex_metadata.py)
- [Lifecycle / Skills checkpoint](../src/local_activity_monitor/thread_state.py)
- [用量與額度投影](../src/local_activity_monitor/usage_records.py)
- [MCP 動態來源, 結果投影與摘要](../src/local_activity_monitor/mcp_records.py)
- [SQL metadata 與內容遮蔽](../src/local_activity_monitor/sqlite_records.py)
- [診斷讀取與錯誤摘要](../src/local_activity_monitor/error_records.py)
- [Runtime 與 journal](../src/local_activity_monitor/monitor_state.py)
- [Git 工作樹唯讀監測](../src/local_activity_monitor/worktree_info.py)
- [錯誤 checkpoint](../src/local_activity_monitor/error_history.py)
- [API, 合併資料與動態來源範圍](../src/local_activity_monitor/server.py)
## 裝置與活動新增欄位

字型選單使用目前作業系統提供的字型家族名稱, Windows 透過 GDI, macOS 透過 CoreText, Linux 使用已存在的 `fc-list`. 回傳最多 500 個去重名稱, 不回傳字型檔案位置. 字型清單與裝置資訊一起讀取, 不在每輪活動更新重新列舉

- 裝置容量: 處理器名稱, 可用邏輯核心, 實體與可用記憶體, GPU 型號, 專用記憶體, 系統共享記憶體上限, 驅動版本. 使用 Windows 系統 API, Linux `/proc` / DRM 與可用的廠商工具, macOS 系統 metadata, 依平台與硬體能取得的欄位顯示
- 裝置使用量: CPU 以兩次系統計數器差值計算, Windows 使用 `GetSystemTimes`, Linux 使用 `/proc/stat`, macOS 使用 Mach CPU ticks. Windows 超過 64 個邏輯處理器時標示目前處理器群組範圍. 已使用記憶體由實體容量減去可用量, 每次完整更新最多每 5 秒重新取樣
- 檔案: 來源 literal 參數的讀寫範圍, 單一獨立檔案工具回傳的 `bytes_read` / `bytes_written`, 送出文字的 UTF-8 大小. 進入檔案頁時自動取得目前檔案大小與修改時間, 顯示期間每分鐘更新, 也可手動重新檢查. 每次最多檢查 100 個已觀察檔案, 文件內容不加入活動 snapshot
- 網路: 網址及網站計次, 對話數, 不同頁面數, 首次與最近參考時間, 操作類型與來源. 計次基於已載入的工具事件, 不是 HTTP 流量計數
- MCP 活動: 選定摘要與數值指標保存至 SQLite, 預設保留 90 天, 可設定 0 - 3650 天, 0 表示不自動刪除, 記憶體工作集合仍保留容量限制, 配合來源開關及頁面時間範圍顯示. 可編輯文件內容不加入 snapshot 或活動快取

Codex Core / App 診斷的錯誤內容由已觀察的紀錄 ID 或已確認檔案位置與行位移按需取得, 核對來源身分及內容雜湊, 遮蔽後才分頁. 資料快照只保存識別 metadata, 不保存原始錯誤本文

## 收集排程與回補監測

快照新增目前收集階段、各階段耗時 / 程序 CPU 時間與當次最長階段 CPU 時間, 不保存來源本文. session 讀取狀態新增首次讀取待處理檔案數、工作狀態與錯誤回補的檔案數及剩餘位元組數. 回補只讀取既有白名單紀錄, 四分之一讀取額度保留給歷史資料, 有錯誤待回補時再保留八分之一整輪額度. 狀態補讀以檔案輪替且每檔每輪最多 1 MiB, 錯誤仍限最近 24 小時

目前頁面的瀏覽器資訊保留名稱 / 版本、語言及時區, 不顯示或保存瀏覽器平台、視窗大小與裝置像素比

頁面效能資訊僅在瀏覽器記憶體保存 TTFB、FCP、LCP、CLS、首次資料呈現時間、長任務數 / 最長耗時及最慢互動耗時. 不保存 DOM 或事件本文, 不送至伺服器, 不寫入偏好與 checkpoint. 不支援或尚未取得樣本時顯示缺值, 最慢互動為 Event Timing 最大 duration

## 包裝輸出中的 MCP 結果 metadata

有界限的結果解包, 用於已觀察 MCP 的 `input_text` / `text` 區塊. 區塊 text 可為物件或 JSON 字串, 最多 100 個區塊 / 1 MiB 字串, 只有一個可辨識 status 結果時才取用. 多個結果不合併, 保留外層 isError

此包裝形式沿用既有 metadata 白名單, Jev 摘要保留有效型別的 status、latency_ms、usage input_tokens / output_tokens 及其單位, 不將 answers、choice、probabilities、model 或原始 request / response 本文加入 snapshot 或活動快取. 完整內容仍依選定觀察 ID、版本、遮蔽及分頁規則按需取得

這些值是來源回覆 metadata, 外層工具包含多個呼叫時不推論每個內層呼叫已成功. 未提供耗時或 Token 時維持缺值, 明確的 0 保留
