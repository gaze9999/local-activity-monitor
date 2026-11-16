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
