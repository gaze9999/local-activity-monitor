# 測試與建置

依[維護與驗收](maintenance.md)選擇受影響的檢查, 使用獨立的合成資料與測試設定

| 檢查範圍 | 執行入口 |
| --- | --- |
| Python 行為 | 在儲存庫根目錄執行 `python -m unittest discover -s tests` |
| 前端互動 | 依修改範圍選擇 `tests/*-flow.cjs` 的對應流程, 核對載入、更新、失敗恢復與實際介面 |
| 離線資產 | 執行 `python tools/build_frontend.py --verify-offline`, 依 [workbench-ui.json](../workbench-ui.json) 核對固定的 WBUI revision |
| 套件與平台 | 依[發行工作流程](../.github/workflows/release.yml)核對建置、隔離啟動及產物 checksum |

公開驗證結果記錄受測 revision、平台、相依版本、合成資料條件、執行方式、觀察結果與涵蓋範圍

## 2026-10-24 固定版本資產校正

重建時比對封裝資產與固定 WBUI revision 的 Git blob, 避免工作目錄的 CRLF / LF 轉換改變封裝內容, 核對方式見[維護與驗收](maintenance.md#共用-ui-與按需內容)

## 2026-11-16 Desktop Model API 翻譯

以 LAM main `5c03daf9` 的 Python `observe_api` 為來源, Rust 分支基準為 `8f8baf1`, Linux 雲端環境使用 Rust 1.100.0

- API 投影的 `thread.id`、`conversation.id` 與加引號的 `thread_id` 優先於診斷來源的 thread metadata, 無效 API ID 才保留來源 ID
- API 記錄滿 1,000 筆時, 同時間的新記錄取代最早插入的記錄, 較舊記錄及重複 observation 不擠掉既有記錄
- 兩個合成資料測試先重現差異, 修正後通過, 同時核對 observation ID、來源位移及不保存本文
- 此輪僅執行針對性測試, 未完成整體翻譯、實際平台來源或介面驗收

### 跨來源 Model API 保存順序

- Core 與 Desktop 共用 1,000 筆已觀察記錄, 每輪依 Python 的 Desktop → Core 順序加入, 同時間排序保留先後順序, 不以 observation ID 決定顯示或淘汰
- 關閉 Model API 觀察或重新掃描診斷來源時清除對應暫存狀態, 再由已授權的來源重新讀取, 不改動持久化歷史格式
- 兩個 queue 回歸測試及五個 Model API HTTP 測試通過, 包含跨輪重複、同時間淘汰、來源停用 / 重新啟用及 Core 1,000 筆後新增 Desktop 事件
- 未重跑完整套件, 此結果不代表所有翻譯或平台行為已完成驗收

來源暫存保留每個讀取批次的完整到達順序, 不在共用 queue 前提前去重或截短, 避免 `[a0, new, a0]` 的第二次 `a0` 在前次淘汰後被遺漏

Desktop commit → 共用 queue 的重複淘汰回歸通過, 寫入失敗測試確認失敗時不發布 API 事件、重試後分兩批保存 4 筆與 1 筆且不重複, 暫存沿用讀取批次預算並每輪排空, 共用保存上限仍為 1,000 筆

### 同一 thread 的工作起點合併

依同一 Python revision 的 `collectors.py`, 各來源先比較狀態時間, 再獨立接受較新的非空工作起點, 不能只用狀態勝出來源的起點

兩個來源互換順序、狀態時間相同及 completed 狀態的合成案例已與 Python collector 核對, 新回歸先重現 360,000 ms 與應為 180,000 ms 的差異, 修正後通過, 既有 metadata 合併測試也通過, 未改動持久化格式或重跑完整套件

### 未知 Context 時間

缺少或無法解析 timestamp 的 Context 不作為彙總 model / reasoning effort 的依據, 仍保留來源內的原始投影與可用 execution metadata, 有效時間的 Context 依既有時間規則選用

以相同 Python revision 的 collector 確認此行為, 缺少 / 無效時間、有效時間與不同合併順序的針對性回歸先失敗後通過, 既有選取來源合併測試通過, 未修改 checkpoint 欄位或宣稱平台驗收

### Execution metadata 別名

canonical 欄位存在但無效時不退回別名, 與 Python `execution_metadata` 一致, 涵蓋 approval policy 與 model provider, 只保留欄位存在的判斷而不保存無效原始內容

缺少 canonical、null / bool / object / 過長值、有效值優先、巢狀與序列化 spawn 的針對性回歸先失敗後通過, 既有模式與私密欄位排除測試也通過

### 結構化 metadata 字元上限

序列化 sandbox policy、collaboration mode 與 spawn source 的上限依 Python 計算 4,096 個字元, 不把 UTF-8 位元組數當作字元數, 計數最多檢查 4,097 個字元且仍只輸出既有白名單欄位

含中文但僅選用安全 metadata 的 4,096 / 4,097 字元邊界回歸先失敗後通過, 三個 metadata 測試通過, 忽略欄位不出現在結果

### 大型快照的 winner 描述規格

新增僅供測試的不可變 winner plan, 比對既有 materialized `merge_snapshot` 的結果, 單一針對性測試通過, 不接入執行期發布

- 先保留既有選取結果, 再依 active → overflow 來源順序決定同 thread / call ID 的 winner, 不改成以事件時間決勝
- 描述依 thread / call 的既有排序輸出, 沒有選中 call 的已識別 thread 來源仍參與 thread metadata 合併, 無 thread ID 的來源仍須另外保留 allowance 與來源錯誤
- 選取未完成或測試建構過程取消時不產生 plan, 重試不沿用部分結果

此規格不代表 16 MiB 限制已解決, 尚未實作跨輪來源更新驗證或串流 reducer, 也不是實際服務取消流程的驗收

後續移植須保留 canonical thread / call / file-index 的同時間次序、精確 p99 的 duration 樣本、distinct-thread 與 history identity 狀態, 不得以近似值或提前截斷取代

Allowance、來源錯誤、connection、skill history、MCP / SQL history merge 與 error detail 的原有優先順序仍須獨立保留, 發布前需一併驗證 active payload、來源 signature / epoch、reference / cutoffs、設定與歷史版本, 全部 windows 和 detail metadata 必須一致發布

現行四個 window 的最終序列化資料仍留在記憶體, 解決來源 payload staging 不等於保證整體記憶體不超過 16 MiB

### FileSummary 相同時間的排序

FileSummary 預覽以既有 timestamp 字串、thread ID、call ID 與原始檔案事件 index 排序, 取代依輸入到達次序產生的 ordinal, 保留目前 Rust BTreeMap canonical 輸入的相同結果, 允許已決定 winner 的呼叫以不同分批順序處理

原始 index 在 read / change 篩選前取得, 外觀相同的事件不去重, 每個已決定的 thread / call winner 仍只能觀察一次, 全部計數與預覽上限不變

反向分批測試先重現預覽次序差異, 修正後連同未修改的 full-sort oracle 共兩個測試通過, 涵蓋未知時間、毫秒內時間差、時間界線、read / change 篩選、重複外觀及不同預覽上限, 執行期發布與來源掃描未切換, 16 MiB 問題仍未解決

### ActivitySeries 資料生命週期

ActivitySeries 改為自行保存 tool bucket 的工具名稱, 可在分批觀察後釋放暫時 Projection, 再序列化結果, 10,080 個 minute bucket 與 20,000 個 tool bucket 上限、精確計數與排序維持不變

六個針對性測試通過, 包含釋放暫時來源後序列化、完整來源計數再取尾端的 oracle、上限、淘汰後重複 key、window 與不完整範圍欄位

此變更增加受 bucket 數量限制的名稱儲存及每次觀察的暫時字串配置, 不代表固定的位元組預算、效能改善或 16 MiB 問題已解決, 執行期發布仍未切換為跨輪串流

### Owned tool statistics prototype

僅在測試模組建立 owned statistics prototype, 執行期仍使用原有 serializer, 不接入發布

Prototype 自行保存 tool group、distinct-thread membership 與精確 duration 樣本, 沿用既有平均值與 p99 插值, 不近似或截斷樣本, 各呼叫的暫時副本可在觀察後釋放

測試注入 entry 數與 key 字串位元組上限, entry 包含 group、thread membership 與 duration 樣本, 超過任一上限會清空並拒絕整個結果, 不輸出部分統計, 這些不是新增的產品限制

四個針對性 statistics 測試通過, prototype 與未修改的 serializer 比較反向輸入、缺值、零 / 負耗時、巢狀 group、時間界線及停用結果, 並驗證超出預算拒絕

儲存量隨實際 group、distinct-thread membership、已知 duration 樣本與 key 長度增加, entry / key 預算不包含 allocator overhead 或最終 JSON, 未宣稱整體固定記憶體、效能改善或 16 MiB 限制已解決, 高 cardinality 的發布方式仍未決定

### Catalog 欄位與未知呼叫數

未取得 catalog 或停用 metadata 時, thread_name、host_id、archived、project_id、project_name、project_kind 明確輸出 null, project_scope 為 unknown, has_schedule 為 false, 保留已觀察的 session 欄位

只有 catalog 的列以 tool_calls: null 表示未觀察到呼叫歷史, 有 session 且沒有呼叫的列仍為 0, tools 內容不變

兩項差異均以獨立 Python 暫存合成 collector 確認, Rust 回歸測試先失敗再修正, 三個針對性單元測試與既有 catalog HTTP 測試通過, 未宣稱 UI 或真實來源驗收

### 跨輪發布的一致性界線

唯讀檢視確認現行 Collector 在同一 worker 建立完整 Snapshots, 再以 send_replace 發布, 這不是已證實的並行 membership 競態

SourceMembership 的 identity、revision、selection_epoch, 完整的 CallSelection 及來源 checkpoint signature 可界定選取階段, 既有測試已涵蓋選取變更、取消、分頁後 signature 改變與驗證階段

Discovery 的 generation 本身不宜作為跨輪 reducer 的一律失效條件, 既有 recent_call_selection_resumes_past_100000_tied_headers 明確保留未變更來源的新一輪掃描進度, 否則可能無法完成大選取範圍

上述訊號不足以代表完整快照版本, 未來跨輪處理仍需涵蓋同一 call ID / 時間下的回覆、錯誤與 context 更新, 以及 reference / cutoff、設定、catalog 與 history 的一致性, checkpoint_dirty 是布林值而非單調版本, 發布 version 也不是來源版本

本輪未新增重複的假想 stamp 測試, 未修改執行期發布或宣稱 16 MiB 問題解決, 改用跨輪 reducer 前仍須決定各來源的版本與共同發布規格


### 同時間事件次序的待修正差異

獨立 Python 合成 collector 證實, 同時間 outgoing calls 依 z、a 插入時, agent_messages 保留 z、a, Rust 目前依 call ID 排為 a、z, 超過預覽上限時可能影響顯示成員

先前 FileSummary 與 full-sort oracle 的驗證僅證明保留 Rust BTreeMap 的既有次序, 不是 Python insertion-order 相容性證明, 不應作為跨輪 reducer 的來源排序規格

修正前需保留跨來源 thread / call 的首次插入位置, 同 ID 後續內容取代不移動位置, 來源內 Call.sequence 會跨來源重複, 不能單獨代表全域順序, 此項仍待完成來源與持久化規格核對

### Worktree Unicode branch 欄位

Branch ref 的 512 上限改依 Unicode 字元計算, 對齊 Python parse_worktrees, 保留 control 字元檢查與 1 MiB 原始輸出上限

實際 Python parser 接受 512 字元並拒絕 513 字元, 分段的 512 字元 / 1,490 bytes 合成 ref 通過唯讀 git check-ref-format, Rust 邊界回歸先失敗再通過, 另通過既有 NUL records / lock reason 遮蔽測試, 未建立分支或變更 Git 狀態


### Winner descriptor 次序規格修正

測試限定的 winner descriptor 現在接受每個來源明確提供的 call 次序, 不再以 BTreeMap 或持久化 sequence 直接代表 Python 次序, 來源數、未知 ID、重複 ID 或遺漏已選 call 的次序資料會拒絕

規格保留 thread 首次出現位置與 call 首次插入位置, 後續相同 call ID 僅取代 winner source, 不移動位置, Rust merge oracle 僅比較 winner 內容集合, 次序另以實際 Python fixture 的 z、a / z replacement、a、b / thread 1 再 thread 0 結果驗證

獨立 Python checkpoint save / load 與重啟 collector 證實, fresh z、a 在 checkpoint restore 後變成 a、z, 因載入以 event_time DESC、call_id 排序, 因此後續移植目標保留 Python 的 fresh / restart 差異, 不自行改成跨重啟固定順序

此規格仍未接入服務, 正式來源遍歷、active / overflow 串接、restore 次序來源與各事件預覽尚待移植, 不代表執行期排序差異已修正

Rust 實際 Collector refresh → checkpoint Store fixture 已確認 SQL 載入順序為 a、z, 保存的原始 sequence 仍為 z 先於 a, 此單一針對性測試通過, 加上兩個 winner descriptor 測試通過, 證明順序來源不能互換, 不代表重啟後的事件預覽已修正

獨立 Python 雙來源 fixture 進一步確認下列同時間事件次序, metadata 停用, 使用暫存檔與控制的 mtime, rescan 前刻意延後下一次 scan

| 階段 | 來源次序 | 全域 file preview |
|---|---|---|
| Fresh A[z,a]、B[q] | A、B | z(A)、a、q |
| 實際 checkpoint 儲存並重啟 | A、B | a、z(A)、q |
| 取代 A.z、增加 A.b 與 B.z, 尚未 rescan | A、B | a、z(B)、b、q |
| B 較新後明確 rescan | B、A | q、z(A)、a、b |

兩次 checkpoint 儲存均成功, 另以兩個 thread 相同時間證實 mtime 改變但尚未 scan 時仍為 1、0, rescan 後才成為 0、1, 因此不能每次 snapshot 依目前 Session.modified 重新排序

測試限定 winner descriptor 已加入上述更新與 rescan 的實測期望, 仍未串接 runtime 來源排序或改變 eviction, 正式移植需要獨立的 discovery-time rank 與 fresh / restored call 次序來源

### 來源排序與容量規格的相容性阻擋點

目前保留既有 Rust bounded overflow、50,000 全域 call 選取、8,192 單來源上限及 eviction 防護, 不因預覽次序差異改變來源成員

Python 的相關行為分為三層, 不能用預覽排序替代

- collectors.py 的 restore load 使用 50,000 減目前 retained calls 的剩餘容量, 來源處理順序受 read_cursor 輪替影響
- session_checkpoint.py 載入以 event_time DESC、call_id 排序, 單次上限為 8,192 與剩餘容量的較小值
- 單來源超過 8,192 時淘汰目前 dict 第一項, restore 後不等於原始 sequence 第一項, 全域超過 50,000 則另以 timestamp-or-empty、完整 path、call ID 升冪淘汰

獨立 preview 次序資料可避免直接改動 eviction queue, 但仍不能宣稱 restore 成員完全對齊 Python, 既有專案文件未提供特別授權取代 Rust 容量策略的依據

跨輪 provenance 設計仍需限制 source-only metadata、空 thread 及 call rank 儲存, 50,000 個已選 descriptor 不能代表全部暫存資料的記憶體界線, 未加入無上限 map 或新的 runtime 排序狀態

另完成 worktree 正常 porcelain 欄位與 account 限額 / credits / usage 數值投影的有界唯讀比較, 未建立新的具體缺陷, 未執行真實 account、Codex CLI 或使用者來源查詢

### Model API 控制空白分隔

API 診斷欄位解析對齊 Python 的 U+001C–U+001F 空白分隔, 避免整筆事件或 token 數值被遺漏, 原有欄位白名單、數值界線與引號遮蔽維持不變

獨立 Python 合成 fixture 確認四種分隔字元的預期結果, Rust 回歸先失敗再通過, Model API 模組 6 項測試全部通過, 未重跑完整套件或變更來源排序、淘汰及大型快照策略

### Core / Desktop 診斷欄位與標題分界

診斷解析同樣對齊 Python 的 U+001C–U+001F 空白分隔, 保留 request ID、thread ID、錯誤代碼與 HTTP 狀態, 並避免把 message 欄位中的 timeout 或 stream disconnected 誤判為標題原因

實際 Python diagnostic_details 與 Desktop collector 合成案例確認四種分隔字元的結果, Rust 回歸先失敗再通過, Core / Desktop 診斷模組 13 項測試通過, 欄位白名單、憑證排除及本文不持久化界線維持不變

### Web 引用的 Unicode 解析範圍

Web 回傳引用解析改依 Python 的 1,048,576 字元範圍, 避免多位元組文字造成範圍內 URL 被提早排除, 30 筆去重引用上限與 URL query 遮蔽維持不變, 不保存回傳本文

Python response_metadata 合成案例確認 400,000 個中文字後的 URL 仍納入, 字元範圍外 URL 排除, Rust 回歸先失敗再通過, history-store 6 項單元測試及 2 項工具錯誤 / Web 引用重啟回歸通過

解析範圍最多涵蓋 4 MiB UTF-8 文字, 這是來源字元規格對齊, 未宣稱效能改善或 16 MiB 快照問題已解決

### 階段提交前回歸

Linux / Rust 1.100.0 執行 `cargo test --locked --offline --manifest-path rust/service/Cargo.toml`, 單元測試 291 項通過、2 項失敗, account 時限案例單獨重跑通過, 另一項 overflow metadata-only 案例的舊期望值由 0 對齊 Python 的 null 後單獨通過

另以 `--test http` 執行 HTTP 測試, 201 項通過、2 項失敗, error-detail 的 503 與 retention retry 的舊快照值均在個別重跑時通過, 尚未取得整套並行測試全數通過的結果, 不將個別通過當成並行穩定性證明

### MCP 結果投影界線與空白相容性

Web 結果先為 references 保留一個投影欄位, 再套用 Python 的 40 欄 metric / credits 界線, 避免多保留一筆指標或 credits, 非 Web 結果與後續 written 等欄位規則維持不變

Python response_metadata 的 38–41 筆合成指標確認邊界, Rust 回歸先失敗再通過, HTTP 合成回歸確認即時快照、session checkpoint 重啟與 Web 歷史投影

Web 歷史依 Python activity_history 規格只保存引用與允許的狀態, 不保存即時結果中的任意指標, 初版測試錯把兩者當成相同投影, 依來源修正期望後通過, 未變更歷史保存欄位或新增來源查詢

本輪比對 LAM main 5c03daf 與 workspace main 9e606e0, Rust 基底 3cab348, 未重跑整套 service 或平台驗收, 來源排序、容量與大型快照的既有阻擋點維持待辦

同批 Web 引用解析補齊 Python 的 U+001C–U+001F 分隔, 避免兩個 URL 被當成單一含控制字元的無效引用而全部遺漏, 仍拒絕 URL 本身的控制字元、不公開的位址與憑證, 維持 query / fragment 排除及 30 筆去重上限

實際 Python fixture 確認四種控制空白、tab、NBSP、em space 與重複引用上限, Rust 針對性回歸先失敗後通過, Web 引用歷史重啟案例通過


工具結束碼記錄同樣接受 Python 的四種尾端控制空白, 避免漏報非零結束碼, 仍排除 0 與未結束的記錄文字, 不保存工具本文

Python tool_error 合成案例與 Rust 先失敗後通過回歸確認正數、負 Unicode 數字及零值, 最終 history-store 完整套件 53 項通過, 工具錯誤 HTTP 快照、歷史與重啟回歸通過, 既有數值、字元讀取上限與欄位白名單不變

### Metadata 文字缺值與回退

排程與背景 metadata 的文字修剪對齊 Python, U+001C–U+001F 也視為頭尾空白, 避免只有控制空白的 catalog 標題阻止 state / session index 的有效名稱回退, 同時維持 project_scope 的空值語意

隔離 SQLite、session index 與 Desktop 快取 fixture 直接比對 Python read_metadata、read_schedules、desktop_activity, 核對標題、model、project ID、排程及遠端名稱, Rust 回歸先失敗後通過, schedules 模組 11 項測試及 HTTP 索引更新回歸通過

字元截斷、來源優先順序、選取對象、查詢次數與讀取界線不變, 未查詢實際帳號或使用者來源, 本段基底為 7761ea1, 原有大型快照與排序待辦不受影響

同批將相同文字規則套用到專案 metadata, 修正頭尾控制空白造成的 project ID 對應失敗, 保留資料庫名稱優先與空 legacy 名稱回退, 專案根目錄仍只在已觀測專案的按需 detail 回傳

Python 專案來源 fixture、Rust 先失敗後通過回歸、projects 模組 7 項測試與 HTTP 專案快照 / detail 回歸通過, 覆蓋 canonical mapping、名稱與路徑修剪、來源優先、metadata 停用及未知專案 / 任意路徑拒絕, 未增加查詢或改變來源範圍

此 metadata 段落另執行完整 service 單執行緒回歸, `cargo test --locked --offline --manifest-path rust/service/Cargo.toml --bin local-activity-monitor -- --test-threads=1` 的 295 項與 `--test http -- --test-threads=1` 的 203 項全部通過, 這是 Linux 單執行緒測試證據, 不取代前次並行波動的穩定性驗證或原生平台驗收

### Plugin 設定分行與 metadata 缺值

Plugin 設定讀取對齊 Python splitlines 與空白辨識, 修正 CR-only 等分行格式漏掉 enabled 的情況, 保持進入其他 section 時重設歸屬, 這是既有唯讀 metadata parser 的相容性處理, 不改寫設定或啟用 Plugin

Manifest 的空白版本改回退到快取資料夾版本, 顯示名稱與描述沿用 Python 文字修剪, 字元上限、目錄名稱白名單、符號連結拒絕、讀取預算及版本排序規則不變

Python read_plugins 合成檔確認 11 種分行與 U+001F 空白、版本回退及文字欄位, Rust 兩項回歸先失敗後通過, plugins 模組 8 項與 HTTP 快照、來源更新及 metadata 開關回歸通過, 未執行實際 Plugin 或讀取帳號來源, 本段基底為 2db42b0

### MCP 來源簡介空白與回退

MCP 設定、套件 metadata 與 README 簡介對齊 Python 的 U+001C–U+001F 空白語意, 空白設定與套件描述不再阻止有效來源回退, README 標題與空白段落仍排除, 套件與 README 描述中的連續空白收斂為單一空格

Python source_description 合成 fixture 確認設定修剪、套件正規化及 README 回退, Rust 簡介回歸先失敗後通過, source_introduction 2 項、設定描述 1 項及 HTTP 套件 / README 簡介回歸通過, 遮蔽、400 字元上限、來源根目錄限制與快取策略不變

本輪核對 LAM main 5c03daf 與 workspace main 9e606e0 未變, 保留 Plugin 同批未提交變更, 未查詢真實來源或重跑完整平台驗收

### TOML 宣告順序與 MCP 選取界線

啟用既有 toml 0.8.23 的 preserve_order, 對齊 Python 前 64 筆設定宣告與 env 候選順序, 修正字母排序讓後段來源擠掉前段來源簡介或按需文件的情況, disabled 項目仍占用原始設定的位置, 第 65 筆有效來源仍列為 configured, 但不讀取其簡介或文件設定

相依套件版本與 checksum 不變, lockfile 只新增 toml 對既有 indexmap 的依賴邊, 未下載或安裝新套件, TOML 來源設定不改寫, 讀取數量與安全界線不變, 結構化 TOML 仍透過既有 JSON 投影, 簡介快取鍵隨原始順序產生但保留原有容量與期限

Python discover_sources / documents 合成 fixture 確認宣告順序, Rust 前 64 筆回歸先失敗後通過, MCP / documents 模組 12 項、env 候選 1 項、structured 8 項及 HTTP MCP 21 項通過, 包含簡介、文件、遮蔽、設定更新與歷史回歸, 未執行真實來源或完整原生平台驗收

本段基底為 645d1c1, LAM main 5c03daf 與 workspace main 9e606e0 未變, 來源事件排序、restore 容量與大型快照待辦仍保留

preserve_order 使用面檢閱涵蓋 service 全部 4 個 TOML 消費模組, 來源與文件選取之外的影響如下

| 使用面 | 實際影響與保留條件 |
| --- | --- |
| mcp | 前 64 筆依宣告順序選取簡介, 最終來源 map 仍依既有鍵排序, 不增加設定檔讀取 |
| mcp_documents | 已觀測來源的設定及 env 候選依宣告順序限制, `/api/mcp/files` 路由、來源名稱驗證、SHA 衝突檢查與原子寫入不變 |
| source_introduction | 根目錄及描述欄位皆按名稱查找, 不受 map 迭代順序影響, 宣告重排可能產生另一快取鍵, 仍受既有 256 筆 / 60 秒限制 |
| structured | 完整 TOML 解析後轉既有 JSON map, JSON 欄位排序、日期檢查、深度限制及遮蔽不變 |

核對目前鎖定 toml / indexmap 的 PartialEq 實作, 仍依 key / value 比較而非插入順序, 文件提交前的設定值衝突檢查未因功能旗標失效, 這是原始碼檢閱證據, 不宣稱新增平台或競態驗收

上述使用面檢閱後, 以 Linux / Rust 1.100.0 執行完整 `--locked --offline` 單執行緒回歸, service binary 301 項全數通過 (104.58 秒), HTTP 204 項全數通過 (158.29 秒), 未將單執行緒結果延伸為並行穩定性或跨平台驗收

### MCP 設定換行相容性

MCP config 解析對齊 Python Path.read_text 的 universal newline 行為, 在既有 1 MiB 有界讀取後, 僅於記憶體將 CRLF / CR 轉為 LF, 修正 CR-only 設定導致來源與按需文件全部遺漏, 多行描述同樣保留 Python 換行值, 不改寫來源設定或放寬 TOML 語法

來源清單與 `/api/mcp/files` 共用同一解析入口, 一般文件本文的 TOML parser 不變, Python 合成 fixture 確認 LF / CRLF / CR 的內容及原始 bytes 不變, Rust 回歸先失敗後通過, MCP / documents 14 項與 HTTP MCP 21 項通過, CR-only 文件列舉 / 遮蔽 / 拒絕連結回歸另行通過

本段基底為 e009f30, LAM main 仍為 5c03daf, workspace main 4616074 相對前次只有報告工具與文件變更, 不影響本段 Python 來源或 WBUI pin, 既有容量、來源事件順序與大型快照待辦不變

同批 MCP 文件發現流程對齊 Python 的 JavaScript 設定檔名稱比對, 不再把 CONFIG.JS / config.JS 當成 config.js 並讀取其參照文件, 一般資料副檔名仍保持大小寫不敏感, 未擴大讀取來源

Python documents 合成 fixture 確認兩種規則, Rust documents 模組 6 項通過, HTTP 文件列舉回歸確認排除大寫 JavaScript 設定與其參照, 同時保留 CR-only 設定、合法文件、遮蔽及連結拒絕, 初版測試誤把資料副檔名也視為大小寫敏感, 已依原始 Python 修正期望, 未修改原有資料副檔名規則

### MCP Python 模組名稱辨識

本機模組來源名稱對齊 Python regex 的字元集合, 保留 ASCII 開頭及各段分界, 接受後續 Unicode 字母、數字與底線, 排除 Rust `\w` 額外納入的組合符號與連接控制字元, 同時修正漏掉 superscript 數字名稱的來源

Python source_roots 合成 fixture 核對字母、數字、底線、組合符號及 join control, Rust 回歸先失敗後通過, source_introduction 3 項與含 Unicode 模組路徑的 HTTP docstring 回歸通過, runtime fixture 不可執行, 未執行模組或其程式碼, 路徑安全檢查、16 筆根目錄上限及 metadata 讀取界線不變

本段基底 07cf188, LAM main 5c03daf 與 workspace main 4616074 已核對, 未變更既有來源事件排序、restore 容量與大型快照待辦

同批模組簡介加入 Python ast.get_docstring / inspect.cleandoc 的縮排清理, 單行全空白 docstring 不再擋住 README 回退, 保留 Python 對多行殘留空白的來源選取差異, tab 以 8 欄展開, Unicode 縮排依字元計算

已核對 helper 僅由來源簡介呼叫, 保留原有 65,536-byte 來源上限、語法解析與禁止執行界線, Python ast / source_description 合成 fixture 通過, Rust docstring 回歸先失敗後通過, session-core sqlite-python 完整 33 項、source_introduction 4 項及 HTTP catalog 5 項通過

### 完整 working set 的來源合併順序

當一次完整 discovery 的所有來源都在 working set 中, 保留 discovery-time mtime / path 倒序供 snapshot 合併與 detail index 使用, 修正重複 thread / call 被檔名字母排序選錯來源的情況, 重新掃描後若順序改變會觸發發布, 掃描前單純變更檔案 mtime 不會重排已載入快照

Python 的規則分別保留: snapshot 同 call ID 由後面的來源覆蓋, tool detail 先選前面的來源, 不強制兩者取相同來源, `/api/snapshot` 與 `/api/codex/tool` 的查詢參數、欄位來源及按需讀取檢查不變

新增排序記錄只涵蓋既有 FILE_LIMIT 內的路徑, 不持久化本文, discovery 不完整、來源超出 working set 或鍵集合不吻合時不套用此排序, overflow 分支、容量、淘汰及來源讀取數量維持原行為

Python CodexCollector 隔離 fixture 確認原始來源順序、mtime rescan、相同時間路徑決勝, 並分別核對 snapshot 與 tool detail, Rust 回歸先失敗後通過, HTTP 確認啟動、checkpoint 重啟、來源通知重排及對應 detail

本段僅完成完整 working-set 的合併 / detail 遍歷, 不代表所有排序已對齊, call 插入順序、跨 active / overflow 全域順序、其他來源預覽、restore 容量與 16 MiB 快照組裝仍待完成, 本段基底為 7b305f8

最終含 detail 遍歷的版本以 Linux / Rust 1.100.0 執行完整 `--locked --offline` 單執行緒回歸, service binary 306 項全數通過 (97.56 秒), HTTP 205 項全數通過 (144.45 秒), 未延伸為並行穩定性、原生平台或真實資料驗收

### 完整 working set 的同時額度選取

額度 `updated_at` 相同時, 完整 working set 改取 discovery-time 順序中的第一個來源, 對齊 Python `max` 的穩定選取, 較新的額度仍優先, mtime 變更只在 rescan 後影響選取, 使用停用與收集停用仍不公開額度

共用既有完整來源排序檢查, 不新增來源讀取、資料保存或容量, discovery 不完整 / overflow 維持既有 fallback, 全域來源順序仍待完成, `/api/snapshot` 查詢參數與額度欄位來源不變

基底 53cefcb, LAM main 5c03daf 與 workspace main 4616074 已再次核對未變, Python CodexCollector 隔離 fixture 確認同時額度 / rescan / 路徑決勝, Rust 回歸先失敗後通過, allowance 5 項與完整來源排序 2 項通過, HTTP 啟動 / checkpoint 重啟 / rescan 回歸通過, 本段未重跑全套或原生平台驗收

同批補強來源順序的 metadata 驗收: 相同 context / token timestamp 時, model、reasoning_effort、execution 與 tokens 由後來源覆蓋, 與額度的前來源決勝分別驗證, Python 原始 fixture 與 HTTP 啟動 / checkpoint 重啟 / rescan / 路徑決勝全部通過, 不需新增 runtime 修改

本批最終完整單執行緒 `--locked --offline` service binary 307 項全數通過 (82.29 秒), HTTP 205 項全數通過 (155.81 秒), Linux / Rust 1.100.0 合成資料驗證, 未延伸為並行、原生平台或真實資料驗收

### 完整 working set 的 session error 分組順序

未由保留歷史提供位置的 live session errors, 改依 Python 首次出現的 thread 順序分組, 同 thread 內保持 discovery 來源順序, 修正相同 timestamp 的 session events 被檔名排序打亂, 四個時間窗與 logs 保留一致排序

只調整完整 working set 的發布遍歷, 不改寫保留歷史、保存上限、事件內容、來源讀取或按需 detail, 已有歷史位置仍依原規則保留, discovery 不完整與 overflow 維持既有 fallback, tool-result / call 事件交錯與全域排序仍待完成

基底 ab31b23, LAM main 5c03daf / workspace main 4616074 已核對未變, Python fixture 確認三來源兩 thread 的順序為 z,a,m, 不是字母排序 a,m,z 或平鋪來源 z,m,a, Rust 回歸先失敗後通過, 最終四窗 / logs / history / fallback 回歸通過, 既有 error 單元 11 項與 HTTP error 20 項通過, 未宣稱完整或跨平台驗收

同批將各 thread 的 tool-result errors 接在該 thread 的 session errors 後面, 修正同時事件被拆成「所有 session 後才是所有 tool」的順序, 新增的暫存排序僅持有既有事件參照, 不複製本文、改寫歷史或增加來源讀取, 同 thread 多 call 插入順序仍未完成

Python 原始 fixture 與先失敗後通過回歸確認 z,a,tool,m, 四窗測試另確認 1h 排除開始時間在窗外的 call failure, 即使完成時間仍在窗內, logs 僅針對 session / tool_result 比較順序, 不排除原有 monitor 啟動事件, 初版測試誤含窗外 call 並把 monitor 訊息納入期望, 已修正測試而非改動 runtime 篩選

最終版本以 Linux / Rust 1.100.0 執行完整 `--locked --offline` 單執行緒回歸, service binary 308 項全數通過 (91.28 秒), HTTP 205 項全數通過 (142.64 秒), 保留並行、原生平台與真實資料驗收限制

### 完整 working set 的 tool-result call 插入順序

工具錯誤預覽改用獨立的記憶體呈現序號, fresh parse 保持 call 首次插入位置, 同 call 更新只替換內容, 跨來源合併保持首次位置與後來源內容, checkpoint 載入依既有 reader 資料列順序重建, 還原後新 call 接在尾端

Python checkpoint 的實際規則是 `event_time DESC, call_id`, 只有 timestamp 相同才依 call ID 排序, 此前相同時間 fixture 的字母排序不能推廣為一般還原規則, 已以不同時間合成資料確認 live z,a,b 與 restore b,a,z, 還原後新增 q 並更新 a 為 b,a,z,q

呈現序號不寫入 call / cursor payload, 不取代原本 eviction sequence, 不增加保存欄位、保留數量、讀取來源或調整 50,000 / 8,192 等容量, `/api/snapshot` 參數、欄位來源與 detail 不變, 只有完整 working set 的 tool-result errors 使用新順序, 其他預覽與 overflow 全域順序仍待完成

基底 b5c9601, LAM main 5c03daf / workspace main 4616074 已核對未變, Python 實際 save/load/append fixture 與 Rust 回歸先失敗後通過, 新增跨來源選取 / 重複覆蓋 / 序號不持久化檢查, snapshot merge 20 項、HTTP checkpoint 22 項及 HTTP error 20 項通過, 最終含還原後 append 的回歸另行通過, 本段未重跑全套

提交前已完成本批全套單執行緒 `--locked --offline` 回歸: service binary 310 項全數通過 (109.93 秒), HTTP 205 項全數通過 (151.96 秒), Linux / Rust 1.100.0, 未變更 eviction / capacity, 保留其他預覽、overflow 與原生平台驗收缺口

### 完整 working set 的工具事件預覽順序

thread.tool_events 的同時事件改依既有呈現序號決勝, 時間倒序仍優先, fresh parse 同時項目保留首次插入位置, checkpoint 依原始載入順序, 不改欄位、查詢參數或來源讀取

RowTools 仍只保留 100 筆預覽, 以有界排序鍵加入序號, 不建立全量 call 參照陣列, tools / nested_tools 與總次數不受預覽截取影響, 停用仍不產生預覽, incomplete / overflow 使用既有 call ID fallback, communication、file 等其他預覽尚未改動

基底 2c1a88c, LAM main 5c03daf / workspace main 4616074 已核對未變, Python 實際 save/load 確認 fresh b,z,a 與 restore b,a,z, Rust 回歸先失敗後通過, snapshot merge 21 項、140 calls 的 100 筆上限 / fallback / 停用測試與 HTTP tool 15 項通過, 本段未重跑全套

同批 JEV call preview 使用相同呈現順序開關, 保留時間優先與 call 內 operation index, 100 筆事件上限、停用與 fallback 不變, 不改 MCP 觀測或 command 解析, Python 實際 save/load 確認 fresh z,a / restore a,z, HTTP 回歸先失敗後通過, JEV HTTP 13 項及含 140 calls / 每 call 2 operations 的有界預覽測試通過

本批最終完整單執行緒 `--locked --offline` 回歸通過: service binary 312 項 (99.07 秒), HTTP 206 項 (146.91 秒), Linux / Rust 1.100.0 合成資料, communication / file / 全域 overflow 順序與其他既有容量、平台待辦未宣稱完成

### 完整 working set 的 agent communication 預覽順序

agent_messages 的同時 outgoing calls 改用既有呈現序號, incoming 仍在相同 timestamp 的 outgoing 前面, 保留每 call 的 message index 與時間優先, 100 筆有界預覽、不保留本文及 incomplete / overflow fallback 不變

Python 實際 save/load 確認 incoming 優先、fresh z,a / restore a,z, HTTP 回歸先失敗後通過, 檢查 snapshot 與 checkpoint call / cursor payload 都不含合成訊息本文, communication 單元 2 項、HTTP 2 項、snapshot merge 23 項通過, 另驗證 2 incoming + 140 calls / 每 call 2 messages 仍只保留 100 筆且 index 正確

基底 6f63115, LAM main 5c03daf / workspace main 4616074 已核對未變, 未新增查詢、修改欄位來源、保存容量或 eviction, file / 其他預覽與全域 overflow 排序仍待完成, 本段未重跑全套

同批 thread.file_reads / file_changes 使用呈現序號決定同時 call 順序, 保留 call 內 file-operation index, 每種 200 筆預覽、read / change 篩選、停用與總量統計不變, 全域 file_activity 仍保留既有排序, 不宣稱跨 thread 排序已完成

Python 實際 save/load 確認各種 fresh z,a / restore a,z, HTTP 回歸先失敗後通過, 合成 operation 只供靜態辨識, 未執行工具或讀取檔案本文, snapshot 無合成本文, 250 calls / 750 operations 的單元回歸確認上限、index、總量與停用行為

本批完整單執行緒 `--locked --offline` 回歸通過: service binary 314 項 (104.34 秒), HTTP 208 項 (159.93 秒), Linux / Rust 1.100.0 合成資料, 全域 file_activity / 其他預覽、overflow、restore 容量與大型快照、原生平台驗收仍待完成

### 完整 working set 的全域 file_activity 預覽順序

沿用首次發現 thread 的順序與 call 呈現序號, 對齊 Python `threads.values()` / `calls.items()` 收集後的穩定 timestamp 排序, 保留 timestamp 優先與 call 內 operation index, 只在完整來源集合啟用

重用既有 error source 的 thread 順序索引, 不新增來源讀取或查詢, 不改 API、欄位來源、1000 筆預覽上限、操作統計、篩選、停用、row 預覽或 incomplete / overflow fallback, 容量與 eviction 不變

Python 實際 save/load 確認較晚修改來源的 thread 優先, live call 為 z,a、restore 為 a,z, 跨 thread HTTP 先失敗後通過, 2400 operations 的單元測試驗證 1000 筆上限與索引順序, 既有 canonical fallback 測試保留

本批完整單執行緒 `--locked --offline` 回歸通過: service binary 315 項 (109.51 秒), HTTP 209 項 (149.02 秒), Linux / Rust 1.100.0 合成資料, 基底 c8e8a4f 已核對遠端一致, 其他全域預覽、overflow、restore 容量、大型快照與原生平台驗收仍待完成

### 完整 working set 的 Git / checks 全域預覽順序

原始 Python 依 thread / call 插入順序收集後穩定排序, Rust 完整來源集合重用 thread 順序索引與 call 呈現序號, 保留 timestamp 優先及 call 內 operation index, 每種 500 筆預覽上限、完整 Git 統計、時間篩選、停用與 incomplete / overflow fallback 不變

Python 實際 save/load 確認 thread2 優先, live call 為 z,a、restore 為 a,z, HTTP 回歸先失敗後通過, fixture 指令僅供靜態辨識, 未執行其中 Git 或測試指令, 不新增 API 或查詢、不變更欄位來源、容量、eviction 或保存策略

每種 1200 operations 的單元測試確認 500 筆上限、thread / call 順序、Git index 與統計, 既有 fallback 回歸保留, 首次全套新增 fixture 缺少 sequence 而失敗, 補齊 fixture 後 targeted 2 項與完整單執行緒 `--locked --offline` service binary 316 項 (83.25 秒)、HTTP 210 項 (157.17 秒) 通過

驗證為 Linux / Rust 1.100.0 合成資料, 基底 83159ec 已核對遠端一致, 其他全域預覽、overflow、restore 容量、大型快照與原生平台驗收仍待完成

### Retained skill 的目前工具欄位來源

Python `collectors.py` 在合併 retained skills 時, 若目前 thread / call 存在且符合時間窗, 以目前 `call.tool` 更新回應中的 `tool`, 即使該 call 已不含相同 skill, Rust 改為相同行為, 未修改 retained timestamp、has_document 或其他欄位

HTTP 合成 fixture 原先 `tool` 為 null, 修正後顯示 replacement tool, Python 實際 refresh 與 HTTP 先失敗後通過, 單元驗證過期或不存在的 call 不覆蓋 retained tool、停用與輸入 history 不變, 只在既有最多 500 筆 history 索引暫存工具名稱, 不改查詢、API 路徑、保存、容量、eviction、預覽排序或上限

Skill 全域排序仍待完成, 原始保留插入順序與 SQLite `event_time DESC, item_key DESC LIMIT 500` 之間的差異涉及保留集合, 未藉此工作變更保留策略

本批 targeted 2 項及完整單執行緒 `--locked --offline` service binary 317 項 (95.94 秒)、HTTP 211 項 (145.30 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 59e6cd6 已核對遠端一致, overflow、restore 容量、大型快照與原生平台驗收仍待完成

同批補齊實際 checkpoint restart 證據: Python 與 Rust 重啟後仍由目前 call 補入 tool, SQLite retained skill payload 不新增 tool, timestamp / has_document 保留, 最新 skill HTTP 9 項通過 (3.72 秒), 此補充只擴充測試, runtime 未再修改, 上述完整回歸仍為補充前的已驗證結果

### 完整 working set 的 thread 列表同時間順序

Python thread 列表以 updated_at 穩定排序, 同時間保留首次來源順序, Rust 改用既有 thread ordinal 作為完整來源集合的同時間 tie-breaker, updated_at 仍優先, incomplete 或沒有來源順序時維持原有順序, 不改 thread 選擇、欄位、分頁、查詢、保存或容量

Python 實際 save/load 與修改來源 mtime 後重啟確認 [3,2,1] / [3,1,2], 較新 updated_at 的 thread3 始終優先, HTTP 先失敗後通過, 單元測試另覆蓋 discovered count 不完整與缺少來源順序的 fallback, 未宣稱 metadata-only 來源順序或 overflow 全域順序已對齊

本批完整單執行緒 `--locked --offline` service binary 318 項 (111.83 秒)、HTTP 212 項 (162.97 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 0ea2399 已核對遠端一致, 其他既有待辦保留

### 重複 call 的 SQLite result 欄位來源

Python 相同 call ID 的後續請求保留既有 error, 即使前次請求不是 SQLite, 後續 SQLite 事件仍應顯示 failed, Rust 原先只檢查 sql_failed 而顯示 returned, 現在投影時同時採用已存在的 failure metadata, 未新增錯誤保存或變更 checkpoint flag

Python 實際 save/load 與 HTTP 先失敗後通過, 重啟後維持 failed, 後續成功輸出恢復 returned, 合成 SQLite 指令只供靜態辨識, 未執行 SQL 或建立來源資料庫, 單元涵蓋已知 failure、sql_failed、completed 組合且序列化 checkpoint 不變

此範圍只使用已存在的錯誤證據, 未補蒐集原先停用或未保存的 failure, 不改查詢、事件數、保留策略、容量或 eviction, SQLite 觀測仍依既有流程發布與保存 result

本批完整單執行緒 `--locked --offline` service binary 319 項 (108.93 秒)、HTTP 213 項 (160.94 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 2f2d3c1 已核對遠端一致, 其他既有待辦保留

### 完整 working set 的已載入來源路徑清單

對照 Python `SOURCE_LOCATION_LIMIT = 20` 與 `server.py` 的 session source registry, 完整來源集合在 `codex.read_state.locations` 列出前 20 個來源路徑、`listed_file_limit = 20`, 並加入 `sources.session.locations` 的 session 根資料夾之後

重用現有已排序來源清單, 每次 snapshot 組只建立一次最多 20 個路徑, 不額外讀檔或查詢, 清單是顯示上限而非收集或保留上限, incomplete / 缺少來源順序維持原先不提供路徑清單的 fallback, 停用仍僅顯示原有根資料夾

Python 實際 save/load 驗證 25 個來源只列前 20 個且依發現順序, HTTP 先失敗後通過並涵蓋 restart / disabled, 既有單元補驗不完整與缺少來源順序時不發布清單, overflow 路徑列舉未宣稱完成, 容量與 eviction 不變

完整單執行緒 `--locked --offline` service binary 319 項通過 (111.16 秒), 首次 HTTP 為 213 通過 / 1 失敗, 既有 retention cleanup retry 測試讀到設定 90 而非 1, 原程式未修改 retention 或該測試, 單項重跑通過 (0.46 秒)、完整 HTTP 重跑 214 項通過 (155.80 秒), 原始失敗證據保留, 未宣稱已查明偶發失敗根因

Linux / Rust 1.100.0 合成資料, 基底 5265a3d 已核對遠端一致, overflow、容量、大型快照與原生平台待辦保留

### Session source 的 enabled_features 欄位來源

對照 Python collector 與 `server.py` source registry, `codex.read_state.enabled_features` 只列出 session collector 已啟用功能, 依既有宣告順序, `sources.session.observations` 改取該清單, 不再把全域設定中的 account、platform 或其他來源功能誤列為 session observation

不變更開關或收集行為, 不新增檔案讀取或查詢, Python synthetic collector 驗證預設與停用清單, HTTP 先失敗後通過並涵蓋設定變更與 codex disabled, 單元測試確認清單來源優先、順序及空值 fallback, 停用時 registry observations 為空

本批完整單執行緒 `--locked --offline` service binary 320 項 (109.88 秒)、HTTP 215 項 (158.96 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 9c109cc 已核對遠端一致, 先前 retention retry 偶發失敗證據與未明根因仍保留, 本次通過不代表已修復該問題

### Checkpoint reader 描述與整數 limits

對照原始 `Dashboard.sources`, 只列出 health 非空的 load / save reader, 帶入對應 checked_at, 未提供時間時明確為 null, write health 空字串時使用 load health, session limits 接受整數與布林、checkpoint limits 只接受整數, 不把浮點數列為限制值

Python 原始 sources 方法以合成依賴執行確認, 單元先失敗後通過, HTTP 補驗 native reader 的 checked_at 欄位存在且為 null, 沒有新增或推測實際檢查時間, 不變更 checkpoint 的 location、保存流程、健康狀態來源、容量或 retention

本批完整單執行緒 `--locked --offline` service binary 321 項 (106.84 秒)、HTTP 215 項 (161.79 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 9c0f44c 已核對遠端一致, 先前偶發 retention retry 與其他待辦仍保留

### Retention retry fixture 的背景清理競爭條件

針對先前 90 / 1 設定差異, 依現有 `main.rs` 背景 cleanup 與 history-store transaction 語意建立明確交錯回歸: 2000 年 fixture 同時符合預設 90 天清理, 若先刪前 500 筆而剩下 trigger 保護的 1 筆, 後續 1 天設定在第一個 transaction 就失敗回滾, 因而回 503 且設定維持 90

已實際重現相同結果, 新增獨立回歸保留此行為, fixture 建立與 trigger 安裝採同一測試 transaction, 原 retry 測試改用 2-3 天前的合成資料, 明確驗證 90 天清理為 0 筆, 再保留原有 1 天清理 partial commit、失敗及 retry 成功斷言

僅修改測試資料與回歸, 沒有修改產品 retention、刪除、容量或 transaction 行為, 原始失敗 log 保留, 已確認可產生該症狀的競爭條件, 但原始那次執行沒有交錯 trace, 不宣稱回溯證明其實際排程

同輪完整 HTTP 首次另有 `source_update_recovers_unreadable_aggregate` 失敗, history_health 已恢復 ok 但 observed_tool_calls 仍為 0, 單項原程式重跑也重現, 測試原先只等待 health 就斷言來源已收集, 現在等待 health 與原本要驗證的 calls=1 同時成立, 原有 aggregate / history_aggregate / calls 斷言完整保留, 不修改 runtime, 另存這次全套與單項失敗證據

兩項 retention 測試連續 20 輪通過, aggregate 測試另連續 20 輪通過, 修正後第二次完整 HTTP 又遇獨立的 `read_only_history_rejects_retention_update_without_changing_data` GET settings 503 / 200 差異, 單項原程式重跑通過 (0.59 秒), 該測試與產品程式碼未改, 原因尚未確認, 未歸類為相同競爭條件

最後完整單執行緒 `--locked --offline` HTTP 216 項通過 (158.81 秒), 前兩次全套各為 215 通過 / 1 失敗, 分別保留 aggregate 與唯讀錯誤證據, service 321 項為前一批已驗證且 runtime 未變的結果, 本輪未重跑, Linux / Rust 1.100.0 合成資料, 基底 d290a24 已核對遠端一致

### Source registry 的欄位排序與 account reader 順序

對照 Python `Dashboard.sources`, Jev telemetry source 的 fields 應為 summary key 排序後的清單, Rust 原先保留 summary 插入順序, HTTP 已先重現差異, 現在只排序既有欄位名稱向量, 不更動 summary、資料庫查詢、請求數、事件順序或保存集合

另核對 account reader 應保留 protocol 宣告順序, 現有 serde_json preserve_order 相依功能已符合, 僅補單元與 fake CLI HTTP 回歸, 未新增真實帳號查詢, Python 以合成依賴執行原始 sources 方法確認兩種順序規則

完整單執行緒 `--locked --offline` service binary 322 項 (107.72 秒)、HTTP 216 項 (162.27 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 c2a57b0 與遠端一致, LAM main 5c03daf / WBUIWS main 4616074 未變, 先前唯讀 settings GET 503 仍屬未明原因, 本次通過不代表已修復, 其他容量、overflow、保留策略與平台驗收待辦不變

### Dots output preview 的衍生電腦與活動範圍

Python `project_dots` 先依時間排序並取既有 500 筆 output preview, 再衍生 computers / activity, Rust 原先使用全部最多 2000 筆讀入候選, 因而把只出現在更舊 output 的電腦也列入

現在提前使用既有穩定排序, 只在衍生活動時使用前 500 筆, 完整候選仍保留供 window 選擇與 total 計算, 不改來源讀取上限、顯示上限、保存容量或 eviction, `/api/codex/dots` 與背景 snapshot 使用相同投影, 沒有新增檔案或網路請求

Python 原始 project_dots / read_dots 與 HTTP 合成 501 筆輸入確認 total 501、events / activity 500、排除只在最舊 output 出現的電腦, HTTP 先失敗後通過並涵蓋 all / 1h、背景 snapshot 與私密內容不輸出, 單元與 Python 另確認同時間的插入順序, focused dots HTTP 4 項通過

完整單執行緒 `--locked --offline` service binary 323 項 (107.11 秒)、HTTP 217 項 (167.37 秒) 通過, Linux / Rust 1.100.0 合成資料, 基底 326c15b 與遠端一致, LAM main 5c03daf / WBUIWS main 4616074 未變, 先前唯讀 settings GET 503 仍未明原因且保留原始失敗證據, 本次通過不代表已修復, 其他既有待辦不變

### Dots computers 同時間的首次 host 順序

Python `project_dots` 的電腦列表在相同或缺少活動時間時保留首次遇到 host 的順序, Rust 原先由 BTreeMap 名稱順序決定, 現在於既有 host 聚合記錄暫時 ordinal, 只用於相同時間的 tie-breaker, 重複 host 不改首次位置, 較新活動時間仍優先

Python 與 HTTP 先重現 new / a / z 與預期 new / z / a 的差異, focused dots HTTP 5 項通過並涵蓋直接端點與背景 snapshot, Python 與單元另驗證 101 個未提供時間的 host 依首次順序保留前 100 個, 不改既有顯示上限、收集、保存容量、retention 或 eviction

此工作保留目前 metadata 載入順序, 未宣稱 metadata-only 上游順序已與 Python 對齊, 不新增檔案或網路請求

完整單執行緒 `--locked --offline` service binary 324 項通過 (145.41 秒), 首輪 HTTP 為 217 通過 / 1 失敗 (209.38 秒), `error_history_restores_recent_metadata_after_sources_disappear` 在停用 codex 後讀到 errors.total=2 而非 0, history health 已為 disabled, 原測試單項重跑通過 (2.99 秒), 未修改其斷言或產品行為

獨立合成單筆錯誤重啟確認 disabled refresh 仍載入 error_history, summary 會發布 health=disabled / total=1 / retained=1, fixture 沒有 global-state 檔而不會進入本輪變更的 dots project, 相關 disabled refresh / error summary 原始碼未改, 因此本次排序改動沒有造成該錯誤路徑, 但完整測試未全數通過, 原始失敗與獨立重現腳本保留, 停用時 retained errors 的呈現與測試期望仍待獨立處理

Linux / Rust 1.100.0 合成資料, 基底 d224a83 與遠端一致, LAM main 5c03daf / WBUIWS main 4616074 未變, 先前唯讀 GET 503 未明原因另行保留, 不將兩者視為同一問題

原始 `Dashboard.snapshot` 以合成依賴實際驗證: codex=false 且 errors=true 仍顯示 retained error, total=1 / retained=1 / history.health=ok, errors=false 才隱藏 retained preview, 因此失敗測試的 codex disabled → total 0 期待不符合 Python, Rust refresh 後保留資料本身不是錯誤, 另有 history.health 直接標為 disabled 的欄位差異, 當時提出最小修正與回歸方案, 後續實作與驗證見下一節

### 停用 codex 時的 retained error 顯示與儲存健康狀態

依原始 Python 語意, 停用 codex 收集不會隱藏已保存的錯誤, 只由 errors observation 控制 preview, Rust 現在於停用啟動時先讀既有 bounded error preview, 並依儲存讀取狀態呈現 health, 不再強制標示 disabled, 啟動增加一次既有 error preview 有界讀取, 不讀原始來源、不新增網路請求、不刪除 SQLite 資料

原測試改為確切驗證啟動後 retained 2 / total 2 / health ok, errors 關閉後 preview 為 0、重新開啟後回到 2, 各次直接比對 SQLite errors 的 item_key / payload 完全不變, 原有 restart、record hash 與完整歷史斷言保留, 新單元另驗證停用收集時真實讀取錯誤仍呈現 unavailable

新增測試初版曾錯誤地在 errors 關閉時要求受限 history API 回 200, 該 400 又新增 monitor error 影響精確總數, 已移除這個非必要請求而改直接唯讀比對 SQLite, 兩次新增測試失敗 log 均保留, 沒有變更 API 權限或錯誤記錄行為, 獨立合成重啟也確認修正後直接得到 health ok / total 1 / retained 1

修正後 retained error 回歸連續 10 輪通過, 新 storage health 單元通過, 完整 service binary 325 項通過 (161.26 秒), 第二輪完整 HTTP 為 217 通過 / 1 失敗 (215.36 秒), 此次為既有 `overflow_tool_detail_uses_verified_source_and_masks_secrets` 在 POST metadata=false 後收到非 200 / 202, 原斷言未記錄實際 status / body

該 overflow 測試未修改, 單項原程式重跑另在 HTTP read_to_end 發生 WouldBlock / Resource temporarily unavailable (19.70 秒), metadata 單一設定原始路徑等待發布 5 秒後可能回 503 Settings temporarily unavailable, 但首次記錄不足以確認實際回應, 不將兩次失敗或先前唯讀 GET 503 視為同一原因, 不反覆重跑直到全綠, 本 checkpoint 保留失敗狀態、原始 log 與未驗收項目, 未改 overflow、timeout、容量或測試斷言

目前變更可分別審查: Dots 只改暫時 host 排序, 最新獨立 dots HTTP 5 項通過 (1.35 秒), retained-history 只增加停用啟動的 bounded read 與 health 判斷, 最新單元及 10 輪 HTTP 已驗證, SQLite payload 不變, 兩範圍均無呼叫 overflow metadata 設定等待路徑的改動, 完整 HTTP 仍保留失敗結果, 下一步需獨立取得 overflow 失敗的實際 HTTP response 或 request timeout 階段, 不先假定其根因

### Overflow HTTP 失敗的有界診斷

獨立腳本固定執行兩輪相同 5001-source 合成情境, 第一輪採 5 秒 POST timeout、第二輪僅診斷用 12 秒 timeout, 實際均於期限內回 200 (3.668 / 3.055 秒), 已保存 status / body、各階段時間與受測程序狀態, 後續 metadata 已隱藏、工具內容遮蔽正常, 兩個受測程序均正常停止且 stderr 為空

原 overflow 測試只增加階段訊息與失敗時的 status / body, 保留所有斷言、5 秒 timeout 與容量設定, 加入診斷後只執行一次, 通過 (13.43 秒), 不覆蓋先前兩份失敗記錄, 不據此改稱完整 HTTP 通過

靜態呼叫路徑確認此 fixture 為 codex enabled 且沒有 global-state, 因而不執行新增的停用啟動讀取或 dots host 排序, enabled 時 history health 的結果也未變, 設定等待與 overflow 來源路徑沒有修改, 但未取得原失敗時的 HTTP response 或完整 request 階段, 尚不能斷言是環境負載或排程原因, 目前保留獨立失敗狀態, 後續自然重現時可由新增診斷辨識階段與實際回應

### Remote summary 活動的同時間順序

復原後確認 LAM main `5c03daf` 與 workspace main `4616074` 未變, 原始 Python `desktop_activity` 依 entries 首次插入順序產生 dot / orbit 活動, 相同時間的 stable sort 保留此順序, Rust 原先最後依 BTreeMap thread ID 順序產生活動

新增暫時 entry_order, 保留既有 metadata 順序, remote summary 首次加入時追加 ID, 重複更新保留原位置, 不改來源讀取、時間比較、preview 上限或保存策略, metadata-only 上游順序差異仍未處理

原始 Python 的反向 ID、相同時間、重複更新 fixture 已驗證順序與 thread_count, 既有 desktop activity 8 項測試通過, diff whitespace 檢查通過, 新 Rust 單元涵蓋相同 fixture, 最初因工具鏈缺失而尚未執行

替換工作環境最初沒有 cargo / rustc 或原相依套件快取, 已檢查既有 runtime、系統及工作區與備份, 未安裝工具或存取其他裝置, 當時 Rust 編譯、格式與單元 / HTTP 測試均未執行, 既有完整 HTTP 217 / 1、overflow socket timeout 與獨立唯讀 503 仍未解決

取得安裝授權後, 已由官方 rustup 安裝獨立 Rust / Cargo 1.100.0 與 rustfmt, 依既有 Cargo.lock 取得相依套件且 lockfile 未變, 新順序單元通過, 完整 service 326 項通過 (128.90 秒), 完整 serial HTTP 單輪 218 項通過 (156.73 秒), 本輪已恢復執行驗證

rustfmt check 仍回報既有 dots.rs 格式, 比對 HEAD 與目前檔案的格式修正內容完全相同, 新增呼叫已符合格式, 未為此重排既有程式碼, diff whitespace 檢查通過

本輪通過不代表先前 HTTP 217 / 1、overflow WouldBlock 與唯讀 503 的根因已解決, 原始記錄全部保留, 不變更保存、容量、timeout 或斷言, 此處驗證不等同遠端交付

### Dots 日期的原始 Python 日曆範圍

基底 `b2657d9` 已與遠端一致, LAM main `5c03daf` 與 workspace main `4616074` 未變, 原始 `codex_metadata.date` 透過 datetime 限制年份為 1 到 9999, Rust chrono 原先接受第 0 年或 10000 年而輸出不同日期字串

Dots 日期投影增加相同年份範圍檢查, 超出範圍保留 null, 不改數值型別檢查、毫秒轉換、來源讀取、排序、容量或保存策略, 原始 Python 合成 fixture 驗證兩側有效邊界、超界 null 與投影排序, 新 Rust 回歸在修正前實際失敗, 修正後 Dots 單元 4 項與 HTTP 5 項通過, 既有 Python desktop activity 8 項通過

所有先前獨立失敗與未完成驗收仍保留, 此次修正不處理 metadata-only 上游順序、overflow、SQLite 保存或唯讀 503

完整 service 327 項通過 (112.61 秒), 完整 serial HTTP 單輪 218 項通過 (154.30 秒), 兩者使用 Rust 1.100.0 與既有 lockfile 的 offline 執行, 原始失敗仍不據此宣稱已找到根因

### Dots 日期的微秒捨入順序

基底 `824d88c` 已與遠端一致, 原始 Python datetime 先將浮點秒數捨入至微秒, 再以毫秒輸出, Rust 原先直接向下取整毫秒, 因而將 0.9999996 秒輸出成 00.999 而非 01.000, 新回歸已在修正前重現

現在分離整秒與帶正負號的小數, 微秒採 ties-even 捨入後再換算毫秒, 使用 checked arithmetic 並保留非有限值、缺值及日曆範圍的 null 行為, 不改來源讀取、preview 上限、排序方式或資料保存

原始 Python 的 6 個明確案例與 128 個固定正負小數 / 日曆邊界比較通過, Dots 單元 5 項、Python desktop activity 8 項通過, 完整 service 328 項通過 (110.43 秒), 完整 serial HTTP 單輪 218 項通過 (156.84 秒), 完整檢查後僅調整新測試陣列換行與本段文件, runtime 未再變更

先前 HTTP 217 / 1、overflow WouldBlock 與獨立唯讀 503 的原始證據及未解決狀態持續保留

### 非物件 Dots atom 的缺值語意

基底 `21f504d` 已與遠端一致, LAM main `5c03daf` 與 workspace main `4616074` 未變, 原始 Python 在 electron-persisted-atom-state 明確為非物件時略過衍生電腦資料, events / activity_items 為 null, query 的 activity 為空陣列且 health 維持 ok, 欄位不存在則仍視為有效空物件

Rust 原先將這兩種情況合併, 可能以既有 catalog 產生 computers, 現在只在明確非物件時提前回傳原始缺值欄位, 保留欄位不存在的行為, 不新增讀取、改動容量或保存策略

原始 Python null、布林、數值、字串與陣列 5 個案例及缺少欄位比較通過, 新 Rust 回歸在修正前失敗, 修正後驗證不因既有 catalog 而產生衍生資料, Dots 單元 6 項、Python desktop activity 8 項通過, 完整 service 329 項通過 (98.38 秒), 完整 serial HTTP 單輪 218 項通過 (153.03 秒)

HTTP 通過記錄仍含既有合成資料情境的 history cleanup unavailable 訊息, 完整 log 保留, 未抑制或修改儲存行為, 先前獨立 HTTP / WouldBlock / 唯讀 503 失敗仍未宣稱已解決

### Core SQLite 日期的小數精度與詳細資料身分

基底 `d7112c7` 已與遠端一致, LAM main `5c03daf` 與 workspace main `4616074` 未變, 原始 error_records 以 datetime.fromtimestamp 解析 SQLite ts, Rust 原先將完整 epoch 秒乘上 1000000 後捨入, 在現代 epoch 會遺失小數精度, 例如 1700000000.0009995 原始 Python 為 .000, Rust 原先為 .001

Core timestamp 現在分開整秒與小數微秒捨入, 保留年份、型別及非有限值驗證, 同一函式繼續供收集投影與 on-demand row timestamp 核對使用, 不新增查詢或改變查詢條件、body 讀取上限、遮蔽及檔案身分檢查, 衍生 content_id 對齊原始時間, 不重寫既有 SQLite 或 retained history, 既有錯誤捨入時間若與來源不符仍拒絕詳細資料, 不放寬核對

原始 Python SQLite REAL 3 個正負 epoch 案例通過, 新 Rust 回歸修正前失敗, 修正後核對正確時間可讀 detail、錯誤時間被拒絕、SQLite ts 不變, Core 單元 14 項通過, 完整 service 330 項通過 (110.88 秒), 完整 serial HTTP 單輪 218 項通過 (150.14 秒)

Python error_records 共 21 項, 18 項通過、3 項因復原環境缺少 generated frontend manifest 而 error, 標準 verify-offline 頁面建置因無法取得或準備固定 WBUI 版本而失敗, 未變更 pin 或認證, 兩份記錄保留, 不宣稱 Python 全套通過, 先前獨立 HTTP / WouldBlock / 唯讀 503 問題與 cleanup 警告仍保留

### 復原固定 WBUI 資產並完成 Python 參考驗證

以獨立 clean checkout 核對 consumer pin `e9741c60590894fa8362f71bd0d4e8197caf4e5c` 與官方 origin 後, 使用標準 build_frontend --source / --verify-offline 建置成功, 未更新 pin、原始碼或 Git 認證, 本機 clone 最初的來源路徑 origin 曾被既有驗證拒絕, 完成來源核對並修正該獨立 clone 的 remote metadata 後才通過, 未略過檢查

原先缺少 frontend manifest 的 Python error_records 重新執行 21 項全部通過 (1.687 秒), 既有 frontend build 18 項通過 (1.071 秒), 先前 18 通過 / 3 errors 與失敗建置 log 仍保留, 此阻礙確認為已修復的復原環境前置條件, 不混同先前仍未解決的 HTTP / WouldBlock / 唯讀 503

僅恢復被忽略的 generated _web / _workbench, 沒有 runtime 原始碼變更, 私有 checkpoint 保存資產與 manifest 供再次復原, 本輪未重跑已通過的 Rust 330 / 218 檢查, 不宣稱跨平台或 live UI 驗收

### 暫時 thread metadata 的來源順序

基底 `9388b89` 與來源 main 比較未變, 原始 Python dict 保留 catalog、state 與 session_index 首次讀取順序, 重複更新不改位置, Rust HashMap 原先使相同時間的 metadata-only threads 與 remote activities 順序不穩定, Dots background 另轉為 BTreeMap 而依 ID 重排

僅將 thread metadata 與 session_index title 暫存改用既有 serde_json::Map, Dots 接續相同順序, metadata 更新比較明確納入順序以發布只有排列改變的 snapshot, Cargo feature tree 已確認 preserve_order 由現有 history-store / session-core 相依套件啟用, 沒有改 Cargo.toml、Cargo.lock、feature 或新增相依套件

公開輸出仍逐欄位投影, 沒有新增內部欄位或改變資料格式, 既有查詢、排序條件、ID 批次、數量上限與持久化 schema 均未變, 暫時容器配置形態改變但保留同一批 metadata, 不需資料 migration

OverflowWorkdirScan 的 staging HashMap、winner、cursor、budget 與保存策略未修改, 其 metadata 併入處只配合目標型別, 本輪不宣稱 overflow 衍生 metadata-only 順序與 Python 相同, 既有容量與資料完整性限制仍保留

32 筆同時間 catalog 的原始 Python fixture 驗證 metadata / remote schedules / Dots hosts 順序與重複值優先序, 新 Rust 回歸修正前實際失敗, 修正後通過, 另驗證 state reader 與 index 最新 title / 首次位置, 新 HTTP 回歸驗證 metadata-only thread 順序與只有資料列排列改變時的重新發布, Dots computers 與 schedules 值保持不變

Rust schedules 單元 12 項通過, 新 HTTP 回歸通過, 原始 Python schedules 5、subagent sources 2、desktop activity 8 項通過, 完整 service 332 項通過 (103.62 秒), 完整 serial HTTP 單輪 219 項通過 (153.39 秒), 原始失敗與 cleanup 警告仍保留, 本輪一併保留前述固定資產復原驗證

### Dots snapshot 欄位優先序與無效值界線

基底 `4ce131d` 與來源 LAM main `5c03daf`、workspace main `4616074` 重新核對未變, 原始 Python desktop_activity 的 primary key 存在但為 null / 無效值時不等於欄位缺少, threadId 存在時不退回 conversationId, 非數值 updatedAtMs 退回 updatedAt 而非 timestampMs, 數值超出日期範圍則維持 null, 不採用另一個有效時間

新增 10 組 source-backed Rust fixture, 涵蓋缺少、null、無效字串、布林值、0、超出日期範圍、無效 host 與私有欄位排除, 同時驗證公開 activity / computers 投影與 activity_items 原始計數, 原始 Python 10 組實際輸出一致, 確認既有實作已符合, 本輪只有測試與文件變更, 未改 runtime、查詢、schema、容量或保存策略

Rust Dots 單元 7 項通過, Dots HTTP 5 項通過, Python desktop_activity 8 項通過, 本輪未重跑完整 service / HTTP, 前一版 332 / 219 通過仍僅代表前一版檢查, overflow 順序排除與歷史獨立失敗、cleanup 警告不變

### Cloud metadata 的 Python 空白修整

原始 codex_cloud 投影沿用 codex_metadata.text, Python strip 也移除 U+001C 到 U+001F, Rust 原先 trim 未移除, 使純分隔字元顯示為非空標題, 並占用多字元標題的前段長度

Cloud text 改用既有 schedules::trim_text, 只影響 /api/codex/cloud 與共用快取投影的 title / environment_label, 仍先修整再依 Unicode 字元取 512 / 160 個, 字串中間的分隔字元與非空白 U+200B 保留, 來源欄位、CLI 命令、授權、快取、查詢次數、schema 與資料保存未改

新增回歸在修正前實際失敗, 原始 Python 驗證 5 種邊界空白、純空白、Unicode 長度與不應修整的內容, Rust Cloud 9 項、Cloud HTTP 3 項、Python Cloud 7 項通過, 合併前述 Dots regression 後完整 service 334 項通過 (114.49 秒), 完整 serial HTTP 單輪 219 項通過 (142.33 秒), 以上僅使用 synthetic fixtures, 未呼叫使用者已登入 CLI, 歷史獨立失敗與 overflow 排除範圍仍保留

### 結構化內容的邊界空白

基底 `44ba619` 與來源 LAM main `5c03daf`、workspace main `4616074` 核對未變, 原始 payload_detail.parse_text 先使用 Python strip, Rust parse / parse_document 原先拒絕以 U+001C 到 U+001F 包住的 JSON、XML、TOML

僅於兩個解析入口沿用既有 trim_text, 影響已選取的 tool / Context payload 與 MCP document parsed 投影, 不新增來源讀取或查詢, 保留原始文字、1 MiB 解析上限、行數 / 深度限制、既有格式辨識、遮蔽與資料保存, 不修改 grammar 內部空白規則

新回歸修正前實際失敗, 原始 Python 12 組格式案例與遮蔽 / 非遮蔽 / nested fixture 通過, Rust 同時檢查 preview、敏感欄位遮蔽、可執行運算式拒絕及超出容量拒絕, structured 9、details 10、MCP documents 6 項通過, Python payload_detail 12 項通過, 完整 service 336 項通過 (117.39 秒), 完整 serial HTTP 單輪 219 項通過 (154.02 秒)

探索時另發現原始 Python Object literal 可接受成員間 U+001F, Rust 仍拒絕, 最初期待 Python 拒絕的 fixture 假設失敗並保留記錄, 本輪未擴大語法處理, 此差異仍待完成, 原有 HTTP / WouldBlock / 唯讀 503、cleanup 警告與 overflow 排除範圍不變

### Object literal 內部 Python 空白

基底 `41bf964` 與來源 main 再次核對未變, 原始 operation_records.literal 的 space 使用 Python isspace, 包含 U+001C 到 U+001F, Rust session-core Literal::space 原先 trim_start 不包含這四個字元, 造成前段記錄的內部 U+001F 差異

僅調整 Literal::space 的空白判定, 同時影響 standalone Object literal、已記錄 exec 的靜態 nested invocation 辨識與既有 detail / document 投影, 不執行記錄中的工具或運算式, 引號內內容、重複 key 拒絕、可執行運算式拒絕、bytes / nodes / depth 上限與資料保存不變, 沒有新增讀取、查詢或持久化欄位

原始 Python 四種分隔字元的物件、陣列、nested invocation、字串保留及拒絕案例通過, 新 core 回歸修正前失敗, 修正後 core 34、service structured 10 項通過, parsed_format 與敏感欄位遮蔽另有回歸, Python payload_detail 12、MCP observation 29 項通過, 完整 service 337 項通過 (104.85 秒), 完整 serial HTTP 單輪 219 項通過 (151.66 秒)

第一次從 service manifest 指定相依套件 feature 的命令被 Cargo 拒絕, 未執行該項檢查, 改用 session-core manifest 與既有 lockfile 執行 sqlite-python feature 後 34 項通過 (0.93 秒), 兩份記錄均保留, 未更動 manifest / lockfile

本次完成前述內部 U+001F 的已確認差異, 不移除原始假設失敗記錄, 不宣稱所有 literal grammar 完全相同, 歷史 HTTP / WouldBlock / 唯讀 503、cleanup 警告與 overflow 排除範圍仍保留

### Object literal 非 ASCII digit escape 的待辦差異

基底 `2f06372` 與來源 main 核對未變, 原始 literal string 在 escape 字元或 ASCII 0 後使用 Python isdigit 判定, 會拒絕 Arabic、superscript、circled digit, 但 Roman numeral 與 fraction 不是 digit, 仍保留為文字, Rust 現行只有 ASCII digit 判定, 非 ASCII 案例仍待處理

已實測原始 Python 3.12.14 / Unicode 15 的 12 組案例, 現有 regex 的 Numeric_Type=Digit / Decimal 解析被拒絕, Decimal_Number 單獨不足以涵蓋 superscript / circled digit, regex 資料為 Unicode 16、Rust char 資料為 Unicode 18, 既有 unic identifier 資料為 Unicode 10, 不可視為相同版本或相同屬性

本輪僅調查與保存證據, 未修改 parser、相依套件或 lockfile, 不以 is_numeric 擴大拒絕 Roman numeral / fraction, 不新增手動維護 Unicode table 或執行 Python subprocess, exact Digit 的生成來源與版本策略尚待決定, 未重跑 Rust 全套, 前次 core 34 / service 337 / HTTP 219 不代表此差異已修復

### YAML subset 的換行與縮排來源語意

原始 parse_yaml 使用 Python splitlines, 以 ASCII space 計算縮排, 另檢查前置空白中是否含 tab, Rust 原先只分割一般換行並把其他 Unicode 空白視為縮排, 使 CR / Unicode 換行漏讀, 或把同層 key 誤判為巢狀內容

僅調整 YAML candidate / YAML line 預處理, 支援來源既有 10 種換行字元, 以 ASCII space 計算縮排, 保留前置 tab 拒絕與空白 / comment 略過, U+001F 不當成換行, 既有 1 MiB、20000 行、深度、遮蔽與保存界線不變, 不影響 JSONL 分割或新增來源讀取

新回歸在修正前於 CR 案例實際失敗, 原始 Python 換行、同層 key、空 parent、comment、tab / 非換行 / 行數拒絕 fixture 通過, Rust structured 11、MCP documents 6、Python payload_detail 12 項通過, 完整 service 338 項通過 (124.25 秒), 完整 serial HTTP 單輪 219 項通過 (151.34 秒)

本輪合併前述 Unicode digit escape 稽核記錄, 該差異仍未修改, 其他 YAML scalar / grammar 完整相容性不由本輪推定, 歷史獨立 HTTP / WouldBlock / 唯讀 503、cleanup 警告與 overflow 排除範圍保留

### YAML subset 非換行空白的 scalar 與 sequence 語意

基底 `50a085c` 與來源 main 核對未變, 原始 Python 將 U+001F 視為空白而非換行, scalar 修整、key 冒號後分隔、sequence body 與 inline comment 判定均使用此語意, Rust 原先可能拒絕內容或將布林值誤保留為字串

僅對齊上述 YAML 空白判定, 沿用既有 trim_text 並補齊來源 regex 的空白範圍, 引號內 U+001F 仍保留, tags / anchors / 重複 key 仍拒絕, 已選取內容的遮蔽投影、行數 / 深度 / bytes 上限與資料保存不變, 沒有新增查詢、來源讀取、相依套件或 schema

新回歸修正前於 key 分隔案例實際失敗, 原始 Python 6 組 scalar / sequence / comment 案例、3 組拒絕與敏感欄位遮蔽通過, Rust structured 12、MCP documents 6、Python payload_detail 12 項通過, 完整 service 339 項通過 (113.92 秒), 完整 serial HTTP 單輪 219 項通過 (147.88 秒)

Unicode digit escape 待辦未修改, 歷史獨立 HTTP / WouldBlock / 唯讀 503、cleanup 警告及 overflow 排除範圍仍保留, 不宣稱所有 YAML grammar 完全相同

### Paged content 數值文字與 revision 的待辦差異

基底 `bee04b0` 與來源 main 核對未變, 原始 content_page 使用 Python json.dumps(indent=2, ensure_ascii=False, allow_nan=False), Rust 使用 serde_json pretty 與既有 arbitrary_precision, 實測 15 組輸入確認並非只有 exponent 補零差異

例如 1e-7 的 Python 文字為 1e-07, 1e0 為 1.0, 1E0005 為 100000.0, 過長小數由 Python float 捨入但 Rust 保留原數字, 1e400 在 Python page 拒絕非有限值, Rust 可輸出數字, 1e-400 在 Python 為 0.0, 大整數與一般字串案例一致

此差異影響現有 page_tool 所選 request / response / output / text / sql 的非字串 JSON 分頁文字、字元總數與 revision hash, 原始文字頁不需經此 JSON formatter, 修正數值文字必然改變受影響內容的 offsets / hash, 不可沿用舊 revision 或以整串文字取代破壞引號內文字

本輪只稽核現有來源與序列化器, 未修改 runtime、數值型別、global serializer 或依賴設定, exact Python 相容所需的數值轉換與溢位錯誤策略尚待決定, 不宣稱只是格式修正, 未重跑 Rust 全套, Unicode digit escape 與歷史獨立失敗仍保留

### 憑證遮蔽的 Python 控制空白語意

原始 operation_records.redact 與 project_instructions.instructions 使用 Python regex 空白, 包含 U+001C 至 U+001F, Rust regex 原先不包含這四個字元, 使 Bearer 或 assignment 的合成憑證可能未遮蔽

僅補齊 detail / diagnostic / instruction 六個憑證 regex 的空白範圍, diagnostic 與 instruction 的未加引號值仍依 Python 在空白處結束, 後面的普通文字保留, detail 原有整行保守遮蔽不變, 未遮蔽模式保持原文, SQL grammar、數值 rendering、HTTP 設定流程、容量與資料保存均未修改, 不增加來源讀取或 schema

兩個新回歸修正前失敗, 修正後 details 11、instructions 4 項通過, 原始 Python 四種控制空白合成 fixture、instruction_sources 7、error_records 21 項通過, instruction 原始檔案與 read_bytes 均保持, 完整 service 341 項通過 (105.57 秒)

完整 serial HTTP 本輪為 218 項通過、1 項失敗 (160.68 秒), 不列為全套通過, overflow_tool_detail_uses_verified_source_and_masks_secrets 已完成初始 snapshot 與遮蔽 detail, 在 posting metadata setting 後於 read_to_end 發生 WouldBlock, 未取得可判讀的 status / body, 與歷史唯讀 GET settings 503 分開保留, 未重跑直到通過、延長 timeout 或放寬 assertion

該 fixture 不含本輪新增的四種控制空白, settings / refresh / overflow 程式碼未修改, 既有 settings snapshot 等待上限與測試 socket read timeout 都是 5 秒, 僅為待診斷的時間邊界, 未證明根因或排除所有間接影響, focused 遮蔽驗證不取代完整 HTTP 接受, 本輪 patch script 的 regex 錯誤與修正前再次測試失敗記錄亦保留

Unicode digit escape、paged numeric rendering、既有獨立 503 / WouldBlock、cleanup 警告與 overflow 排除範圍仍未解決

### Overflow metadata POST 的有界失敗診斷

在 `83d76a2` 後僅為既有 overflow HTTP fixture 的 metadata POST 增加 elapsed、回應長度、HTTP status line、generic unavailable body 命中與 child liveness 記錄, 不輸出請求內容或完整 settings, 原有 5 秒 socket timeout 與 200 / 202 assertion 不變, 未修改 runtime

單次 focused run 失敗 (0 通過、1 失敗, 17.46 秒), read_to_end 在 5.004049432 秒完成 455 bytes, 實際回應為 HTTP 503 與 Settings temporarily unavailable, child.try_wait 為 Ok(None), 這次取得 server 回應, 與前次無 status / body 的 WouldBlock 及更早唯讀 GET settings 503 分開保存, 不把前次未取得的回應推定為相同 503

來源追蹤確認此 metadata-only 請求走 server.rs 專用分支, 不是 batch settings 的 202 fallback, 專用分支先更新 atomic 設定與 journal、喚醒 collector, 再最多等待 5 秒直到 published snapshot.metadata_enabled 符合, 未符合或 channel 結束即回 503, 本次證據尚未區分 collector 延遲或其他未發布原因

collector 在 projections 階段讀取 metadata atomic, disabled 分支仍維持 overflow workdir 準備與既有 detail index 更新, 最後產生 snapshots 並 publish, 不能只以設定已寫入推定新投影可用, 原始 Python set_settings 在 refresh_lock 下同步 refresh 後回傳設定, 本輪不更動等待、隱私 guard、overflow 上限或資料保存

下一步需區分設定送出時 collector 的階段與設定改變後首次 snapshot 發布時間, 本輪 focused 失敗不構成修復, 未另跑全套或重試直到通過, 先前所有獨立失敗保持未解決

另以既有公開 snapshot collection metrics 進行單次獨立合成來源 phase probe, 不更改 production instrumentation, POST timeout 仍為 5 秒, 並行 GET 只記錄 phase / elapsed / metadata bool, POST 後 0.243 秒見 sessions, 0.955 至 3.911 秒的回應見 projections, phase_ms 為 257 至 3193, POST 於 3.854 秒回 200, 4.035 秒確認 metadata=false、private workdir 不可見, 最後 detail 200 且憑證已遮蔽, child 正常退出

該 probe 未重現 503, 並行 GET 可能先擷取較舊 snapshot 再完成回應, 不可從回應完成時間推定發布順序倒退, 此觀察只定位成功路徑有數秒 projection / publication 延遲, 未證明失敗當次的細部耗時、根因或問題已解決, 原始失敗與本輪 focused 失敗仍保留

### 隔離合成來源的 snapshot 成本分析

以 exact `83d76a2` 的獨立 source copy、獨立 target 與 locked offline toolchain 建置時間戳診斷 binary, 主工作目錄 runtime 未改動, instrumentation patch 與每輪記錄分別保留, 所有來源均為 5001 個合成 session fixture

第一輪取得 POST 503 (5.008 秒), 5.577 秒確認新 metadata=false snapshot, 該 cycle 的 overflow workdirs 6 ms、metadata branch 6 ms、detail_index 353 ms、snapshots 4759 ms、projection span 5126 ms, child 正常退出, 後續細分輪同樣取得 503 (5.054 秒), 5.514 秒確認新 snapshot, snapshots 4193 ms 中四個 windows 約 3380 ms、window 後處理約 620 ms

再細分 windows 的一輪回 200 (4.235 秒), 非 503 重現, 每個 window 約 3.73 MB, metadata=false cycle 的四個 window 完成時間分別 575 / 935 / 571 / 645 ms, 包含 row / Value 建構與序列化, snapshot 總計 3813 ms, window 後至 return 約 721 ms, instrumentation、並行觀察及執行環境變異都可能影響時間, 不把某輪數字推定為歷史失敗根因

一次 instrumentation script 因 typed windows binding 未符合預期而在寫入前失敗, 該 shell 未設定 set -e, 接著意外重跑原診斷 binary 一次, 另存 prestage-repeat 與 script failure 記錄, 不歸入新 stage 證據或接受結果, 後續命令已使用 set -e

來源顯示 logs 組裝為取得 jev.health / reader.checked_at 而解析四個完整 window, 可評估直接重用已生成欄位以避免此成本, 尚未改動主 runtime, 須先核對欄位生成、augmentation 與序列化相容性, 不以縮減 snapshot、調高 timeout 或變更保存上限作為修正

JEV 欄位重用稽核: health 為生成的固定狀態字串, checked_at 為生成時間, source_registry 不改寫 JEV, snapshot serializer 只特別處理 codex, 但既有 snapshot_json::field 仍先對完整 window 做 Checked 深度 / 語法驗證, 遇 reserved serde key 或 Unicode escape 採原 Value fallback, 直接重用 descriptor 會略過其他欄位造成的拒絕邊界

本輪決定不實作 cached descriptor 或改寫 selector parser, 保留完整驗證後再重用只能省去第二次 RawValue traversal, 收益尚未量測且會增加 caller 與序列化結果一致性的要求, 新 single-pass selector 另需 duplicate-last、非 object、數字、深度與 legacy reserved-key 等價證據, HTTP 失敗保持未解決, 不以跳過驗證換取速度

### JSONL 空白行的 Python 控制字元語意

來源 main 與 workspace main 核對未變, 原始 parse_text 會忽略只含 U+001F 或 space / U+001F / tab 的空白行, Rust 原先保留該行而無法辨識 JSONL, 本輪僅將 parse / parse_document 的兩處 JSONL blank-line filter 改用既有 trim_text, 不修整非空行的 JSON 內容, 不更動 CSV、bytes / lines 上限或持久化

新回歸修正前失敗, 修正後 structured 全家族 13、MCP documents 6、Python payload_detail 12 與直接來源 fixture 通過, U+001F 前綴的非空 JSON 行仍拒絕, 完整 service 342 項通過 (130.70 秒)

完整 serial HTTP 單輪為 217 通過、2 失敗 (107.36 秒), overflow fixture 本輪通過但不代表舊問題解決, 新失敗分別為 stdin_control_pauses_resumes_and_stops_the_owned_service 預期 terminal chunk 0 卻讀到 7551, 以及 stream_and_http_publish_matching_window_snapshots_and_session_logs 的 checked_at 欄位不同, desktop 為 .031 / .054、session / JEV 為 .031 / .055, 畫面列出的 entries / counts 相同, 完整記錄保留, 尚未判定根因或改動 assertion

本輪不列為完整 HTTP 接受, 不重試直到通過, 舊 WouldBlock、metadata POST 503、唯讀 GET settings 503、cleanup 警告、Unicode digit 與 numeric rendering 待辦均保留

### SSE 暫停與跨請求 logs 比較的測試同步

在 `6416cd5` 上使用單次合成來源 probe, 初始 SSE version 2 後只追加 newline, error entries 與 totals 完全相同, 後續 GET logs 的 desktop / session / JEV checked_at 由 .171 變成 .201, 整份 logs 不同, 接著在不讀取已排入 SSE 的資料時 pause, paused acknowledgment 後仍收到 version 3 的有效 snapshot chunk (31318 bytes), 再收到 terminal 0 與 EOF, 暫停時 child 仍存活, stdin 結束後正常退出且 stderr 為空

此 controlled probe 證明既有測試假設可被合法交錯打破, 不宣稱重建歷史失敗的精確排程, API 的 logs 只支援 window 選取, 沒有 pinned version, SSE 在同一發布版本內組裝 snapshot / logs, 之後的 HTTP GET 則讀取當時最新快照, pause 釋放連接埠並關閉 stream, 不撤回已送入用戶端緩衝區的有效資料

僅修正測試同步: pause 在原 5 秒期限內驗證最多 64 個 queued SSE chunk, 每個沿用 1 MiB 上限, 接受有效 snapshot / heartbeat 後仍要求 terminal 0 與 EOF, 保留 slow socket 關閉、監聽連接埠釋放、resume、同 PID 與 journal 驗證, logs 比較移入原 matching loop 並要求整份內容完全相同, 最後比較同次擷取的 response, 不刪除 checked_at 或放寬內容等價要求, runtime 未修改

focused pause 1 項通過 (4.63 秒)、matching 1 項通過 (0.23 秒), 完整 serial HTTP 本輪 218 通過、1 失敗 (122.81 秒), 兩項同步測試通過, overflow metadata POST 於 5.003903529 秒收到 455 bytes / HTTP 503 / Settings temporarily unavailable, child 仍存活, 沒有重試直到通過, 未宣稱完整 HTTP 接受

前次相同 runtime 的 service 342 項通過結果保留, 本輪只改測試而未重跑 service, 歷史兩項 SSE 失敗、controlled probe、metadata 503 / WouldBlock / 唯讀 GET 503 與其他既有待辦均保存, 不把成功同步測試視為所有 stream / shutdown 排程的證明

### Source registry 改用借用輸入與分批寫回

原 augment 為讀取少數來源 metadata 而複製完整 codex / mcp / settings / monitor, 包含不使用的 thread / event / history 樹, 本輪改為借用這四份輸入, 依原宣告順序生成小型 sources map, 借用結束後逐項更新既有 sources, 保留未知項目、已存在 key 的位置及缺少 configuration 時既有 mcp_config, 沒有新增來源讀取、請求、schema、parser 或保存 / 容量變更

隔離副本以原函式作比較, 66 組有效 / 缺少 / null / malformed 輸入的完整序列化結果、順序及預期 panic 後的部分修改狀態一致, 原有 4 項測試亦通過, comparator 初次因 nested module 路徑錯誤未編譯, 修正 comparator 路徑後通過, 失敗記錄保留

三次固定先 legacy 後 borrowed 的合成大型樹單函式量測為 64957 / 8070、1134040 / 6851、37219 / 354 微秒, 該 fixture 在 codex / mcp / monitor 各放入 5000 個合成項目, 有明顯排程與快取變異, 不以此推定實際 HTTP 加速比例

正式回歸保留未讀樹、未知來源項目、順序、idempotence、缺少 / null 與 8 組 malformed 輸入拒絕及原值, focused source_registry 5 項通過, 完整 service 343 項通過 (140.43 秒), 完整 serial HTTP 本輪 218 通過、1 失敗 (91.45 秒), 新失敗 idle_stream_heartbeats_do_not_republish_the_source 預期 heartbeat 卻收到有效 snapshot version 2, 原始 frame 與記錄完整保留, 未修改該 assertion 或重跑直到通過

本輪 overflow 與兩項 SSE 同步測試通過, 單次 main binary 合成 profile 的 metadata POST 於 4.895 秒回 200、5.129 秒確認新 snapshot, private workdir 不可見、detail 200 且遮蔽、child 正常退出, 不代表舊 503 / WouldBlock 或新的 heartbeat 差異已解決

heartbeat 測試只等 health=ok / device_ready, 讀取首個 SSE 後寫入 ignored.txt, watcher 會被 session root 內非 access event 喚醒, 但實際發布仍受多個 collector change flag 控制, 本輪沒有記錄失敗當次是哪個 flag, 尚無法區分初始背景更新未穩定或無關檔案喚醒造成的變更, source registry 未改動上述控制流程, 不以此自動宣告與本輪時間變化無關

### Heartbeat 測試的初始發布穩定條件

在 exact `d3fd1b0` runtime 的隔離副本加入僅輸出 publication reason 名稱的診斷, 各跑一次不寫檔 control 與寫 ignored.txt treatment, 兩者首個 SSE 都是 version 2, 下一個 heartbeat 分別在 15.020 / 15.021 秒到達, 程序正常退出, 兩次初始 version 2 原因都是 worktree_changed / requested, 本輪沒有重現失敗, 不推定無關檔案必然造成發布

來源核對: font / GPU worker 先記錄 device completion, 再設定 manual refresh 與喚醒 collector, device_ready 是 snapshot 組裝時讀取 completion 所產生的欄位, 不是 render 即時補入, 因此進行中的 collection 可以先包含已完成的裝置資料, 同時另一個 requested refresh 仍待下一輪發布

另一次 controlled gap 僅在隔離 worker 的 completion 與 refresh 之間加入 1 秒診斷延遲, 不寫 ignored.txt, 經原 health / device_ready 條件後首個 SSE 為 version 1, 0.956 秒後收到 version 2, reason 為 requested / worktree_changed, 隨後另一個 worker refresh 產生 version 3, logs entries 相同但 codex 有變化, 程序正常退出, 此實驗證明該交錯可成立, 不宣稱重建原始失敗排程, 主 runtime 未加入延遲或 instrumentation

僅修改測試 setup: 以明確 18 秒 deadline、最多 64 個經驗證且 version 遞增的初始 snapshot 等到第一個 quiet heartbeat, 以最後一份合法 snapshot 作 baseline, 再寫 ignored.txt, 原本 treatment 後的下一個 heartbeat 與完整 codex equality assertion 不變, treatment 後 read timeout 仍為 18 秒, 不略過 treatment 後的新 snapshot

focused heartbeat 1 項通過 (30.19 秒), 完整 serial HTTP 單輪 219 項通過 (134.57 秒), runtime 未改動而未重跑前次 343 項通過的 service, 所有原始 heartbeat / SSE 失敗、control / treatment / controlled gap 記錄保留, 此次通過不代表所有排程或歷史 metadata 503 / WouldBlock / 唯讀 GET 503 已解決

### 憑證遮蔽的保守 Unicode 邊界補足

來源 LAM main `5c03daf`、workspace main `4616074` 核對未變, Python 3.12.14 使用 Unicode 15.0.0, 既有 regex 1.13.1 / regex-syntax 0.8.11 使用 Unicode 16.0.0, 原始 Python 會遮蔽 U+0301、U+200C、U+200D、U+203F 後的 Bearer、sk- 與 api_key, Rust 的 Unicode word boundary 原先將這些前綴視為 word 而漏遮蔽

完整 Unicode scalar 分類比較顯示原 Rust word 集合比 Python 多 7646、少 915 個字元, 直接改成 Letter / Number / underscore 仍有 5004 個版本差異, 不採全面替換, 既有 superscript 等較保守遮蔽亦不得移除, 本輪僅在原 boundary 之外補上 Rust word 中非 Letter / Number / underscore 的 2642 個字元, 本次分類比較沒有新增 Python word 字元, 不代表所有 Unicode 版本或 regex case folding 完全等價

合成測試顯示初版使用所有非 Letter / Number / underscore 前綴會使泛用 SECRET 提前吃掉 ` .api_key` 的句點, 因此限縮為兩個字元集合的交集, 保留原邊界的標點行為, 此中間版本與修正前兩項失敗記錄保留, 不作最終接受證據

僅調整憑證相關的六個 pattern 與取代字串, 新增匹配會保留前綴, 結構型 secret 仍整段隱藏, SQL 語法 pattern、無遮蔽請求、資料讀取數量、來源檔案、schema、相依套件與保存 / 容量界線不變, 受影響的資料仍來自既有按需 detail / diagnostic / MCP 文件 / instruction 來源, 沒有新增 endpoint、查詢參數或網路請求

新命中的遮蔽文字與其內容 revision 會隨輸出變更, 分頁 offset / hash 演算法及無遮蔽輸出不變, 合成 overlap audit 共 3584 組 pattern 案例, 原先 1988 個匹配範圍全部仍被遮蔽, 此為有界 fixture 證據而非所有輸入的證明

最終限縮版本 focused 2 項通過 (0.10 秒), 完整 service 345 項通過 (129.21 秒), 完整 serial HTTP 單輪 219 項通過 (135.93 秒), 原始 Python 合成 fixture 24 組、instruction 7 項與 payload 12 項通過, instruction 測試確認來源 bytes 未改寫, 中間版本另有 service 345 / HTTP 219 通過記錄, 不混用為最終版本證據

初始合成 probe 的 Unicode escape 位數錯誤與 overlap probe 缺少括號各在執行前失敗, 修正 probe 後取得上述稽核結果, 未影響主 runtime, 記錄保留, 仍有 5004 個 Unicode 版本邊界差異及原先 915 個較保守案例, 不宣稱完整 Unicode 等價, 歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、Unicode digit、數字頁面格式與其他待辦保留

### YAML 整數負零 key 與重複鍵判定

來源 LAM main `5c03daf`、workspace main `4616074` 核對未變, Python 先將 JSON 整數 `-0` 解析為 `0` 再以 str 生成 mapping key, Rust arbitrary_precision 原先保留 `-0` 字串而產生不同 key, 使 `0` / `-0` 重複鍵未被拒絕

僅在 YAML pair 的非浮點數 key 分支將精確 `-0` 正規化為 `0`, 引號中的 `'-0'` 仍為獨立字串 key, `-0.0` / `-0e0` 保持浮點負零 key, 大整數名稱與原始 numeric scalar 不變, 不改寫數字序列化、分頁 offset / hash 演算法、相依套件或容量界線, 非字串頁面的完整數字呈現差異仍待設計

新增直接 parser / parse_document / 自動辨識回歸, 涵蓋頂層與 sequence mapping 的重複鍵拒絕、quoted / floating / large integer 保留及 scalar 值不變, 原始 Python 8 組 fixture 通過, 新 Rust 回歸修正前失敗記錄保留, 修正後 structured 全家族 14 項通過 (1.01 秒), 完整 service 346 項通過 (133.49 秒), 完整 serial HTTP 單輪 219 項通過 (134.67 秒), 歷史 503 / WouldBlock 與 Unicode 差異仍未宣告解決

### Plugin JSON 的前置 UTF-8 BOM

原始 Python `_json_metadata` 以 json.loads 解析 bytes, 接受一個位於最前方的 UTF-8 BOM, Rust 原先直接交給 serde_json 而回報 unavailable, 本輪僅在既有 byte-budget 檢查後移除一個精確 BOM 再解析, 適用 `.codex-plugin/plugin.json`、fallback `plugin.json` 與明確設定的 `.mcp.json`, 不更動 config.toml、全域 JSON parser、來源路徑或 metadata 白名單

BOM 本身仍計入 bytes_read、單一 manifest 65536 bytes 與總計 2 MiB 上限, BOM-only、double BOM、非前置 BOM、無效 UTF-8、非 object 與 malformed JSON 仍拒絕, JSON 字串內的 U+FEFF 保留, 證據包含 source bytes 未改寫、兩種 manifest 路徑及 MCP runtime arguments 不外露

Python 14 組 bytes / budget fixture 通過, 其中 UTF-16 / UTF-32 為 Python 可讀、Rust 本輪仍未支援的獨立編碼差異, 不宣稱完整 bytes decoding 等價, 新 Rust 3 項回歸修正前均失敗, 修正後 Plugin 全家族 11 項通過 (0.02 秒), Python 既有 Plugin 10 項通過, 完整 service 349 項通過 (120.31 秒), 完整 serial HTTP 單輪 219 項通過 (128.00 秒), 歷史 503、數字呈現與 Unicode 限制繼續保留

### Session index 的逐行 UTF-8 BOM

原始 Python 以 binary stream 逐行呼叫 json.loads, 因此每行可各自帶有單一前置 UTF-8 BOM, 本輪僅於既有 1 MiB tail 擷取與首行丟棄之後, 在 merge_session_index 的每行 JSON 解析前移除該精確前綴

保留 observed ID 篩選、最新有效 title、catalog title 優先、512 字元上限與來源檔案, BOM-only、double / 非前置 BOM、malformed、無效 UTF-8 與非 object 行仍忽略, title 內 U+FEFF 不移除, tail 起點恰落在完整行開頭時仍依原行為丟棄首行, 未因 BOM 放寬讀取範圍

不改動共用 global-state decoder 或其他 JSON 來源, Python 對 global-state 的專案與 Dots 呼叫分別使用文字與 bytes, 不以單一 decoder 變更推定兩者等價, UTF-16/32 仍是獨立未支援差異

原始 Python 2 組完整來源 fixture 通過且 bytes 未改寫, 新 Rust 2 項回歸修正前失敗記錄保留, 修正後 schedules 全家族 15 項通過 (0.06 秒), Python 既有 subagent sources 2 項通過, 完整 service 351 項通過 (126.44 秒), 完整 serial HTTP 單輪 219 項通過 (121.07 秒), 歷史 503、數字呈現與 Unicode 限制仍保留

### 已觀察檔案的 Unicode 保留副檔名

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 核對未變, Python `observed_file_metadata` 使用 Unicode lowercase 判定保留檔名與副檔名, Rust 原先使用 ASCII lowercase 而讓合成 `fixture.KEY` 回傳檔案 metadata

僅將既有兩個 lowercase 呼叫改為 Unicode 版本, 保留 `.env`、`auth.json`、`credentials.json`、`.pem` 與 `.key` 清單、已觀察事件篩選、絕對路徑與 symlink 防護, 不讀取檔案本文、不新增 endpoint 或查詢參數、不改變請求數量與資料保存

原始 Python 18 組絕對 / workdir 相對路徑 fixture 通過且 9 個來源檔案 bytes 未改寫, 新 Rust 單元回歸先重現差異再通過, 涵蓋大小寫保留檔、Kelvin sign、希臘字母、dotted I 與 `.KEY.txt` 非保留副檔名, HTTP detail 與 summary 回歸確認保留檔回傳 rejected 且 bytes / modified_at 為 null, 未觀察路徑與停用觀察的既有拒絕檢查仍保留

Python Unicode 15.0.0 與 Rust Unicode 18.0.0 的非 ASCII scalar lowercase 稽核, 在輸出完全為 ASCII 的範圍皆僅有 U+212A → k, 此證據限定目前 ASCII 保留清單, 不宣稱所有 Unicode lowercase 行為相同

Linux / Rust 1.100.0 的完整 service 352 項通過 (67.77 秒), focused HTTP 1 項通過 (0.31 秒), 完整 serial HTTP 單輪 219 項通過 (121.54 秒), 歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、數字呈現與其他 Unicode / 平台驗收限制仍保留

### 手動 Dots 查詢的前置 UTF-8 BOM

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 核對未變, Python `read_dots` 以 bytes 解析桌面快取而接受單一前置 UTF-8 BOM, `read_metadata` 則以 UTF-8 文字解析同檔而拒絕 BOM, 不能一併放寬共用讀取器

Rust 僅在 `/api/codex/dots` 的按需查詢選用 BOM 解碼, 原有 16 MiB metadata / 實讀 bytes 檢查之後才移除精確前綴, 其他 project、schedule 與背景 Dots 的 text-mode 呼叫維持原判定, 不增加讀取次數、查詢參數、來源範圍或保存內容, 回應仍只取既有 thread / host / timestamp / artifact type 白名單, 不外露 artifact 本文與帳戶欄位

新增兩項回歸修正前均失敗, 修正後 Dots 全家族 9 項通過 (0.27 秒), HTTP 既有查詢測試新增 BOM 內容與原投影一致、來源 bytes 不變、時間窗篩選及 metadata 停用仍生效的檢查, 該測試通過 (0.06 秒)

Python 9 組 BOM / decode / byte-limit fixture 與既有 desktop activity 8 項通過, 涵蓋 BOM-only、double / 非前置 BOM、非 object、malformed 與無效 UTF-8 拒絕, 16 MiB 精確邊界接受、加一 byte 拒絕, 文字 metadata 路徑仍回報 unsupported

Linux / Rust 1.100.0 的完整 service 354 項通過 (66.54 秒), 完整 serial HTTP 單輪 219 項通過 (121.86 秒)

UTF-16 / UTF-32 bytes decoding 仍未支援, 歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、數字呈現及其他 Unicode / 平台限制保留

### 結構化憑證 key 的 Unicode 小寫判定

依 LAM main `5c03daf` 的 `operation_records.redact` 與 `payload_detail.complete_payload`, Python 先做 Unicode lowercase 再保留 ASCII a-z, Rust 原先先濾掉非 ASCII bytes 而漏遮蔽 `apİkey`、`cooKie` 等 key

本輪保留原 ASCII 判定, 僅於未命中且含非 ASCII key 時加上 Unicode lowercase 判定, 避免直接替換使 `apiKkey` 等原有較保守遮蔽消失, 敏感 suffix 清單、無遮蔽輸出、來源 bytes、schema、讀取與保存界線不變, 沒有新增 endpoint、查詢參數或網路請求

新增兩項單元回歸涵蓋字典、巢狀內容、完整 / 有界 payload、diagnostic 與表格欄位, Unicode 漏遮蔽測試先失敗再通過, 380 組原有保守 key 仍通過, 四組不應新增匹配的字元控制保留, HTTP Context fixture 驗證新 key 遮蔽、一般欄位保留與來源 bytes 未改寫, focused HTTP 1 項通過 (0.13 秒)

Python 25 組遮蔽 / 無遮蔽路徑、4 組控制與既有 payload 12 項通過, Python Unicode 15.0.0 與 Rust Unicode 18.0.0 的非 ASCII scalar lowercase 後擷取 ASCII 字母稽核, 兩者皆僅 U+0130 → i 與 U+212A → k, 不宣稱其他 Unicode 行為完全等價

先前完整 service 記錄在中斷後缺少結束結果, 保留為未完成而不計入通過, 恢復後 Linux / Rust 1.100.0 的新一輪完整 service 356 項通過 (89.44 秒), 完整 serial HTTP 單輪 219 項通過 (134.67 秒)

原始純文字 regex 的 Unicode IGNORECASE dotted / dotless I 差異不在本輪字典 key 修正內, 歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、數字呈現、UTF-16/32 及其他 Unicode / 平台限制仍保留


### 憑證名稱的 dotted / dotless I 大小寫匹配

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 核對未變, Python `operation_records.redact`、instruction 與 SQL 憑證名稱的 IGNORECASE 可將 i / I / İ / ı 視為相同, Rust 原有 regex 未匹配後兩者, 合成 `apİ_key=SYNTHETIC_VALUE` 回歸先重現漏遮蔽

僅在既有 text、diagnostic、structured secret 與 SQL secret-column 四個 pattern 中, 將憑證名稱的 literal i 擴為 `[iİı]`, 保留原大小寫與邊界規則, 不更動 SQL 語法、Bearer / sk- token 字元集合、dictionary-key normalization 或一般文字內容, 此為憑證名稱的局部補齊, 不宣稱所有 regex case folding 等價

新增兩項 focused 回歸通過 (0.11 秒), 涵蓋 dotted / dotless I 的一般與結構型值、SQL 欄位、diagnostic 分隔符號保留及無遮蔽輸出, 四個新舊 pattern 的 1,120 組合成比較確認所有既有匹配範圍仍被新 pattern 包含, HTTP Context fixture 新增 dotless I key 並維持來源 bytes 不變, focused HTTP 1 項通過 (0.29 秒)

原始 Python diagnostic / SQL 的 20 組合成案例通過, 既有 SQLite suite 11 項中 9 項通過、2 項因恢復環境缺少前端建置資產而出錯, 錯誤發生於 Dashboard refresh 的 asset 載入, 未修改測試或以單元結果代替這兩項端點驗收

依固定 WBUI `e9741c6` 執行既有 `build_frontend.py --verify-offline` 嘗試恢復前置資產, 但 shell 的既有 Git 認證 / 網路無法取得 Workbench UI, 保留建置失敗與原始兩項測試錯誤, 未變更 pin 或建立替代資產

之後透過既有已認證終端成功取得相同固定版本, 本輪重新執行 offline 驗證通過, 已生成資產與 WBUI pin 一致, SQLite suite 11 項全部通過 (0.240 秒), 前置資產恢復後才重跑, 原始建置與測試失敗仍保留

Linux / Rust 1.100.0 的完整 service 358 項通過 (81.60 秒), 完整 serial HTTP 單輪 219 項通過 (119.75 秒)

沒有新增 endpoint、查詢參數、網路請求、資料來源或保存內容, 遮蔽文字與內容 revision 可隨新命中變更, 分頁演算法、容量界線與來源 bytes 不變, token alphabet 的 dotted / dotless I、歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、數字呈現、UTF-16/32 及其他 Unicode / 平台限制保留


### Bearer / sk- token 的 dotted / dotless I 字元集合

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 再次核對未變, 依 `operation_records.redact` 與 instruction 原始 pattern, Python IGNORECASE 下的 `[A-Za-z]` 包含 İ / ı, Rust 原本的 token pattern 漏遮蔽含這兩個字元的 token, 新回歸先重現 `Bearer İSYNTHETIC_TOKEN` 完全未遮蔽

僅在共用 TOKEN 的 Bearer 與 sk- 兩個字元集合中加上 İ / ı, 保留原集合、大小寫規則、邊界與 prefix capture, 不放寬其他字元、Bearer padding 或 sk- 至少 12 字元的判定, credential-name、dictionary-key 與 SQL 規則不變

新增兩項 focused 回歸通過 (0.05 秒), 涵蓋 token 開頭 / 中間 / 結尾、sk- 11 / 12 字元界線、padding 後一般文字及無遮蔽輸出, 168 組新舊 pattern 比較保留所有既有匹配範圍, HTTP Context fixture 驗證完整遮蔽與來源 bytes 不變, focused HTTP 1 項通過 (0.35 秒)

原始 Python 18 組 token fixture 與既有 instruction 7 項通過, 新增匹配限於本輪指定字元, 不宣稱所有 Unicode 版本、word boundary 或大小寫行為等價

Linux / Rust 1.100.0 的完整 service 360 項通過 (78.28 秒), 完整 serial HTTP 單輪 219 項通過 (120.44 秒)

沒有新增 endpoint、查詢參數、請求數量或資料保存, 內容 revision 可因完整遮蔽而改變, 分頁演算法與容量界線不變, 歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、數字呈現、UTF-16/32 及其他 Unicode / 平台限制保留


### Plugin JSON 的 UTF-16 BOM 解碼

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 核對未變, Python Plugin 的 `_json_metadata` 使用 `json.loads(bytes)` 接受 UTF-16 LE / BE BOM, Rust 原先僅處理 UTF-8, 新 byte-budget 回歸先重現有效 UTF-16 object 被判為 unavailable

僅在既有 Plugin manifest / `.mcp.json` 讀取器中, 於原始讀取與 byte-budget 檢查之後辨識 UTF-16 BOM, 成對讀取 code unit 並以嚴格 UTF-16 decoder 轉為文字後解析 object, 不改變 65,536-byte manifest、2 MiB 跨來源預算、讀取次數、路徑 / symlink 防護或 metadata 白名單, 中間解碼配置仍受原始 manifest 大小約束, 不將 decoded UTF-8 長度取代原始 byte 計數

新增兩項回歸涵蓋 LE / BE、中文與 surrogate pair、字串內 BOM、BOM-only / double BOM / malformed / 非 object / 奇數 bytes 拒絕、無 BOM / UTF-32 / unpaired surrogate 保持拒絕, 65,536 bytes 接受與超限截斷、剩餘預算恰等於實讀 bytes 時仍 truncated, Plugin 全家族 13 項通過 (0.03 秒)

HTTP fixture 保留最初 UTF-8 manifest, 更新為 UTF-16 BE BOM 後仍由 watcher 觀察到新 MCP 白名單, 來源 bytes 未改寫, 原有 metadata 開關及後續更新檢查仍通過, focused HTTP 1 項通過 (0.24 秒), 原始 Python 20 組解碼 / byte-budget fixture 與既有 Plugin 10 項通過

Python 另接受無 BOM UTF-16 與 unpaired surrogate, Rust 本輪不支援且不放寬 surrogate 拒絕, UTF-32 與其他 JSON 呼叫的 UTF-16 解碼也未處理, 不宣稱完整 bytes decoder 等價

Linux / Rust 1.100.0 的完整 service 362 項通過 (77.54 秒), 完整 serial HTTP 單輪 219 項通過 (117.73 秒)

沒有新增 endpoint、查詢參數、來源範圍或保存內容, 不外露 MCP runtime arguments, 歷史 metadata POST 503 / WouldBlock、唯讀 GET 503、數字呈現及其他 Unicode / 平台限制仍保留


### Plugin JSON 的無 BOM UTF-16 判定

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 再次核對未變, 依 `_json_metadata` 與本輪 Python 3.12.14 `json.detect_encoding` 原始實作, Plugin JSON 可由最初 4 bytes 的零位元組位置或精確 2-byte 短輸入判定 UTF-16 LE / BE, 不要求 BOM, 新回歸先重現無 BOM `{}` 被拒絕

本輪只補上 Python 的 UTF-16 前綴判定分支, 延用前輪嚴格 decoder, 明確 UTF-8 / UTF-16 BOM 優先順序與原始 byte-budget 檢查不變, UTF-32 前綴不作 UTF-16 接受, 不變更其他 JSON 呼叫

新增回歸涵蓋 LE / BE、前置 JSON 空白、中文 / surrogate pair、2-byte 不完整 JSON、malformed / 非 object / 奇數 bytes 拒絕、64 KiB 精確界線與跨來源剩餘預算, 無 BOM UTF-32 仍拒絕, Plugin 全家族 14 項通過 (0.03 秒), 先前的無 BOM 拒絕預期由此明確的 source-backed 接受範圍取代

HTTP fixture 延續 UTF-8 與有 BOM UTF-16 BE 的更新後, 以無 BOM UTF-16 LE 檔案進行替換, watcher 正確更新第三個 MCP server 且來源 bytes 不變, 原有 metadata 停用 / 重啟檢查保留, focused HTTP 1 項通過 (0.27 秒), Python 原始 reader 的 22 組解碼 / byte-budget fixture 通過

Linux / Rust 1.100.0 的完整 service 363 項通過 (66.51 秒), 完整 serial HTTP 單輪 219 項通過 (115.87 秒)

UTF-32 與 unpaired surrogate 仍未支援, Python 可接受的部分 surrogate 字串仍由 Rust 拒絕, Plugin 以外 JSON 來源的 UTF-16 支援未擴張, 沒有新增 endpoint、查詢參數、讀取次數、資料範圍、保存內容或容量, 歷史 HTTP、數字呈現及其他 Unicode / 平台限制繼續保留


### Plugin JSON 的 UTF-32 解碼

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 再次核對未變, 依 `_json_metadata` 與 Python 3.12.14 `json.detect_encoding` / `json.loads(bytes)`, UTF-32 LE / BE 可帶 BOM 或由前 4 bytes 判定, 新 byte-budget 回歸先重現有效 UTF-32 object 被拒絕

僅在 Plugin 私有 reader 中, 於原始 byte 檢查後先辨識 UTF-32, 再沿用既有 UTF-16 / UTF-8 路徑, 避免 UTF-32 LE BOM 被當成 UTF-16, 每 4 bytes 嚴格轉成有效 Unicode scalar 才解析 object, 不足 4 bytes、surrogate 或超出 U+10FFFF 的值仍拒絕

核對 production caller 僅為兩種 Plugin manifest 路徑與 `.mcp.json`, 其他 JSON 來源沒有改動, 64 KiB manifest 與 2 MiB 跨來源預算仍計算原始 bytes, 白名單、symlink 防護、讀取次數與來源 bytes 不變, 不保存或外露 MCP runtime arguments

新增兩項回歸涵蓋 LE / BE、有 / 無 BOM、中文與 supplementary scalar、字串內 BOM、double BOM / malformed / 非 object / 不完整 scalar 拒絕、精確 byte 上限與剩餘預算, 兩種 manifest 路徑及 MCP 白名單皆驗證, Plugin 全家族 16 項通過 (0.03 秒)

HTTP fixture 延續原 UTF-8 / UTF-16 更新, 再寫入 UTF-32 LE BOM manifest, watcher 正確發布第四個 MCP server 且來源 bytes 不變, metadata 開關檢查保留, focused HTTP 1 項通過 (0.37 秒), Python 原始 reader 44 組解碼 / byte-budget fixture 通過

Linux / Rust 1.100.0 的完整 service 365 項通過 (65.90 秒), 完整 serial HTTP 單輪 219 項通過 (125.53 秒)

先前 Plugin UTF-32 不支援範圍由本輪明確接受條件取代, Python 另有 4 組有 / 無 BOM 的 unpaired surrogate fixture 可讀, Rust 保持拒絕, 不宣稱完整 JSON 或 Unicode 等價, 其他 JSON 呼叫的 UTF-16/32、歷史 HTTP 503 / WouldBlock、數字呈現與平台限制仍保留


### 手動 Dots 查詢的 UTF-16 / UTF-32 bytes 解碼

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 再次核對未變, 依 `read_dots`, 手動查詢使用 `json.loads(bytes)`, 共享 metadata 與背景讀取則為 UTF-8 文字, 兩種來源模式不得一起放寬, 本輪先以原始 Python 核對 8 種 LE / BE、有 / 無 BOM 的 UTF-16 / UTF-32 fixture, 直接 Dots 成功而文字 metadata 保持 unsupported

將前輪 Plugin decoder 移到私有 `json_bytes::parse`, 除 Vec 改為 slice 存取及說明外解析邏輯相同, helper 不讀檔也不決定讀取上限或 object 白名單, 僅由既有 Plugin reader 與 Dots bytes-mode 呼叫, Plugin 保留原門檻及 object 判定, Dots 仍先完成 16 MiB metadata / 實讀 bytes 檢查再解碼

新增兩項 Dots 回歸修正前失敗, 修正後 Dots 全家族 11 項通過 (3.25 秒)、Plugin 全家族 16 項通過 (0.03 秒), 涵蓋 UTF-16/32 各端序與 BOM 組合、白名單、來源 bytes、不完整 / 非 object / malformed / surrogate 拒絕, 兩種代表編碼的 16 MiB 精確邊界接受、加一 byte 拒絕

文字模式的 `read_state_with_health` 仍回報 unsupported, `read_state` 仍為 None, 背景 Dots 不新增 activity, 不改動 project / schedule 的共同讀取方式, 16 MiB 是原始輸入限制, 不代表解碼與 JSON 配置的整體記憶體低於 16 MiB

HTTP 既有查詢 fixture 新增無 BOM UTF-16 LE 與有 BOM UTF-32 BE, events / activity 與原 UTF-8 結果相同、來源 bytes 未改寫且私密 artifact / account 欄位不外露, 原時間窗與 metadata 開關檢查保留, focused HTTP 1 項通過 (0.07 秒), Python 12 組 direct / text-mode / byte-limit fixture 與既有 desktop activity 8 項通過

Linux / Rust 1.100.0 的完整 service 367 項通過 (60.08 秒), 完整 serial HTTP 單輪 219 項通過 (126.35 秒)

不新增 endpoint、查詢參數、讀取次數或保存內容, unpaired surrogate 仍不支援, 其他 JSON bytes 呼叫未自動採用新 helper, 歷史 HTTP 503 / WouldBlock、數字呈現與 Unicode / 平台限制仍保留


### Session index 的逐行 bytes 解碼與 LF 邊界

來源 LAM main `5c03daf`、workbench-workspace main `4616074` 再次核對未變, 依 `read_metadata`, session index 以 binary stream 逐行交給 `json.loads(bytes)`, 不是先將整份檔案轉成文字, 本輪將 Rust 分行改為保留原始 LF byte 的 `split_inclusive`, 並僅在每個原始資料列使用既有私有 JSON bytes decoder

保留 1 MiB tail、offset 大於零時無條件丟棄第一行、observed ID 篩選、最後有效 title、catalog 優先與 512 字元限制, 不改變讀取次數或持久化欄位, byte-LF 是原始分隔符而非解碼後的換行

UTF-16 / UTF-32 BE 行的換行 bytes 可完整落在同一資料列, LE 行若帶編碼後 LF 則會被 raw 0x0A 切斷並依 Python 行為拒絕, LE 無尾端換行的完整資料列仍可接受, 字元本身含 raw 0x0A 也會分行而拒絕, 相同字元以 JSON escape 表示則可接受, 未因編碼支援改用整檔轉碼或修補破碎資料列

兩項 framing / tail 回歸修正前失敗, 新增三項回歸後 schedules 全家族 18 項通過 (0.10 秒), 涵蓋 LE / BE、有 / 無 BOM、raw-LF / escaped-codepoint、剛好 1 MiB 與多出前綴時的首行丟棄、混合編碼、title 上限、未知欄位排除及來源 bytes 未改寫, Python 原始 reader 的 29 組 framing / tail / selection fixture 通過

HTTP fixture 保留初始 UTF-8, 更新為 UTF-16 BE 與 UTF-32 BE BOM 後 watcher 依序更新名稱, 空白 title 不覆蓋有效值且來源 bytes 不變, focused HTTP 1 項通過 (0.17 秒), 初版 HTTP fixture 因插入錯誤測試區段造成 `utf16` 未定義的編譯錯誤, 已校正至指定測試並還原無關區段, 原失敗記錄保留, 該 HTTP 中間版本未執行

Linux / Rust 1.100.0 的完整 service 370 項通過 (62.11 秒), 完整 serial HTTP 單輪為 218 項通過、1 項失敗 (115.29 秒), 未重跑至綠燈

新觀察到的失敗為未修改的 `idle_snapshot_and_new_stream_report_current_uptime`, 等待 1.2 秒後 refreshes 為 3 而預期 2, 並非 uptime 比較失敗, 該測試停用 Codex, 原始碼的停用路徑不執行本輪 session-index reader

原始碼另可見 device completion 先標記 ready 再排入 refresh wake, 因此 ready 不必然表示所有啟動 wake 已處理完畢, 這是可疑競態而非本次事件的已確認原因, 尚缺觸發來源追蹤, 本輪不修改 idle assertion、計數或排程, 此項 HTTP 失敗保留待處理

其他 JSONL 呼叫不自動採用此 decoder, unpaired surrogate、歷史 HTTP 503 / WouldBlock、數字呈現與其他 Unicode / 平台限制保留, 不新增 endpoint、查詢參數、容量或保存內容


### 閒置 uptime 測試的啟動發布基準

保留 `81a8f62` 的完整 HTTP 218 / 1 失敗記錄, 當時的 refreshes 3 / 2 差異尚未追蹤到確切 wake, 本輪僅修正測試起始條件, 不宣稱已證明歷史失敗的觸發來源, LAM main `5c03daf`、workbench-workspace main `4616074` 核對未變

原始碼在 fonts / GPU completion 標記 ready 之後才排入 refresh wake, 因此 device_ready 不保證啟動發布已結束, 既有 heartbeat 測試已使用安靜區間處理同一前置條件, 本輪抽出原有測試 helper 共用, 保留最多 64 frames、18 秒 deadline、frame 類型 / version 驗證及單調 version 檢查

服務的 heartbeat 來自 15 秒未收到 snapshot 變更, uptime 測試先等待該安靜區間, 再取新的 HTTP snapshot 作為時間基準, 額外核對 refresh count 與已穩定的 SSE snapshot 一致, 原 1.2 秒後 uptime 增加、refreshes 精確相等及新串流 uptime 不落後的 assertions 完整保留, 既有 ignored-file / heartbeat assertions 亦未放寬

確定性的合成 SSE fixture 送出 ready / refreshes 2、延後的 refreshes 3 與 heartbeat, 僅取首個 ready frame 的版本先失敗, 使用安靜區間 helper 後取得 3, 此 fixture 驗證 gate 的行為, 不是對歷史失敗的觸發追蹤

四項 focused HTTP 測試通過 (46.80 秒), 比對確認原 gate 除 receiver / 縮排 / 說明外相同, 原 uptime 與 heartbeat 的結果 assertions 逐字保留, 僅修改 HTTP 測試與驗證文件, runtime、device_ready 定義、refresh 計數與排程未變, service 370 項沿用前輪證據而未重跑

修改測試起始條件後的完整 serial HTTP 單輪 220 項通過 (130.51 秒)

不以重跑未修改測試取得綠燈, 原失敗與合成基準失敗記錄均保留, 歷史 HTTP 503 / WouldBlock、其他 race、數字呈現、surrogate 與平台驗收限制仍保留


### Monitor journal 的原始換行與空白列計數

LAM main `5c03daf`、workbench-workspace main `4616074` 再次核對未變, 依 `MonitorState.load_journal`, 每個檔案先讀取最後 64 KiB 原始 bytes, 使用 `bytes.splitlines()` 分行, 超過上限時無條件丟棄第一列, 不是整檔文字解碼

Rust 原本僅以 LF 分行並略過空白列, 本輪改為私有 iterator 處理 CR、LF、CRLF, CRLF 視為單一分隔符, 空白實體列計入 unsupported, 空檔與尾端分隔符不產生額外資料列, Python 3.12 原始 reader 的 12 組 fixture 與 256 種單一 byte 分類通過

兩項 reload 回歸修正前失敗, 修正後 journal 全家族 8 項通過 (0.01 秒), 涵蓋 CR / LF 混合、連續空白列、前一份與目前檔案順序、剛好 64 KiB 與多出前綴、首列為空白時的丟棄、CRLF 跨 tail 起點、來源 bytes 不變與未知欄位排除

新增 `/api/logs?window=all` fixture 1 項通過 (0.03 秒), 兩筆 bare-CR / CRLF 分隔記錄皆保留且 unsupported_lines 精確為 1, 私密欄位不外露, 啟動正常附加事件但未改寫既有前綴

不改變原始讀取上限、保留筆數、events 上限、輪替或寫入格式, JSON 解碼仍為既有 serde reader, journal 的 BOM / UTF-16 / UTF-32 與 surrogate 差異並未在本輪補齊, 不新增 endpoint、查詢參數或讀取次數

保留前輪 idle refresh 失敗與後續測試基準修正的各自證據, 不宣稱歷史原因已確認, HTTP 503 / WouldBlock、數字呈現、其他 Unicode 與平台驗收限制仍保留

Linux / Rust 1.100.0 的完整 service 單輪 373 項通過 (63.73 秒), 完整 serial HTTP 單輪 221 項通過 (132.51 秒)

### Monitor journal 的逐列 JSON 編碼

依 LAM main `5c03daf` 的 `MonitorState.load_journal`, 每個原始資料列交給 `json.loads(bytes)`, 不是先將整份日誌轉碼再分行

Rust journal 改用既有私有 JSON bytes decoder, 支援 UTF-8 BOM 及有 / 無 BOM 的 UTF-16、UTF-32 LE / BE, 解碼仍在 64 KiB 原始讀取上限、CR / LF / CRLF 分行及超限首列丟棄之後, 不更改輪替、寫入、保留筆數或白名單投影

Python 原始 reader 的 50 組編碼 / 分隔符 fixture 通過, 另核對 2 組內嵌 CR / LF byte 與 1 組超限尾段, 編碼後的換行仍依原始 byte 切開: LE 留下無效零值片段, BE 使前列長度無效, 不擴大為整檔轉碼

新回歸先重現 UTF-8 BOM 資料列遺漏, 修正後 journal 10 項通過, 涵蓋編碼、空白列、tail 上限、來源 bytes 不變及未知欄位排除

另核對 Python 接受未成對 surrogate 的差異, Rust 仍維持既有嚴格 scalar 驗證, 本輪未放寬, 歷史 HTTP 503 / WouldBlock、idle refresh 原因未明、數字呈現與平台驗收限制保留

新增 `/api/logs?window=all` 的 UTF-32 LE BOM fixture, 驗證投影與 unsupported_lines, 沒有新增 endpoint、參數、來源讀取次數或寫入格式

Linux / Rust 1.100.0 的完整 service 單輪 375 項通過 (75.20 秒), 完整 serial HTTP 單輪 222 項通過 (128.63 秒), 使用既有固定 WBUI `e9741c6` 資產且離線驗證通過

### PerformanceDebug 的實體資料列保留

依 LAM main `5c03daf` 的 `PerformanceDebug.load`, 每份檔案先驗證最多 8,192 bytes 的 LF 首列, 再讀最後 256 KiB、依 CR / LF / CRLF 分行、超限時丟棄第一列, 最後只解析尾端 150 個實體資料列

Rust 原先在保留與超限首列判斷前排除空白列, 且只依 LF 分行, 本輪共用現有 journal byte iterator, 空白列不會產生記錄但仍占原始 150 列範圍, 不因空白列而重新帶入更舊記錄

原始 Python 的 7 組 fixture 通過, Rust 新回歸先重現 CR 分隔的兩筆記錄遺漏, 修正後 PerformanceDebug 5 項通過, 覆蓋 150 列邊界、混合換行、空白 tail 首列、來源 bytes 不變及私密欄位排除

新增 `/api/snapshot` 回歸核對 `monitor.debug.records` 的倒序投影, debug 未啟用時仍能唯讀載入歷史且不改寫檔案, 不新增 endpoint、參數或讀取次數

首列驗證仍只以 LF 結束, 兩筆 bare-CR 相接不能變成有效 header, JSON BOM / UTF-16 / UTF-32 與 surrogate 的既有差異未在本輪擴充, 輪替、寫入與其他驗收限制保持不變

Linux / Rust 1.100.0 的完整 service 單輪 377 項通過 (64.36 秒), 完整 serial HTTP 單輪 223 項通過 (127.96 秒)

### PerformanceDebug 的首列與資料列編碼

依同一 LAM main `5c03daf` 的 `PerformanceDebug.load`, header 與資料列都使用 `json.loads(bytes)`, 本輪將兩個 Rust parser 呼叫改用既有私有 JSON bytes decoder, decoder 本身不變

支援 UTF-8 BOM 與有 / 無 BOM 的 UTF-16、UTF-32 LE / BE, 仍先套用 8,192 bytes header readline、256 KiB tail、首列丟棄及 150 個實體資料列範圍, 不先轉碼整檔

Python 原始來源的 40 組 header、50 組資料列、2 組原始 byte 上限與 1 組輪替 / 目前檔案 fixture 通過, header 與 body 的分行差異明確保留: 編碼 BE 換行可通過 header 但該 body 列無效, 編碼 LE 換行在 header 即失敗

兩項新增回歸修正前失敗, 修正後 PerformanceDebug 7 項通過, 新 `/api/snapshot` fixture 核對 `monitor.debug.records` 的 UTF-32 BE 前一份檔案與 UTF-32 LE 目前資料列, 維持倒序、白名單及來源 bytes 不變

header 的 JSON 與 Unicode 解碼錯誤仍沿用 Rust 的 ValueError 分類, 未對齊 Python 的細部分類, 未成對 surrogate 仍嚴格拒絕, 不改變輪替、寫入、endpoint、參數或讀取次數, 歷史失敗與其他驗收限制保留

Linux / Rust 1.100.0 的完整 service 單輪 379 項通過 (60.98 秒), 完整 serial HTTP 單輪 224 項通過 (126.25 秒)

### Codex Cloud CLI 的 stdout 編碼

依 LAM main `5c03daf` 的 `CodexCloudSource._read`, CLI stdout 為 bytes, 成功結束且未超過 1 MiB 才交給 `json.loads`, 本輪 Rust 對應呼叫改用既有私有 JSON bytes decoder

以隔離的合成 CLI 驗證 UTF-8 BOM 與有 / 無 BOM 的 UTF-16、UTF-32 LE / BE, Python 原始 subprocess reader 的 10 組編碼、剛好 1 MiB 與超出 2 bytes fixture 通過, 未呼叫實際 Codex 或雲端帳戶

新增回歸先重現編碼輸出無法解析, 修正後 cloud 10 項通過, 同時驗證原始 byte 上限、失敗退出碼與截斷編碼仍拒絕, 保留 cursor、選定資料目錄及私密欄位排除

新增 `/api/codex/cloud` 的 UTF-32 BE stdout fixture, 核對實際 endpoint 投影及同頁快取不重啟子程序, 不改變 60 秒快取、最多 10 頁、20 筆投影上限、逾時或程序清理

此結果僅涵蓋合成 CLI 與本機 HTTP, 不代表實際帳戶或跨平台認證驗收, 未成對 surrogate 與既有歷史失敗、其他驗收限制保留, 不新增 endpoint、參數或請求次數

Linux / Rust 1.100.0 的完整 service 單輪 380 項通過 (62.28 秒), 完整 serial HTTP 單輪 225 項通過 (137.47 秒)

### 固定公開來源的 HTTP JSON 編碼

依 LAM main `5c03daf` 的 `PublicApiSource._read`, OpenAI 狀態與 Codex 發布資訊皆先限制原始 response bytes 再交給 `json.loads`, 本輪 Rust 共用 reader 改用既有私有 JSON bytes decoder

支援 UTF-8 BOM 與有 / 無 BOM 的 UTF-16、UTF-32 LE / BE, OpenAI 的 64 KiB 與 Codex 的 1 MiB 原始上限維持不變, 不依解碼後長度放寬輸入

Python 原始 reader 的 loopback HTTP 20 組編碼與 4 組上限 fixture 通過, Rust 新回歸先重現 BOM 輸出解析失敗, 修正後 public_api 6 項通過, 涵蓋兩種投影、剛好上限、超出 2 bytes、截斷編碼、redirect 與非成功狀態拒絕

測試 server 改以原始 bytes 回覆, 原有 redirect / 超限斷言保留, 執行期固定 URL、60 秒快取、查詢條件、白名單與請求次數不變

此輪只驗證 loopback HTTP reader 與既有快取測試, 未連線實際固定來源, 不代表遠端服務可用性或完整外部 endpoint 驗收, surrogate 與既有歷史失敗、其他限制保留

Linux / Rust 1.100.0 的完整 service 單輪 381 項通過 (65.06 秒), 完整 serial HTTP 單輪 225 項通過 (131.32 秒)

### Platform 回應編碼與精確數值

依 LAM main `5c03daf` 的 `OpenAIPlatformSource.query`, HTTP bytes 先受 1 MiB 限制再以 `json.loads(..., parse_float=Decimal)` 解碼, Rust 改用既有私有 JSON bytes decoder, 沿用 serde 的 arbitrary_precision 與原有金額投影

Python 原始 query 以模擬 transport 驗證 models / usage / decisions / costs 共 40 組編碼與 2 組原始上限, 僅使用合成 key, 沒有外部請求且未使用真實憑證

Rust loopback HTTP 回歸先失敗後通過, platform 9 項通過, 涵蓋 UTF-8 BOM、UTF-16/32 LE / BE、有 / 無 BOM、18 位小數金額與 i64 最大值計數, 剛好 1 MiB 接受、超出 2 bytes 與截斷編碼拒絕

`/api/openai-platform` 的來源選擇、外部路徑、時間參數、快取、狀態分類及白名單不變, 未新增請求或放寬憑證條件

此結果分別來自 Python 模擬 transport 與 Rust loopback reader, 不代表實際帳戶、外部 Platform 或跨平台認證驗收, surrogate、歷史失敗與其他限制保留

Linux / Rust 1.100.0 的完整 service 單輪 382 項通過 (62.51 秒), 完整 serial HTTP 單輪 225 項通過 (129.65 秒)

### 關聯子代理尾段的原始列選取

依 LAM main `5c03daf` 的 `CodexCollector.subagent_lifecycle`, 只讀取已選定、未載入的關聯 session 尾段, body 使用 CR / LF / CRLF 分行, 並先要求原始 bytes 含帶雙引號的 task_started 或 task_complete 再解析 JSON

Rust 改用既有 byte iterator 與 JSON bytes decoder, 補齊 CR 分行、UTF-8 BOM 與原始標記篩選, 保留 64 KiB tail、讀取預算、檔案身分 / symlink 檢查、快取及同時間選取順序

超限後第一段仍只依 LF 丟棄, 不改成 CR 分隔, UTF-16/32 與只有跳脫文字的標記仍不通過原始 byte 篩選, 標記位於其他欄位時仍沿原始流程繼續解析, 不將此條件擴大套用到一般 session 收集

本機 Python 的 decode_record 呼叫與 main 的 json.loads 有差異, 因此另取固定 main 的完整方法直接驗證 8 組 fixture, 沒有宣稱整份 collector 相同

Rust 新回歸先失敗後通過, related_lifecycle 3 項通過, 新 snapshot HTTP fixture 1 項通過, 驗證 session header 所屬 parent 與檔名 child 不同時確實走關聯 fallback, CR 與 BOM 的完成狀態進入投影且不外露本文

`/api/snapshot` 的 thread status / status_updated_at / status_source 與 metadata_sources.rows_read 仍來自既有選取尾段, 不新增 endpoint、參數、檔案讀取或保存欄位, surrogate、歷史失敗與其他限制保留

Linux / Rust 1.100.0 的完整 service 單輪 383 項通過 (66.43 秒), 完整 serial HTTP 單輪 226 項通過 (131.49 秒)

### Account JSON-RPC 的最後一列

固定 LAM main `5c03daf` 的 `codex_account.py` 與本機檔案 blob 相同, 原始 reader 的 readline 允許 EOF 前最後一列沒有 LF, Rust 移除額外的 LF 必要條件, 完整 JSON 仍須通過物件、request id 與 result 驗證

Python 原始 source 與 Rust 合成 app-server 各驗證 6 組 fixture, 涵蓋無 LF、CR 尾碼、截斷 JSON、空 EOF、累計剛好 1 MiB 及超出 1 byte, 無效回應仍保留前面成功結果並標示 partial

Rust 新回歸先重現 partial 而非 ok, 修正後 account 12 項通過 (0.51 秒), 累計原始輸出限制、時限、來源欄位順序、子程序清理與快取不變

此輪沒有變更 JSON 編碼 decoder, 未宣稱 UTF-16/32 account protocol 相容, 沒有啟動真實 Codex 或使用帳戶憑證, read-only account 方法、參數、次數與投影白名單不變, 跨平台驗收與歷史限制保留

完整 service 首輪 383 項通過、此新 fixture 1 項失敗, 回報 error 而非 ok, 尚未證明原因, 後續加入 fixture 編號與結果診斷, 並將此測試的 3 秒期限對齊正式 reader 原有的 10 秒, 未改動正式時限

完整 serial HTTP 單輪 226 項通過 (127.49 秒), 執行起始與首輪 service 部分重疊, 不將此輪當成獨立效能基準

最終測試設定的完整 service 單輪 384 項通過 (64.74 秒), 不消除首輪失敗的未明原因

### Account JSON-RPC 的 UTF-8 BOM

沿用已核對的 LAM main `5c03daf` account source, Python json.loads 接受各原始回應列開頭的 UTF-8 BOM, Rust 在累計原始 bytes 上限檢查後移除一個 BOM, 同一份借用資料同時交給值解析與保留欄位順序的解析器

沒有重新序列化 JSON 或變更 map 順序, 合成 app-server 的 initialize、account/read、rateLimits/read 與 usage/read 回應皆含 BOM, 重複 zulu bucket 保留最後值且仍優先於 alpha, Python 原始 reader 與 Rust fixture 皆驗證此結果

Python 另以 6 組 BOM / EOF / 累計上限 fixture 驗證原始行為, Rust 新回歸先失敗後通過, account 13 項通過 (0.51 秒), 既有無 BOM 的精確上限與 EOF 回歸保留

僅補 UTF-8 BOM, 未擴大為 UTF-16/32 協定支援, 方法、參數、查詢次數、快取與白名單不變, 沒有真實帳戶請求, 前輪 fixture 未明失敗、其他歷史限制與跨平台驗收保留

Linux / Rust 1.100.0 完整 service 單輪 385 項通過 (61.45 秒), 結束後執行完整 serial HTTP 單輪 226 項通過 (129.87 秒)

### Account JSON bytes 與原始欄位順序

沿用已核對的 LAM main `5c03daf`, account 的 readline 在原始 LF 分割後交給 json.loads, 最後 EOF 的 UTF-16/32 LE / BE JSON 可接受, 有編碼 LF 時 BE 可接受但 LE 會被截斷而拒絕, 不將此行為描述為任意多列寬字元協定支援

既有私有 JSON bytes decoder 抽出相同的泛型解析流程, 原 Value 呼叫維持介面, account 的值與 RawValue 皆使用此流程, RawValue 保留原始 key 順序與重複值, 不由已排序的 Value 重新序列化

Python 原始 source 的 8 組合成 app-server 編碼 / 分行 fixture 通過, Rust 對既有 EOF 回歸增加相同 8 組, 並另驗證四種編碼的 zulu / alpha 順序、重複 zulu 最後值與截斷拒絕, account 14 項通過 (0.51 秒)

累計原始 1 MiB 上限、時限、LF framing、方法參數、請求次數、快取與投影不變, RawValue 現在使用 owned buffer, 原始輸入上限不等於整體程序記憶體上限, 未新增相依套件

沒有真實帳戶或認證操作, surrogate 差異、先前 fixture 未明失敗、其他歷史限制與跨平台驗收保留

Linux / Rust 1.100.0 完整 service 單輪 386 項通過 (59.19 秒), 結束後完整 serial HTTP 單輪 226 項通過 (136.64 秒)

### 保留 Skill 事件的文件尾段查詢

核對 LAM main `5c03daf` 的 skill_detail, 保留事件對應的 session 尾段使用 bytes.splitlines 與 json.loads, Rust retained_skill_paths 改用既有 byte iterator / JSON bytes decoder, 補齊 CR 分行與 UTF-8 BOM

僅修改已觀察 Skill 的保留事件 fallback, 未更動一般 session、工具本文與 context detail parser, 既有 call / skill / timestamp 比對、檔案身分、anchor、symlink、checkpoint guard、讀取預算與超限首段 LF 丟棄方式維持不變

Python 依固定來源的 splitlines / json.loads 流程搭配現有 workflow_operations 驗證 6 組 CR / LF / CRLF 與有無 BOM, 此為對應流程測試而非完整 skill_detail endpoint 驗收

Rust 新回歸先於 CR 案例失敗, 修正後 1 項通過 (0.01 秒), 涵蓋相同 6 組、實際原始 bytes 預算扣除與未知 call 拒絕, 不開啟真實使用者文件

受影響的 /api/codex/skill 仍依 skill、thread_id、call_id 選取已觀察來源, 沒有新增來源查詢或放寬文件允許條件, surrogate 與歷史限制保留

Linux / Rust 1.100.0 完整 service 單輪 387 項通過 (65.10 秒), 結束後完整 serial HTTP 單輪 226 項通過 (130.32 秒), 包含既有保留 Skill 尾段 HTTP 查詢回歸

### 索引 detail 原始列的 1 MiB 上限

核對 LAM main `5c03daf` 的 detail_lines, 已觀察 offset 的 readline 結果須包含 LF 且原始長度不超過 1 MiB, Rust 原先移除 LF 後才檢查長度, 多接受 1 byte

read_record 改在移除 LF 前檢查原始長度, LF / CRLF 均計入上限, hash 仍針對移除最後 LF 後的原始內容, 不修改 offset、hash 格式或 JSON 解碼, 無 LF 的索引列仍拒絕

Python detail_lines 的 LF / CRLF 精確上限與超出 1 byte 共 4 組 fixture 通過, Rust 新回歸先重現超限接受, 修正後 1 項通過 (0.20 秒), 同時驗證錯誤 hash 與缺少 LF 拒絕

影響共用索引列讀取的工具、SQL、訊息、錯誤及 context detail 呼叫者, 僅縮回來源既有邊界, 查詢參數、讀取次數、權限與保存資料不變, 沒有擴大或修改 legacy tail fallback, 歷史限制保留

Linux / Rust 1.100.0 完整 service 單輪 388 項通過 (59.81 秒), 結束後完整 serial HTTP 單輪 226 項通過 (138.39 秒)

### 39c6f98 的 metadata 503 隔離重查

以 exact `39c6f98` tracked source archive 與既有固定前端資產建立獨立來源副本, 沿用 5001 個合成 session 的 overflow HTTP fixture、5 秒 socket / server 期限與原 assertions, 未連線真實帳戶或讀取使用者來源

未加 instrumentation 的單次 baseline 通過, metadata POST 於 3.796831409 秒回 200, child 存活, 後續 metadata=false、private workdir 不可見與工具憑證遮蔽 assertions 通過, 此次沒有重現歷史 503 / WouldBlock

另於隔離副本加入只輸出 version、byte count 與 elapsed 的 snapshot 診斷, 主來源 runtime 未變更, 單次診斷 fixture 同樣通過, POST 於 3.510865775 秒回 200

診斷版 metadata 更新 cycle 的四個 window 約各 3.73 MB, JEV field 完整驗證 / 擷取分別 129 / 126 / 135 / 125 ms, windows 與 logs 合計 2978 ms, snapshot 起點至 error_details 前為 3137 ms

兩次樣本皆沒有重現失敗, instrumentation 與執行環境可影響時間, 不推定歷史失敗根因、不宣稱穩定達標, 不延長 timeout、降低 fixture 容量、略過 JSON 完整驗證或改寫錯誤回應

目前結果支持繼續評估 snapshot 組裝成本, 但尚不足以採用略過驗證的 JEV descriptor 快取, 原歷史失敗、16 MiB staging 與跨平台接受仍未解決

### Single-pass selector 的隔離候選

候選保留 reserved serde key / Unicode escape 的既有 Value fallback, 其他物件一次遍歷, 目標欄位解析為 Value、其餘完整使用 Checked 驗證, duplicate key 取最後值, 結尾仍檢查 trailing data

對非 object root 先以 Checked 驗證再回 null, 避免 arbitrary_precision 的 number synthetic map 被誤當真實欄位, 原 selector 另保留為 test-only oracle

74 組有效 / 無效 fixture、其全部截斷前綴及四種目標欄位的對照通過, 包含非物件、精確數字、深度邊界、reserved key 與 Unicode escape, 不代表任意 JSON 輸入的形式化等價證明

3,678,608 bytes 合成 JSON 的六組交替配對, debug build 中位數為舊 selector 65.569 ms / 候選 43.631 ms, 第六組兩者均有較大時間變異, 不將此結果推論為完整 endpoint 改善

接著在獨立來源副本將 field 指向候選, 保留原 selector 作測試 oracle並重跑對照, 2 項通過, 實際 5001-session overflow HTTP fixture 分別單次執行候選後 baseline, 原 5 秒限制與隱私 / detail assertions 均保留

候選 metadata POST 於 3.68249689 秒回 200、fixture 14.10 秒, baseline POST 於 3.158204272 秒回 200、fixture 11.72 秒, 兩者皆未重現 503, 此單組 endpoint 結果未顯示候選較快

主 runtime 未採用候選, 不宣稱修復 503 或取得端到端效能收益, 保留原歷史失敗與驗收界線

### 相依 Rust 套件的完整驗證補查

在 `d72f05f` 的 Linux / Rust 1.100.0 執行 history-store manifest 的完整 locked 測試, 53 項通過: unit 9、legacy 6、migration 7、observation_view 18、storage 13, doc tests 0 項

session-core manifest 加入既有 sqlite-python feature 的完整 locked 測試 34 項通過 (0.39 秒), doc tests 0 項, 未改動相依套件或 lockfile

此補查涵蓋 service / HTTP 測試不會自動執行的相依套件測試, 不代表 Windows / macOS 或 Python extension 驗收, session-scan manifest 明確停用 test / doctest, 未將零測試視為 extension 通過

16 MiB 稽核仍見 selected_overflow_projections 保存完整 Projection vector 並於完成後 clone, 現有 winner / reducer prototype 未接入執行期, 跨輪來源與設定版本、原子四 window / detail 發布仍需完整設計與驗證, 本輪不新增截斷、近似統計或儲存政策

### Linux 合成介面的執行限制

以既有 serve_loading_fixture 的資料產生段建立 12-session 合成 home, 改由 Rust 服務啟動, 未使用真實 Codex home 或帳戶, 原始與正式檔案未改寫

Playwright / Chromium 在導航前因 Unix socket Operation not permitted 結束, 一次 supported escalation 重試得到相同限制, context-agent / context-refresh flow 均未執行, 不列為 UI 通過

首次 fixture wrapper 因 stdin 結束而退出, 因此當時 browser 的 connection refused 不足以證明網路隔離, 後改為有時限的 fixture, 但 supported browser 的 failed-tab 檢視與新 origin 皆遭安全政策拒絕, 未繞過、變更隔離或切換裝置

有時限 fixture 已在 120 秒後正常結束, 未留下服務程序, failed browser tab 的清理亦被政策阻擋, 未取得畫面或 visual acceptance

目前工具鏈僅安裝 x86_64-unknown-linux-gnu target, 此環境未找到 Windows cross compiler, 不宣稱 Windows / macOS 編譯或原生驗收

### 查詢輸入 capture 的最小前置實作

依跨輪計畫先抽取既有同步 snapshots 內的查詢讀取段, 私有 SnapshotQueries 保存 account / cloud / public / platform 共 8 個來源的 settings 與 owned result, 仍在原有四 window 前的位置擷取

原讀取段逐 byte 比對相同, 保持 account settings→result、cloud settings→result、public settings→results、platform settings→results 的順序與既有 clone 次數, 不新增查詢、鎖或依賴

針對性 1 項通過, 以原直接讀取流程為 oracle 比較預設缺值、已設定來源、零值 / false / null, capture 不改變來源 version, 之後修改來源不改變已擷取結果

此物件不是 settings/result 的原子讀取或跨輪 token, 未改 reference、cutoffs、裝置讀取、detail_writer、send_replace 或 HTTP schema, 尚未實作其餘 capture 範圍及跨輪發布

Linux / Rust 1.100.0 完整 service 單輪 389 項通過 (70.58 秒), 結束後完整 serial HTTP 單輪 226 項通過 (131.59 秒), 未重試已記錄的 browser 阻擋項目

### 來源設定 capture 的同步前置實作

私有 SnapshotSourceSettings 保存既有 tracking、MCP source 開關、customizations 與 MCP / web 開關, 仍在活動合併後、MCP 篩選前的原位置擷取

原 tracking→sources / customizations→switches 讀取段逐 byte 保留, sources 與 customizations 的同時持鎖範圍及釋放位置不變, 沒有合併或延長到跨輪的鎖

針對性 1 項通過 (0.01 秒), 原直接讀取 oracle 比較預設與已設定值, 涵蓋四種 customization map、false source、MCP / web 分流與 tracking, 後續來源變更不改寫已保存的 map

此步不提供共同設定 revision 或 atomic publication, 沒有改動 source query、HTTP schema、metadata / detail 隱私 guard 或即時 telemetry, 跨輪與 16 MiB 待辦保留

Linux / Rust 1.100.0 完整 service 單輪 390 項通過 (64.71 秒), 結束後完整 serial HTTP 單輪 226 項通過 (129.67 秒)

### 查詢 version 的 producer 邊界驗證

新增兩項 Shared producer 回歸測試, 保留原始 runtime, 直接呼叫現行 configure / publish, 不引入假想 revision 或改動發布介面

- 預設/相同設定與唯讀操作不增加 version, enable、interval、disable 變更保留原 force_due 與事件行為
- 完全相同的 cached payload 仍更新 completed 並增加 version, wake queue 已滿不阻止 publish, 零值 / false / null 保留, 後續不同 payload 仍可通知

來源稽核確認此 counter 是事件版本, settings/result/completed 分開寫入後才 signal, 不是共同 atomic capture 或僅內容變更的 revision, 詳見 maintenance 的查詢 producer 邊界

Linux / Rust 1.100.0 針對性 2 項通過 (0.00 秒), 完整 service 單輪 392 項通過 (74.78 秒), 無 runtime / endpoint / query 次數變更, 未重跑前輪已通過的 HTTP 226 項, 未重試 browser 阻擋項目, 尚未驗證跨輪一致性或原生 Windows / macOS

### Active / history producer 稽核與同 key mutation

依 `4caa994` 追查 Session read/reset、Projection consume/context/complete、retire / error bound、checkpoint save/conflict 與 history refresh, 更新 maintenance 的 mutation owner 表, 未加入 revision 或切換架構

新增 `payload_mutations_preserve_selection_keys_and_latest_timestamp` 單輪通過 (0.01 秒), 真實 consume producer 在 call ID / time / 數量與 Projection updated 不變時接受錯誤回覆及相同時間戳的 model / effort 修改, 同時驗證有效無 payload 行與 malformed 的回傳值 / 狀態邊界

此輪只新增測試與文件, 無 runtime 變更, 未重跑前輪 service 392 項或 HTTP 226 項, 不把針對性通過擴稱為 source replacement、儲存 rollback、跨輪取消或原子發布驗收, browser 阻擋項目未重試

### 共同發布設計的 reader 稽核

盤點 `server.rs` 8 個 detail index 讀取點, 涵蓋 SQL、error、Git/check、skill、JEV、MCP、tool、context/agent-message, 目前各自與 snapshots 分開讀取, MCP 先取 candidates 再取 all-window event

maintenance 新增以既有 Snapshots 擁有 immutable detail index 的整合草案, 區分共同發布、跨輪 capture、實體來源驗證與 live disable, 不新增 counter、channel、runtime 或 endpoint, 不宣稱已重現混版回應

此輪為來源碼/文件稽核, diff 檢查通過, 尚未執行草案的交界注入、跨輪或 HTTP 整合驗收, 既有測試結果沿用各自記錄範圍, 未重試 blocked browser

### 快照與明細的共同發布

Snapshots 現在以 Arc 擁有 detail index 與 error_details, 移除 Monitor / server State 的獨立 detail RwLock, worker 不再先公開明細再發布快照, 沒有變更 index 的批次保留原 Arc

SQL、error、Git/check、skill、JEV、MCP、tool、context/agent-message 的 8 個 reader 位置統一擷取同批 all-window / error map / index, request 只做 Bytes / Arc clone, 在來源 I/O 前釋放 watch borrow, 未修改 endpoint 參數、查詢主體、response 欄位來源或外部查詢次數

`detail_request_keeps_one_publication_across_replacement_and_live_guards` 使用兩組真實 session 檔案經 Collector 建立的發布物件, 在 request capture 與 route 讀取之間替換 watch 發布, 驗證舊 request 的 tool / error 仍對應舊內容, 新 request 對應新內容, 移除舊來源後拒絕過期明細, capture 後停用仍回傳 conflict

針對性初版與補強 error coupling 後各 1 項通過 (各 0.06 秒), 完整 service 單輪 394 項通過 (75.99 秒), 完整 service 執行後僅補強同一測試的 error assertions, runtime 未再修改

首次 serial HTTP 在建置期間遇執行中斷, 記錄僅有編譯警告而無測試結果, 不列為通過或失敗, 恢復後執行未完成的必要 HTTP 檢查, 非重試 browser 阻擋項目

此處交界注入使用實際 watch 與 route, 不是 worker 任意階段的 fault injection, 尚未驗證跨輪 candidate 失敗/取消、共同輸入 revision、全部來源同時一致或移除 16 MiB staging, 原待辦保留

恢復後完整 serial HTTP 單輪 226 項通過 (132.39 秒), 涵蓋既有真實 worker / HTTP 明細、停用、checkpoint、來源保護與 5 秒設定等待回歸, Linux / Rust 1.100.0, 未執行原生 Windows / macOS 或 browser 驗收

### 單一來源 settings / result paired capture

Shared 以同一個 QueryState RwLock 保存 settings 與 retained result, 新 capture 供 SnapshotQueries 使用, 不再將單一來源的兩次獨立讀取組成一對, public / platform 改成逐來源成對讀取

原有 getter、configure / publish、no-op、force_due、completed、cached 相同結果的 signal、queue 滿時 version 行為保留, 沒有新增 counter、外部查詢或 HTTP 欄位, 仍只 clone 每個 result 一次

直接讀取 oracle 的 1 項 capture 測試通過 (0.00 秒), 4 項 Shared producer 測試通過 (0.00 秒), 其中新增 retained result / owned capture 與共同讀鎖下真實 configure / publish writer 的交界測試, 不使用 sleep 或競速假設判斷成功

完整 service 單輪 396 項通過 (69.18 秒), 此步只保證單一來源當下的 settings/result 共同讀取, 不代表結果由目前設定產生、completed 同步、8 個來源共同版本或跨輪 candidate token

本輪完整 serial HTTP 未全數通過: 225 通過 / 1 失敗 (134.04 秒), `mcp_and_web_observation_switches_filter_independently_and_replay` 在 `tests/http.rs:3327` 的 `!stored.contains("444")` 失敗, 原斷言未改動, 未重新執行以取得綠燈

失敗 fixture 的暫存資料庫已隨測試清理, 未保留原始 stored 值, 因此不能將此失敗歸因為 fixture 或宣稱已排除回歸

來源稽核發現 stored 是完整 Call JSON, 含 time 與 request_position.hash, 原斷言對所有文字搜尋 444, 不是只檢查 MCP metadata

另做獨立的合成 producer 診斷, 使用時間 `2026-11-17T00:00:00.444Z`、MCP 關閉與 max_files=444, 真實 consume 序列化結果為 observation=null、mcp_sources_incomplete=true, 完全沒有 max_files, 但 stored.contains("444") 仍為 true, 來源是時間欄位, 診斷單項通過 (0.00 秒)

這只證明原斷言存在誤判可能, 沒有重現或解決原失敗, 診斷暫時程式碼已移出產品 diff 並保留證據, 尚未放寬或替換驗證

後續人工測試應保留失敗時的原始 stored JSON 與 fixture 時間, 先判定 444 位於 time/hash/offset 或真正 metadata, 同時核對 observation、mcp_sources_incomplete 與禁用資料是否落盤, 不以單次重跑成功推翻本次失敗, 此 checkpoint 不列為完整 HTTP 驗收通過

### 禁用 MCP 保存驗證的結構化 oracle

依 Call serde 欄位、Projection::tool_event 的禁用分支與 Checkpoints::save 的序列化來源, 將上述失敗測試的 substring 斷言改為完整 persisted Call JSON 相等比較, 不是只刪除數字檢查或忽略任意欄位

預期完整結構包含 tool、正規化 time、completed=null、sequence=2、observation=null、sql_failed=false、mcp_sources_incomplete=true 與精確 request_position, offset 取附加前檔案大小, hash 由實際附加紀錄去掉最後 LF 後計算

因此多出的 arguments / metadata / 觀測值或任何未知欄位都會失敗, time / offset / hash 中合法出現 444 則依實際來源值驗證, 不造成 substring 誤判, runtime 與禁用行為未修改

修改後針對性實際 HTTP 測試單輪通過 (0.34 秒), 原始失敗的 stored 值已遺失, 仍不宣稱原始失敗原因已確定, 此次是基於已確認 oracle 缺陷的測試修正

最小測試修正後, 完整 serial HTTP 單輪 226 項通過 (131.97 秒), service runtime 未變更, 沿用前輪 396 項通過的範圍, 不抹除前輪 225/1 與原始失敗未知的紀錄, 未重試 browser 或擴大至跨平台驗收

### 真實 worker 的共同發布回歸

新增 `worker_publishes_detail_index_with_matching_snapshot_and_keeps_old_index`, 啟動真實 Monitor worker 讀取合成 session, 等待第一筆 call 發布後附加第二筆, 透過既有 manual refresh / wake 與 watch channel 等待更新

對讀取到的非空 publication 同時檢查 snapshot observed_tool_calls 與同批 index 的 a / b call 成員, 第二批 version 增加且 index Arc 不同, 保留的第一批 bytes / Arc 仍只有第一筆, 第二筆 locator 可通過實際來源讀取

針對性單輪 1 項通過 (0.08 秒), worker 在測試結束透過 Monitor Drop 停止, 無 runtime 變更, 此測試補足前輪僅注入 watch / route 的範圍

watch 可能合併未讀通知, 此測試只驗證實際讀到的兩批更新, 不宣稱覆蓋所有交錯、任意 worker fault injection、跨輪 candidate 取消或 16 MiB staging, 保留既有失敗與阻擋紀錄, 未重試 browser

完整 service 單輪 397 項通過 (61.53 秒), Linux / Rust 1.100.0, 本輪只新增測試與文件, 未重跑前輪已通過的 serial HTTP 226 項, 未擴稱原生平台或 browser 驗收

### 已完成 payload stage 的共用驗證

在完整來源驗證成功後 move staged vector 到 Arc, snapshots 只借用 slice, 沒有變更 selector、winner、merge、HTTP 欄位或外部查詢次數

首次針對性執行 2 項失敗: 新增的 completed cache 在 signature-mismatch early return 未清除, 以及既有 65-source 測試仍檢查舊 mutable Vec 的保存位置, 尚未發布此實作

已修正該 early return 清除 completed, 並把內部保存位置斷言改為 mutable Vec 必須為空、completed 必須有 65 筆, 來源/完整性 oracle 未放寬, 修正後針對性單輪 2 項通過 (0.09 秒)

既有 600-call materialized oracle 保留, 新增 Arc::ptr_eq 確認成功再驗證共用同一個 allocation, signature 改變後必須回傳 None 並清除 completed, 先前交付的 600-call Arc 內容不變, 65-source 跨輪驗證、取消、預算不足與 source signature 檢查仍保留

此證據驗證 allocation 共用與生命週期, 未量測程序 RSS、端點延遲或宣稱全部輸入同時一致, 原 16 MiB staging 與跨輪 candidate 一致性缺口仍存在

完整 service 單輪 397 項通過 (77.04 秒), 結束後完整 serial HTTP 單輪 226 項通過 (129.22 秒), Linux / Rust 1.100.0, 初始 2 項失敗記錄保留, 未重試 browser 或改動原生平台驗收界線

### 真實 worker 停用與重新啟用的發布

擴充既有 worker publication 測試, 第二批發布後停用收集, 等待 disabled publication, 再於停用期間附加第三筆 call 並重新啟用, 驗證 version 繼續增加、新 index 可讀取第三筆, 保留的停用前 index 不會取得第三筆

初次測試失敗是新增斷言假設 disabled 的 observed_tool_calls=0, 原始碼明確將 codex 換成精簡 disabled 物件並省略該欄位, 未因此修改 runtime 或 schema

依實際來源修正為必須省略 observed_tool_calls、threads=[]、tools={}, 修正後針對性單輪 1 項通過 (0.11 秒), 原始新增測試的失敗紀錄保留

本輪僅擴充測試與文件, 未重跑前輪 service 397 / serial HTTP 226 項, 不宣稱覆蓋每個交錯、跨輪候選取消或原生平台, worker 已透過 Drop 停止, 未重試 browser

### 個別 MCP source 停用的保存 oracle

另行檢查 `mcp_source_setting_filters_history_details_and_replays_on_enable` 的 777 substring 斷言, 本輪未觀察到此測試失敗, 修正依據為同樣會搜尋 time / offset / hash 的結構問題, 不將先前 444 的未知失敗原因套用到此測試

依來源保留個別 source 停用與全域 MCP 停用的差異: 前者保存 observation identity / category / action, metadata 與 result 必須為空, failure 不得保存

改成完整 Call JSON 比較, 包含正規化 time / completed、sequence、狀態、精確 request / response offset 與 SHA256, 任意額外欄位也會失敗, 未修改 runtime 或後續重新啟用的 replay 斷言

針對性實際 HTTP 單輪 1 項通過 (0.25 秒), 本輪僅測試與文件, 未重跑前輪完整 service 397 / HTTP 226 項, 既有未知失敗與 browser 阻擋紀錄保留

### Snapshot 輸入 owner 稽核與現況同步

依 `7338ce6` 讀取 snapshots / error_windows / render 及 JEV、platform credentials、source registry、worktree / core / journal helpers, 更新 maintenance 的 owner 表與過時的獨立發布、深複製、分離查詢讀鎖描述

新確認的剩餘邊界包括每個 window 的 JEV config/唯讀 SQLite I/O、credential availability 環境讀取, 以及 error_windows 額外讀 mcp_sources / journal, 明確區分已載入 registry/worktree projection 與即時 render telemetry

本輪只有文件修改與來源稽核, diff 檢查通過, 未執行新 runtime 驗收、未調整查詢/鎖定順序、未將來源生命週期風險描述為已觀察到錯誤, 既有跨輪/容量/原生平台與 browser 界線保留

### Journal 單次擷取與視圖共用

在原 snapshots 開頭讀取 journal events 的邊界, 同一既有鎖一併擷取 entries 與 source, error_windows 與 logs overview 改用同一 owned capture, journal 鎖由每批 3 次改成 1 次, events 不再重複複製, schema、時間窗過濾、排序、monitor error 與停用規則保留

這會讓擷取後抵達的 journal 事件於下一批發布才出現, 避免同批 monitor events、errors 與 logs source 分別讀到前後狀態, 不將 journal capture 宣稱為跨來源或跨輪 revision, JEV、credentials、debug 與即時 render 未變更

新增測試逐一比較擷取值與原 Journal getters, 之後加入 HTTP 503 並停用 journal, 原擷取的 error/log bytes 保持不變, 新擷取保留 live error、entries 為空、source disabled 與正確 retained count, 針對性單輪 1 項通過 (0.01 秒)

完整 service 單輪 398 項通過 (63.72 秒), 接續完整 serial HTTP 單輪 226 項通過 (138.78 秒), Linux / Rust 1.100.0, diff 檢查通過, 未重試已受阻 browser 或改變原生平台、16 MiB 與跨輪驗收界線

### JEV 四時間窗固定資料 oracle

新增 172 筆合成 SQLite 資料: reference 往前 0 至 170 小時的整點資料, 加上 reference 後 30 分鐘一筆, 四窗預期 calls 分別 3 / 26 / 170 / 172

完整比較 summary 的 calls、since、attempts / retries、known / unknown tokens、bytes、latency 與 statuses, 並驗證 recent 的完整 timestamp 順序與 80 筆上限、series 完整群組順序 / count 與 168 群組上限, 驗證 cutoff 包含邊界、沒有 future upper bound、enabled_at 不截斷歷史, 以及 privacy projection 與 paused 欄位差異

初版 focused 單輪 1 項通過 (0.04 秒), 之後加強 recent / series 為完整順序比較, runtime 未修改

加強後 JEV 模組單輪 4 項通過 (0.04 秒), diff 檢查通過, 本輪僅測試與文件, 未重跑前輪 service 398 / serial HTTP 226 項, 尚未執行並行 writer 的可見性驗收, 既有失敗與受阻項目紀錄保留

### JEV 受控 writer 可見性

新增 deterministic WAL 雙 connection 測試, reader 使用 runtime 相同唯讀 / NOFOLLOW / NO_MUTEX、query_only / trusted_schema 與 80 ms timeout, 不使用 sleep 或競速重試

首個 count SELECT 完成後由 writer transaction 新增資料並 commit, 再實際呼叫現有 details, autocommit 分支保留舊 calls=1 但 statuses / recent / series 與新 count 為 2, explicit BEGIN 分支全部仍為 1, ROLLBACK 釋放 reader 後讀到 2, writer 最終資料均為 2

針對性單輪 1 項通過 (0.01 秒), 本輪沒有 runtime transaction 變更, 首個 count 僅模擬 summary 讀取邊界, 不宣稱已在完整 snapshot / HTTP 注入 writer 或驗證所有 SQLite journal modes, 未重跑先前 aggregate suites, diff 檢查通過, 既有受阻與歷史失敗不重試

### JEV 單次 snapshot 短生命週期讀取 transaction

將原 summary 與 details 的 4 個 SELECT 保留原文並移入 read_snapshot, 使用預設 Deferred transaction, 成功時顯式 ROLLBACK 後回傳 owned values, query failure 使用 RAII 清理, 原 config / identity / timeout / error mapping 不變

受控 WAL 測試擴充為實際 production read_snapshot, 僅測試組態在 summary 完成後注入 writer commit, 當次 summary / statuses / recent / series 均維持 2 筆, 函式返回已回到 autocommit, 下一次讀取均為 3 筆

另新增 summary 缺表、details 缺 metadata 欄位失敗後的 transaction 釋放, 與已有 transaction 造成 BEGIN 失敗時保留呼叫端 transaction 的測試, JEV 模組單輪 6 項通過 (0.06 秒), 原四窗 cutoff / cap / privacy oracle 保持通過

這次注入涵蓋實際資料讀取 helper, 不是完整 HTTP 內注入, 不宣稱四個 window 或跨輪 candidate 原子一致, 並行 writer 驗證限於 WAL, 保留原 80 ms timeout 與原生平台、browser 驗收界線

完整 service 單輪 401 項通過 (66.39 秒), 接續完整 serial HTTP 單輪 226 項通過 (134.54 秒), Linux / Rust 1.100.0, 另以程式比較確認 summary SQL 與 JSON 欄位 projection 和前版完全相同, diff 檢查通過

### JEV rollback journal 的 writer 釋放邊界

以 `286d92d` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, 恢復官方 Rust 1.100.0 後使用原 Cargo.lock 建置

新增 DELETE / TRUNCATE / PERSIST 三種 journal mode 的 deterministic 雙 connection 測試, reader 沿用 production 唯讀旗標、pragmas 與 80 ms timeout, writer 的 timeout 設為 0 以立即觀察鎖定, 不使用 sleep 或競速重試

在實際 read_snapshot 的 summary 後插入一筆並嘗試 COMMIT, 三種模式均回 DatabaseBusy 且保留 writer transaction, 當次 summary / statuses / recent / series 均維持 1 筆, reader 返回後已回 autocommit, 同一 pending writer 可成功 COMMIT, 下一次讀取的四項投影均為 2 筆

JEV 模組單輪 7 項通過 (0.07 秒), diff 檢查通過, 本輪僅測試與文件, runtime SQL / timeout / endpoint / response 欄位與查詢次數未變, 前輪 service 401 / HTTP 226 為歷史結果, 四窗共同版本與跨輪 candidate 仍待完成

### JEV 平均延遲的中點取偶捨入

以 `2068448` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, Python `JevCollector.snapshot` 對 SQLite avg(latency_ms) 使用 round, 中點取偶, Rust 原先使用 round 而讓 8.5 / -8.5 變成 9 / -9

實際 Python collector 的 10 組合成資料確認正負中點、非中點、正負 0.5、零、全 null 與空集合, Rust 新回歸先重現 9 與預期 8 的差異, 改成 round_ties_even 後四窗各自符合相同預期, null 與零仍分開

Linux / Rust 1.100.0、原 Cargo.lock, JEV 模組單輪 8 項通過 (0.05 秒), serial HTTP 的 JEV 13 項通過 (3.85 秒), runtime diff 比較確認僅替換 average_latency_ms 的捨入運算, diff 檢查通過

`/api/snapshot` 的 jev.summary 與 mcp.telemetry.jev.summary 仍使用原 jev_events / timestamp>=cutoff 查詢結果, SQL、四窗查詢次數、80 ms timeout、資料庫讀取旗標與 response 欄位未變, 未修改共同 capture 或重跑完整 service / HTTP suites

### JEV 非文字 metadata 的略過行為

以 `69b3a1a` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, Python `JevCollector.snapshot` 略過非字串 metadata, Rust 原先將 BLOB 轉換為 String 失敗後回報整份 telemetry unavailable

實際 Python collector 的混合 SQLite 型別資料確認 8 筆仍計入 summary / statuses / series, recent 只保留 2 筆有效文字 JSON, BLOB、整數、浮點、null、無效 JSON 與非物件 JSON 不產生 recent 項目, Rust 新測試先重現 unavailable, 改為只讀取文字型別後四窗各自通過

Linux / Rust 1.100.0、原 Cargo.lock, JEV 模組單輪 9 項通過 (0.04 秒), serial HTTP 的 JEV 13 項通過 (3.51 秒), diff 檢查通過

`/api/snapshot` 的 jev.recent 與 mcp.telemetry.jev.recent 仍取自 jev_events.metadata, SQL、timestamp>=cutoff、80 筆上限、65536 位元組防護、查詢次數、summary 與 transaction 邊界未變, 未修改四窗共同 capture 或重跑完整 service / HTTP suites

### JEV attempts 投影邊界

以 `562c7e1` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, 新增與實際 Python collector 對照的合成資料驗證, 本輪不修改 runtime

20 筆原始 attempts 的前兩筆不是物件, 先取前 16 筆再過濾後顯示 14 筆, http_attempts 仍為 20, 不以後續項目補足預覽, 巢狀 http_status / latency_ms 保留 0 與 i64 最大值, 負數、布林、浮點、字串及 null 皆為 null

另核對無效 attempt status 變成 unknown、無效 resolved_model 沿用有效 requested_model、無效 source 為 null, null / 物件 / 字串 / 空陣列 attempts 均不產生預覽或計數, 原始 prompt / body 不出現在投影

Linux / Rust 1.100.0、原 Cargo.lock, Python 合成資料與 JEV 模組單輪 10 項通過 (0.06 秒), runtime 位元組比較與 diff 檢查通過, 本輪未重跑 HTTP 或完整 service suites, 不代表跨來源一致性或完整驗收

### Model API 診斷欄位的優先順序

以 `7fdcd76` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, 直接執行 Python `api_diagnostic` 取得 10 組完整輸出, 將固定結果保存為 Rust 結構比對 oracle, 不以 Rust projector 產生預期結果

涵蓋重複 key 的最後有效值、先選 alias 再驗證而不回退、HTTP status 優先於 success、無效主要 endpoint 不改採次要 alias、token 整數限制、duration 小數、retry / connection 移除 request 專用欄位, 以及未結束的引號不覆寫先前有效 model

Linux / Rust 1.100.0、原 Cargo.lock, Model API 模組單輪 7 項通過 (0.21 秒), runtime 位元組比較與 diff 檢查通過, 本輪只有測試與文件, 未新增外部 API 請求或修改資料來源、欄位與查詢次數, 未重跑 HTTP / 完整 service 或實際平台驗收

### MCP p99 的原始浮點數捨入

以 `13df697` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, Python `mcp_records.server_summary` 對插值後的原始浮點數直接 round(value, 2), Rust 原先先乘 100 再捨入, 會使 2.675 的 p99 變成 2.68 而非 Python 的 2.67

實際 Python server_summary 的 8 組資料確認 2.675、1.005、精確中點、1.225、雙值插值、空集合與零, Rust 新測試先重現差異, 改用兩位小數格式化再解析後通過, 同時核對 average / p95、direct calls 計數與 nested call 排除

Linux / Rust 1.100.0、原 Cargo.lock, `sessions::mcp` 篩選共 16 項通過 (包含 mcp_documents, 2.09 秒), serial HTTP `mcp_` 篩選 21 項通過 (7.23 秒), 既有 legacy scalar-summary oracle 未修改且通過, runtime diff 確認只有 production p99 的捨入運算改變

`/api/snapshot` 的 mcp.servers[].p99_ms 仍取自選定 window 中各 server 的 direct-call duration_ms, cutoff、插值、平均值、p95、來源讀取與請求次數未變, 未重跑完整 service / HTTP suites, 不代表記憶體上限或跨來源一致性驗收

### Rust 同輪完整 service / HTTP 回歸

在 `081d3ef`、Linux / Rust 1.100.0 與原 Cargo.lock 執行一次完整回歸, 測試期間未修改來源, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變

- `cargo test --locked --manifest-path rust/service/Cargo.toml --bin local-activity-monitor`: 407 項通過, 69.03 秒
- `cargo test --locked --manifest-path rust/service/Cargo.toml --test http -- --test-threads=1`: 226 項通過, 172.63 秒
- 本輪未重試歷史阻擋檢查, 未執行 browser、macOS / Windows 原生平台或 Library 下載驗收

此結果整合近期 JEV 捨入 / metadata 與 MCP p99 修正的 Linux 回歸證據, 不抹除歷史失敗, 也不代表四窗共同擷取、跨輪 candidate、16 MiB staging、所有 producer revision 或完整重建已驗收

### 已完成 phase 的計時精度

以 `5009ef3` 為基底, 來源為 Python main `5c03daf` 的 `Dashboard.phase`, workbench-workspace main `4616074` 核對未變, Python 完成每個 phase 時將 elapsed_ms / cpu_ms 四捨五入至小數兩位, Rust PhaseTimer 原先直接輸出未捨入的浮點數

以受控 clock 執行實際 Python Dashboard.phase, 7 組資料涵蓋 2.675、1.225、精確中點、1.005、一般小數與零, Rust 新測試先重現差異, production finish 改用可直接測試的私有 phase_metrics 投影, 保留原始計時來源、CPU 差值非負處理與 CPU 不可用時省略欄位

Linux / Rust 1.100.0、原 Cargo.lock, performance_debug 模組 8 項通過 (0.01 秒), 加強的 HTTP debug_collection_reports_only_measured_phase_metrics 通過 (0.08 秒), 核對實際 sessions / worktrees / history 數值的兩位精度, diagnostics 仍保留兩個 timer 結果相加的原規則

影響 `/api/snapshot` 中已完成 phase_metrics 的 elapsed_ms / cpu_ms 及其 debug 記錄, 不改動來源讀取、請求次數、目前進行中 collection 的計時或裝置 capture, 本輪未重跑完整套件, 前輪 407 / 226 為 `081d3ef` 的歷史結果

### 進行中 collection 的計時捨入

以 `34fef7f` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, Python `MonitorState.snapshot` 對進行中的 phase_ms / phase_cpu_ms 直接 round(value, 2), Rust 原先先縮放再使用中點遠離零的 round, 使 0.125 變成 0.13 而非 0.12

受控 clock 執行實際 Python MonitorState.snapshot 的 7 組資料, Rust 新測試先重現差異, 修正後與已完成 phase 共用兩位小數捨入, 保留原 elapsed / CPU 取樣、非負 CPU 差值、CPU 不可用時的 null、waiting / idle / stopped / error 的零與 snapshot 不增加 revision

Linux / Rust 1.100.0、原 Cargo.lock, performance_debug 模組 9 項通過 (0.01 秒), HTTP monitor_snapshot_reports_idle_after_collection 通過 (0.07 秒), diff 檢查通過

僅影響 `/api/snapshot` 與 SSE render 中 monitor.collection 的數值捨入, 未改變 render cache 條件、來源讀取或請求次數, 不固定即時 telemetry 或裝置 capture, 未重跑完整套件

### Debug 記錄的產生時計時精度

以 `2c040b8` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, Python PerformanceDebug.record 將產生的 uptime_ms / process_cpu_ms 捨入到兩位小數, Rust 原先將原始計時精度直接保存

實際 Python record 的 5 組受控 clock 資料確認中點、二進位浮點差異與零, Rust 新測試先觀察到未捨入 uptime_ms, 修正後核對記憶體記錄與 JSONL 的產生時計時相符, peak_rss_bytes 保持整數, 呼叫端提供的 refresh_ms 精度與敏感欄位排除不變

Linux / Rust 1.100.0、原 Cargo.lock, performance_debug 模組 10 項通過 (0.02 秒), serial HTTP debug_ 篩選 7 項通過 (0.54 秒), diff 檢查通過

僅在 debug 記錄產生時轉換這兩個計時欄位, 不修改共用 process_metrics 原始樣本、既有匯入記錄或輪替規則, `/api/snapshot` 的 monitor.debug.records 與對應 JSONL 沿用原來源和請求次數, 完整套件未重跑

### Uptime 秒數的中點取偶

以 `a042f21` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, Python MonitorState.snapshot 對 uptime 使用 round, Rust 初始快照與即時 render 原先使用中點遠離零的 round, 在恰好 0.5 秒時回傳 1 而非 0

受控 clock 執行實際 Python snapshot 的 12 組資料, 涵蓋零、數個半秒及相鄰 1 ns 邊界, Rust 新測試先重現差異, 兩個原有讀取點改為共用 Duration 到中點取偶秒數的轉換, 不變更 Instant 起點或取樣次數

Linux / Rust 1.100.0、原 Cargo.lock, focused uptime 測試通過, HTTP idle_snapshot_and_new_stream_report_current_uptime 通過 (17.00 秒), 確認無新 collection 時 snapshot 與新 SSE stream 仍回報目前 uptime, diff 檢查通過

影響 monitor.uptime_seconds 與其原有整秒 render cache key 的中點選擇, 不修改 cache 欄位、traffic / collection revision 或裝置發布, 未重跑完整套件

### af7c18e 計時修正後完整回歸與保留失敗

在 `af7c18e`、Linux / Rust 1.100.0 與原 Cargo.lock 執行一次完整回歸, 期間來源未改, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變

- service binary: 411 項通過, 67.90 秒
- serial HTTP: 224 項通過、2 項失敗, 179.41 秒, 不是完整通過
- `core_sql_diagnostics_follow_the_wal_and_publish_only_metadata`: `tests/http.rs:10000` 的 files[0].record_id 實際為 2、預期為 3, 尚未進入後續 read_lines / append / restart 驗證
- `overflow_tool_detail_uses_verified_source_and_masks_secrets`: metadata POST 經 5.004106815 秒收到 HTTP 503 與 Settings temporarily unavailable, 已讀 455 bytes, child 仍存活, 未通過預期 200 / 202, 後續設定與明細驗證未執行

單次原始碼檢查發現 Core fixture 的前三筆 INSERT 各自 commit, 等待條件只要求 SQL total=1 與 errors total=1, 前兩筆已能滿足, 隨後卻立即要求 logs cursor=3, 因此等待條件不足以保證第三筆已發布, 這是可辨識的測試同步缺口, 但本輪沒有再次觀察到第三筆是否最終完成, 不宣稱 runtime 遺失或已修復

Overflow 503 與先前保留的 metadata POST 失敗屬相同操作與回應類型, 本輪僅證實再現, 未建立與計時捨入變更的因果, 不延長 timeout、放寬 assertions、重寫預期結果或自動重試

上述兩項保留為使用者後續測試, 後續獨立驗證不得以再跑完整套件的方式默默重試它們, 若跳過兩項須明確標示為篩選結果而非完整 suite

使用者在可用且已授權的環境核對 repository / branch / revision、原 Cargo.lock 與合成 fixture 後, 可分別執行以下既有測試, 保留完整 stdout / stderr、實際退出碼與版本, 這些是待執行步驟, 不代表本輪重跑

```sh
cargo test --locked --manifest-path rust/service/Cargo.toml --test http core_sql_diagnostics_follow_the_wal_and_publish_only_metadata -- --exact --test-threads=1 --nocapture
cargo test --locked --manifest-path rust/service/Cargo.toml --test http overflow_tool_detail_uses_verified_source_and_masks_secrets -- --exact --test-threads=1 --nocapture
```

此失敗紀錄不撤銷 `081d3ef` 的歷史 407 / 226 通過, 也不以舊通過覆蓋目前結果, 四窗共同擷取、跨輪 candidate、記憶體與平台驗收仍未完成

### Linux 合成工作量的記憶體基線

在 `35823b4` 文件 checkpoint、runtime `af7c18e` 的 Linux / Rust 1.100.0 debug binary 執行六個獨立程序, 每種工作量兩次, 沒有執行兩項延後測試或變更 runtime

| 已驗證 tool calls | 來源檔案 | 來源 bytes | 第一次 RSS / HWM bytes | 第二次 RSS / HWM bytes | 回應 bytes 範圍 |
| --- | --- | --- | --- | --- | --- |
| 0 | 0 | 0 | 30,224,384 | 30,261,248 | 26,607–26,728 |
| 1,000 | 2 | 391,018 | 38,084,608 | 38,105,088 | 60,696–60,765 |
| 5,000 | 10 | 1,963,970 | 56,692,736 | 56,307,712 | 196,597–196,599 |

每次使用全新 temporary home 與獨立程序, 每個 JSONL 最多 500 組 function_call / function_call_output, arguments 僅有 64 個 x 字元, 以 loopback snapshot 確認 observed_tool_calls 等於預期、device_ready=true 與 collection=idle, 隨後讀取該 child 的 `/proc/PID/status`, VmRSS / VmHWM 在這六次擷取時相同, 原生 kB 乘 1024 換成 bytes

從啟動到量測的觀察時間為 0-call 0.412–0.550 秒、1,000-call 0.462–0.472 秒、5,000-call 1.222–1.562 秒, 各次 GET 次數為 8–25, 每次量測均包含硬體初始化與 GET / JSON 序列化影響, 不是純 collector 耗時或 release benchmark, 不用兩次樣本推論統計顯著性

六個 child 均於關閉自有 stdin 後正常退出 0, temporary homes 已移除, binary SHA-256 為 `065b19be64fd4451fbb96d3f8db0aa0eb91ca80e0c23ed1041a6b1f6c8096534`

重現方式保存在 [measure_rust_memory.py](../tools/measure_rust_memory.py), 使用已建置的 Rust binary, 腳本不安裝套件、不啟動 browser、不存取真實 Codex home, 執行者另記錄實際 revision、build profile 與 Rust 版本

```sh
python tools/measure_rust_memory.py --binary rust/service/target/debug/local-activity-monitor
```

本輪樣本未超過 active working set 或 16 MiB payload staging, 不能由約 30–57 MB 的觀察值推論記憶體上限、overflow / candidate 正確性、跨平台或完整重建驗收, 原 411 pass / HTTP 224 pass + 2 fail 結果不變

### 記憶體量測 CLI 與同來源分頁容量邊界

`ba91075` 的正式 measure_rust_memory.py 另以明確 --binary 參數完成端到端執行, 六個新 child 均核對正確 call 數並退出 0, 0 / 1,000 / 5,000-call 的 RSS / HWM 範圍分別為 30,380,032–30,773,248 / 37,961,728–38,092,800 / 56,328,192–56,373,248 bytes, 與前節分開保留, 不合併成 release 或上限證據

以 `ba91075` 為基底擴充原有 snapshot_payload_pages_coalesce_metadata_and_reject_stale_or_cancelled_stage, 保留 600-call materialized oracle 不變, 先讀取 512-call 首頁的實際 staged_bytes, 再將測試 budget 設為恰好等於該 charge

首頁仍可暫存且 reader 存活, 同來源第二頁使容量不足後不回傳部分結果, reader / projections / completed 均釋放, staged_bytes 不超額且不排入無來源變化的持續 refresh, 恢復原測試 budget 後從頭讀取並完成 600 calls, 與原 materialized oracle 全結構相符

Linux / Rust 1.100.0、原 Cargo.lock, 該 focused 測試通過 (0.15 秒), runtime 位元組比較與 diff 檢查通過, 未重跑任何延後測試

這驗證既有 charge 門檻的中途取消與恢復, staged_bytes 仍主要依序列化來源長度記帳, 不是所有 allocation 的計量, 未解除 16 MiB staging 容量、未實作 reducer 或跨輪共同 candidate

### Snapshot stage 的 epoch 讀取失敗清理

以 `d8d6758` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, 依既有 stage 失敗須釋放 reader / payload 的規格, 補查頁面讀取前的 selection_epoch 錯誤路徑

原先 epoch query 的 `.ok()?` 會直接返回, 在已讀完第一頁時仍保留 selected_snapshot_scan 與其 source reader, 合成 fixture 暫時將 epoch 改為無法讀成 i64 的值, 先重現 cache 未清除, 修正後 query 失敗清除該 stage 再返回 None

恢復原 epoch 後從第一頁重新讀取, 600 calls 與原有 materialized oracle 完整相符, 容量耗盡、signature 變更與分頁 / 驗證輪次 assertions 均保留

Linux / Rust 1.100.0、原 Cargo.lock, snapshot_payload_ 篩選 2 項通過 (0.15 秒), diff 檢查通過, 未執行延後的 HTTP 測試或完整套件

僅變更既有 epoch query 失敗時的清理, 不改變 query、查詢次數、selection hash、16 MiB budget、正常成功資料或 response schema, 不代表共同 candidate 或整體記憶體上限完成

### Compact detail stage 的 epoch 失敗清理

以 `345f160` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, sibling selected_detail_index_with_limit 同樣在 epoch query 失敗時過早返回, 未清除既有 compact stage 與 reader

沿用 40-call、每筆額外 1 MiB 合法 JSON 空白的既有分頁 fixture, 第一輪保留 reader 後注入不可讀取的 epoch, 先重現 stage 未清除, 修正後只清除 selected_detail_scan 再返回 None, 不變更已發布的 completed_detail_index

恢復 epoch 後重新開始, 第二輪完成所有 40 calls 與 signature 驗證, 原有 previous-publication 選取遮罩、不改寫舊 index、signature 失效及容量 / source / validation 邊界 assertions 保留

Linux / Rust 1.100.0、原 Cargo.lock, selected_details_ 篩選 2 項通過 (2.85 秒), diff 檢查通過, 未執行任何延後的 HTTP 測試或完整套件, 正常 query / 次數 / budget / response schema 不變, 不代表整體 payload staging 容量已解除

### Header selector 前置查詢失敗的暫存清理

以 `8e65de7` 為基底, LAM main `5c03daf` 與 workbench-workspace main `4616074` 核對未變, select_recent_calls 的 epoch / unverified_count 查詢在 heap / cursor 已存在時可能直接返回 None, 留下 pending scan

新增獨立三筆同時間 header fixture, 每輪只讀一筆, 在第一輪後分別注入不可讀 epoch 與暫時不可用的 sources table, 先重現 epoch 失敗後仍保留 scan, 修正後任一前置查詢失敗均清除 recent_call_scan, pending 變成 false, 恢復來源後從頭掃描並選出原排序要求的 b / c

此 selector 不持有 SQLite reader, 清理對象是記憶體 heap / cursor 與其 pending 狀態, 不混同前兩項 snapshot / detail reader 修正

Linux / Rust 1.100.0、原 Cargo.lock, recent_call_selection_ 篩選 3 項通過 (2.68 秒), 包含既有 100,000 筆以上同時間 header 的續讀與 generation-only discovery 保留進度, diff 檢查通過

正常 SQL、查詢順序 / 次數、排序、top-K、epoch / input hash 與成功結果不變, 未執行延後 HTTP 或完整套件, 未新增 candidate 架構

### 同步 opening / time capture 抽取

以 `c8d83a1` 為基準, 保留原 journal → debug 與 overflow 後 reference / cutoffs 的讀取位置, 只抽取私有具名結果與 helper, settings / query capture 不移動, 不新增共同版本或 reducer

新增針對性測試核對原 journal / debug 結果、started fallback、奈秒 reference 與四窗 cutoff, 並確認後續 journal / updated 變更不改寫已擷取值, 既有四窗 materialized 行為測試與 oracle 未修改

Linux / Rust 1.100.0 / 原 Cargo.lock 下, `cargo test --locked --manifest-path rust/service/Cargo.toml --bin local-activity-monitor snapshot_` 通過 58 筆, 355 筆未選取, 測試耗時 6.19 秒, 涵蓋既有四窗 merge / series、winner、preview、journal 與 settings / query capture 回歸

此為 filtered service 測試, 未執行 HTTP 或完整套件, 未重試已延後的 Core WAL cursor 與 overflow metadata POST 測試, 不代表跨輪一致性、staging 容量、裝置或平台驗收

### 同步 diagnostic metadata capture 抽取

以 `5509b7f` 為基準, 原 query capture 後的 diagnostic locations / logs overview 區塊移入私有 helper, 不移動呼叫位置、變更來源順序、增加 I/O 或共同版本, source registry 保留相同的 journal source

新增 ownership 測試核對兩個 buffer 的保存數、來源 metadata 與停用狀態, 後續 journal 寫入不滲入開頭已擷取的 source, 清空 buffer / 改設定也不修改舊結果, 路徑測試使用空來源, 不宣稱已驗證平台探索

首次編譯發現 source registry 仍需 journal_source 借用, 已在原四窗前補回同一 journal 欄位的借用, 未增加來源讀取, 原失敗記錄保留

修正後在 Linux / Rust 1.100.0 / 原 Cargo.lock 執行 filtered `snapshot_` service 測試, 59 筆通過、355 筆未選取, 耗時 6.74 秒, 包含未修改的四窗 logs overview / disabled 與 merge / series oracle, 未執行 HTTP、完整套件或延後測試

此步僅整理既有同步輸入, 跨輪 candidate、mutation revision、JEV / 裝置語意與 staging 容量仍未驗收

### 單一來源 FileSummary candidate 實驗

以 `b3b5f06` 為基準, 僅在既有測試模組加入 candidate 實驗, production 程式碼與原測試 oracle 均未修改

- 使用真實 Collector refresh / checkpoint 儲存與 select_recent_calls, 一個來源保存 600 個互異 call ID, 分別包含近一小時、近一天、近一週與未知時間
- Candidate 持有實際 SelectedCheckpointReader 與四個 owned FileSummary, 每次只讀一頁, 第一頁觀察 512 calls 後仍回傳 pending, 不保留 Projection, 第二頁完成後重查 signature / epoch 才回傳結果
- 四窗 file_activity 與未修改的 materialized checkpoint + FileSummary、實際 Collector::snapshots 序列化結果相符, totals 分別為 150 / 300 / 450 / 600, events、operations 與同時間排序完整比對
- 第一頁後分別注入取消、membership epoch 改變與真實 checkpoint signature 改寫, 均不回傳部分結果且移除 reader / reducer 擁有者, 恢復原 checkpoint 後重建成功, 已完成 owned 結果保持不變

此實驗僅涵蓋單來源無衝突 winner 與 file reducer, 未串接完整 worker candidate / send_replace、跨來源 winner、active mutation revision、history / settings 取消、所有摘要或明細發布, 亦未量測 allocation 或驗證超過 16 MiB 的服務行為, 不把 Option 擁有者釋放當作平台資源量測

Linux / Rust 1.100.0 / 原 Cargo.lock, 新增 targeted 測試通過 1 筆、414 筆未選取, 加入實際 snapshots oracle 後的最終測試耗時 0.52 秒, 未執行 HTTP、完整套件或延後測試

### Candidate encoded payload 實驗未通過

在 `6d4c28d` 後嘗試擴充既有 600 calls fixture, 每筆加入 32 KiB 的額外 JSON 欄位, 目標是比較原 16 MiB staging 與 test-only candidate, 但未取得有效結果

- 第一次失敗為 projection failed, fixture 使用非 JSON signature, 不符合既有 VersionedAnchor 驗證
- 改為保留原 anchor 並增加 revision 後, 第二次仍為 projection failed, 來源檢查確認 Call 使用 deny_unknown_fields, 額外欄位不是可忽略 padding
- 兩次 targeted 測試均失敗, 耗時分別 1.91 / 1.97 秒, 失敗不能證明 production 容量缺陷或 candidate 可完成大型輸入

已停止此檢查並撤回本輪未提交的測試擴充, 保留既有通過的單來源測試與原 runtime, 未放寬 schema、上限或 oracle, 兩份失敗記錄保留, 不自動重跑此大型輸入實驗

後續若恢復此項驗證, 應沿既有 detail fixture 使用合法 JSON 空白擴大 encoded payload, 先確認 fixture 解碼有效再評估 staging, 本輪未執行此替代方案, 未量測 allocation 或宣稱超過 16 MiB 驗收
