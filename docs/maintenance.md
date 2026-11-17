# 維護與驗收

依修改範圍選擇檢查, 記錄受測版本、fixture、執行方式與結果. 資料流程沿 source → collector → API → UI 核對, 介面串接使用 `workbench-ui.json` 記錄的 WBUI revision

## 資料收集與保存

- 核對欄位型別、單位、來源時間、統計範圍、去重與讀取上限. 缺值、已確認零值及讀取失敗各自保留狀態
- 對話狀態依 lifecycle 事件, Model / Reasoning 依最新設定事件. Token 使用每個 thread 最新累計值, Skills 依 Thread / Call ID / Skill 去重
- 檢查差異讀取、長紀錄、多檔輪替、部分行、截短、刪除或移動來源, 以及重啟後的 checkpoint 與歷史回補. 持續寫入時仍須有歷史回補進度
- Session 每輪共用 8 MiB, 即時讀取保留四分之一預算給歷史資料, 錯誤回補保留八分之一整輪預算, lifecycle 每檔每輪最多補讀 1 MiB
- SQL、網路與 MCP 活動檢查保存期限、容量上限、快取版本及合併更新. 縮短保留天數移除過期摘要, 欄位較少的新紀錄保留已有的回傳與指標
- MCP 名稱、分類、用途、標籤與指標取自設定及紀錄. 分開統計直接呼叫、exec 靜態辨識位置、來源 telemetry 與外層回傳
- SQL 個別耗時、外層工具耗時、rows_affected 與 rows_returned 分開呈現. 操作明細以已觀察 ID 查詢, 檢查內容遮蔽、分頁與 revision
- 工作樹檢查路徑去重、分支 / HEAD、鎖定及遺失目錄、關聯對話、30 秒快取與讀取預算
- Codex 雲端清單檢查所選 home、自動開關 / 間隔、手動查詢、cursor、metadata 投影、逾時與子程序清理, dots / 排程的 cloud / remote 分類與來源時間分開
- 公開 OpenAI / Codex API 檢查關閉自動查詢不輪詢、手動查詢、固定來源、redirect 拒絕、本文上限、快取、失敗與手動恢復
- 來源紀錄開關使用來源設定, 介面設定匯入與還原保留來源狀態. 官方帳戶查詢檢查快取、逾時、失敗與本機額度 fallback

## 介面與互動

- 八個主頁及所屬子頁依 [使用說明](usage.md)核對. 舊入口、已保存排序與圖表參數保持相容, 來源時間範圍由頁首全域選單控制
- 初次載入、背景更新、空資料、失敗及重試使用共用資料狀態元件. 更新時保留既有內容與高度, 延後圖表完成後更新所屬狀態
- 主頁摘要預設四項, 子頁摘要預設隱藏. 核對項目、數量、名稱、順序、繼承及匯入匯出
- 總覽最多四欄, 其他圖表與資訊卡依數量使用一至三欄. 共用卡片控制器管理布局, 隱藏、顯示與排序後更新, 移除容器或離開頁面時 destroy, 返回快取頁面時重新建立
- WBUI 依容器與卡片用途選擇緊密排列或直向分欄. 核對全寬卡片、瀏覽順序、卡片完整性與間距
- 桌面、820、390 與 320 px 檢查長名稱、大數值、12 - 24 px 字級、字型變更及連續縮放. 表格表頭與本文對齊, 橫向捲動限於表格容器, 窄螢幕資料卡保留欄位名稱
- 圖表檢查刻度、單位間距、圖例、tooltip、標籤傾斜及完整讀值. 平均與 P99 使用有效樣本, 活動趨勢包含零活動樣本, 分布與保存筆數卡依資料意義呈現
- 表格檢查完整篩選結果的排序與分頁、缺值置後、頁碼草稿、欄位順序、收合筆數及數值熱度. 預設每頁 10 筆, 排行前 5 項
- 主頁與子頁切換保留頁面捲動位置. 明細 Tab 切換回到內容頂端, 背景更新保留明細 DOM、展開狀態、捲動位置與編輯草稿
- 明細返回、關聯操作、內容分頁、原文 / 預覽、取消及晚到回應均須驗證. 關閉後中止要求並釋放所屬控制器
- 拖曳預設關閉, 主頁提供開關, 各區排序及鍵盤操作依穩定 ID 保存. 隱藏 / 顯示與排序後核對 DOM 順序與焦點
- 欄位 tooltip 說明資料意義, 一般控制項依入口明確啟用. 檢查邊界定位、modal 開關與焦點返回
- 設定檢查保存、重載、無效值、草稿取消、匯入預覽及還原. Switch 本體與鍵盤切換, 套用全部前確認受影響範圍
- 繁體中文、English 與日本語同步 key、變數與單位, 實際切換各主頁、設定與明細. Log 等級保留來源英文值

## 共用 UI 與按需內容

Workbench UI 負責共用元件、布局與 UIUX, LAM 負責資料投影、欄位意義、tooltip 及保存. 串接前讀取該 revision 的 AI guide、型別規格與使用教學, 共用修正交由 WBUI 維護

- Store 訂閱使用 subscribe(options, listener), 需要 WBUI 0.13.0 以上. 原生及來源封裝使用 `python tools/build_frontend.py --verify-offline`, 核對實際版本、資產 checksum、固定 pin 與頁面 revision. 不相容時保留既有頁面、資產及封裝檔, 來源更新先完成交付及 pin / 資產同步, 再核對封裝
- 離線 manifest 只證明包內一致, 固定 pin 的四份資產另與該 Git 提交的 blob 比較, 避免 checkout 的 CRLF / LF 差異進入封裝, WBUI 0.20.2 起 staging 改讀固定提交, LAM 的 pin 皆晚於該修正, 重建時仍須核對來源位元組, 校正證據見 [驗證紀錄](validation.md#2026-10-24-固定版本資產校正)
- 技能檔案樹使用穩定節點 ID, 檢查展開、鍵盤導覽、載入、返回與關閉清理
- Git、SQL、工具、Log、專案資料夾與指示文件僅在已識別明細中按需讀取, 檢查允許路徑、大小、敏感欄位遮蔽與快取失效
- 結構化內容由 `workbench-ui.json` 固定 WBUI 的 `parseData` 處理, 依實際格式核對結構顯示與原文, 解析失敗保留原文. Markdown 以文字節點及允許連結呈現
- MCP 來源卡檢查用途、狀態、最後更新時間及明細入口. 可編輯資料檔檢查格式、版本衝突、取消與原子保存
- 頁尾來源區塊預設收合, 位置、欄位、健康狀態、上限與回補量取自當次讀取結果. 切換子頁列出相關來源

## 效能與啟動

- 單一收集 worker 分批整理來源, snapshot 讀取使用最近完整結果. 檢查更新中與取消時的 API 可用性
- 圖表接近可視範圍後逐張繪製, 新快照與 Tab 切換取消舊排程. 隱藏卡片、總覽副本與獨立統計卡保留自己的設定
- 同一 fixture、視窗、偏好及 WBUI revision 比較 cold / warm / update / cancel / close 的耗時、程序 CPU / RSS、讀取量、HTTP 大小及瀏覽器指標
- 頁面效能檢查 TTFB、FCP、LCP、CLS、首次資料呈現、長任務與 Event Timing. LCP 使用首次進入背景前樣本, CLS 使用 1 秒間隔 / 5 秒群組, 最慢互動顯示最大 duration
- CLI 檢查已有 Python、缺少 Python、參數、Unicode 路徑、服務重用及停止. WBUI 取得檢查正式 tag、Release 選擇、認證、離線重用、無效資產保留與建置回復
- Watcher 依來源內容與修改穩定時間判斷更新, 相同內容重新存檔不重啟. 建置及來源載入失敗保留現有服務, 替代服務啟動失敗恢復同一舊程序. 檢查修復後更新、切換期間取消及退出清理

## 建置與交付

1. 建置已記錄的 WBUI 與 LAM 前端, 執行受影響 Python / JavaScript 檢查及實際頁面流程
2. 核對程式版本、套件 metadata、`workbench-ui.json`、兩組 manifest 與資產 hash
3. Wheel、sdist、原始碼 zip 與原生 CLI 包檢查頁面、語系、圖示、授權文件及離線啟動
4. 五平台 CI 執行 unittest、原生建置與隔離服務 smoke check, 核對 artifacts 及 checksum
5. 依交付需求建立實際來源的完整 annotated Tag, patch 與 minor 均適用. 手動 workflow 指定既有 Tag, 預設只保存 Actions artifacts, 明確啟用 `publish` 才附加至草稿 Release, 資產齊備並核對後依授權公開

執行方式見 [測試與建置](validation.md), 收集流程與保存上限見 [效能與資料上限](performance-report.md)

## Rust session 掃描器

在原始碼儲存庫根目錄, 使用 Cargo 和啟用 GIL 的 CPython 3.10 以上版本執行 `python tools/build_rust.py` (macOS 可用 `python3`), 下一次從原始碼啟動時會載入建置完成的 Rust 模組, 未建置時使用 Python 掃描器

Rust 逐行產出 Codex session JSONL 的位元組位置及內容, 並解碼即時收集、回補、按需明細與 SQLite checkpoint 讀出的 JSON, token 計數只採用白名單內的非負整數, 保留已確認的零值

遇到超出 Rust 解碼器支援範圍的資料時, 由 CPython 保留原有解析行為, 包含帶 BOM 的位元組輸入及超過 64 位元有號整數範圍的數值

以 `python -c "import sys; sys.path.insert(0, 'src'); from local_activity_monitor.session_scan import forward; print(forward.__module__)"` 核對實際載入的模組, Rust 模式顯示 `local_activity_monitor._session_scan`

## Rust 歷史儲存模組

`rust/history-store` 提供 Rust 程序直接存取 LAM SQLite v4 資料庫的介面, 包含建庫、已投影資料的交易寫入、游標分頁、彙總、清理及 checkpoint 讀寫

在原始碼儲存庫根目錄執行 `cargo test --locked --manifest-path rust/history-store/Cargo.toml`, 測試使用各自的暫存資料庫

Rust 服務在獨立程序串接儲存模組, 歷史查詢使用唯讀連線, session checkpoint 使用交易寫入, 每個程序由各自的 SQLite 實作管理檔案鎖定 [SQLite 檔案鎖定說明](https://www.sqlite.org/howtocorrupt.html#multiple_copies_of_sqlite_linked_into_the_same_application)

Rust 服務啟動時會將已存在的 LAM SQLite v1 至 v3 資料庫升級至 v4, 先驗證 `lam-history.sqlite3.schema-v<N>.bak`, 再以交易套用升級, 若升級失敗, 歷史查詢回傳 503, 原資料庫與已完成的備份保留供重試

## Rust 舊版 JSON 歷史匯入

Rust 服務啟動時會檢查所選目錄的 `monitoring/activity-history.json` 與 `monitoring/error-history.json`, 加上 `--codex` 時也檢查 `monitoring/thread-state.json`

每種資料各自驗證檔案大小、格式及欄位, 套用保留期限後才寫入 SQLite, 已有 metadata 或資料列的類別以 SQLite 為準, 舊檔保留供核對

單一來源無效時會在啟動輸出標示該類別, 其他來源仍可匯入, 修正後可重新啟動再試

## Rust session checkpoint

歷史儲存的 `visit_selected_checkpoint` 可在同一 SQLite 讀取交易內, 按選取順序分批處理合計超過 16 MiB 的 checkpoint 呼叫資料. 每批最多 512 筆, 游標與該批資料合計不超過 16 MiB, 單筆過大仍拒絕; 呼叫端須等整輪成功才發布暫存結果. 服務 detail 另用擁有唯讀 SQLite snapshot 的可恢復 reader, 跨輪只保留 call position / tool 的 compact index, 不保留全部 call payload; 每輪最多 16 頁, 位元組門檻為 16 MiB 加最多一頁有界 lookahead, 完成並每輪最多 64 來源 signature 再驗證後才發布. compact staging 採保守的 16 MiB 記帳及 40,000 來源上限, 超過不發布且維持 overflow details 未就緒, 不是無上限索引; 停用、取消及失敗會釋放讀取交易. header 選取已保存跨輪 top-K heap / cursor, 每次最多掃描 100,000 個 header, 超出後繼續而非丟棄進度; 完整 snapshot 的 16 MiB payload 組裝界線仍待處理. snapshot 的檔案事件已改為精確計數加有界借用預覽, 全域最多 1,000 筆、每個 thread 讀取 / 變更各最多 200 筆, 不再先配置全部檔案事件再截斷; 代理訊息亦先保留每個 thread 最多 100 個借用預覽再產生 metadata JSON; 每個 thread 的 tool / Jev 預覽亦各限 100 個借用參考, 精確計數不再先建立完整 sorted call-reference 陣列; Git / check 每個 window 亦各保留最多 500 個借用預覽, 精確 Git total / operations 不再依賴完整 command-call 排序參考陣列; Skill events 改為 current / history winner 精確計數加最多 500 個借用預覽, 不建立全量 current / merged JSON 陣列, 歷史 winner 索引仍借用現有記錄; 仍保留完整 Call、計數 map、合併的 incoming metadata 與其他事件組裝路徑. 工作目錄查詢已改為逐來源讀取 cursor metadata, 保存跨輪 cursor 與有預算的 staged winners, 不載入 call payload 或累積全部來源投影. 每輪最多 64 個來源, cursor 位元組門檻為 16 MiB 加最多一份有界 lookahead, 保留來源 ROUND_LIMIT / 快取上限 / 既有 metadata 優先順序, 完成及 signature 再驗證後才發布; staged 結果超過 16 MiB 仍不發布, 此容量界線尚未解決

以 `--codex` 啟用收集後, Rust 在所選資料目錄的 `monitoring/lam-history.sqlite3` 儲存最後完整記錄的位置、已選取的對話欄位及工具呼叫摘要, checkpoint 使用 `rust:v1:` 開頭的鍵值, 初次從 Python 切換至 Rust 時會從來源重新收集一次

重啟時核對檔案識別及已讀前綴兩端的摘要, 驗證成功後從已提交的位置續讀, 前綴改寫或檔案更換時重新收集, 未完成的半行在重啟後重新讀取

舊版 Rust checkpoint 會重讀來源以補齊工具錯誤摘要, 來源已不存在時只能查詢先前保存的歷史

`/api/snapshot` 的 `codex.read_state.checkpoint` 提供讀寫狀態與樣本位元組計數, SQLite 忙碌時待下一次來源更新重試, 若回報 `conflict`, 保留另一個程序已提交的位置, 停止重複的收集程序後可重啟續讀

## Rust 服務入口

`python3 tools/build_release.py --rust-preview` 使用本機 Cargo 建置最佳化執行檔, 驗證固定前端資產後組成測試包, 並用隔離資料目錄執行煙霧測試, 通過後印出測試包路徑

從測試包啟動時將 `--assets` 指向包根目錄, 可搭配 `--codex-home <fixture-home> --port 0` 驗證隔離服務, 加上 `--codex` 才會收集, 此模式限本機測試

`tools/smoke_release.py <rust-binary> --rust-assets <asset-root>` 使用隔離資料目錄驗證執行檔及固定資產, Rust 快照的 `monitor.version` 取自 `rust/service/Cargo.toml`, 版本調整時一併核對 lockfile 與 Python 發行版本

未指定 `--codex-home` 時使用 `CODEX_HOME`, 該值未設定或為空時使用使用者目錄下的 `.codex`, 未指定 `--port` 時使用 8787, 指定 `--port 0` 可取得臨時連接埠

指定非零連接埠時, 入口會查詢 loopback 上既有服務的 `/api/instance`, 應用程式及資料目錄識別碼均相符時回傳 `already_running`, 識別不符時由連接埠綁定結果回報失敗

加入 `--open` 時會在新服務啟動或重用既有服務後開啟該服務的 loopback 網址, 優先使用 `BROWSER` 指定的程式, 否則使用平台預設瀏覽器, 暫停後恢復不會再次開啟

加入 `--watch-stdin` 後可逐行傳送 `pause`、`resume` 與 `restart`, 暫停會釋放監聽連接埠並回傳 `paused`, 恢復會重新綁定同一連接埠並回傳 `listening`, `restart` 或輸入結束會關閉程序, 恢復時若連接埠已被占用則回報啟動失敗

開發用啟動器可設定 `LAM_RUST_SERVICE` 為已建置的 Rust 執行檔絕對路徑, 使用既有前端建置產物和替換流程, 未設定時使用 Python 服務

替換程序使用 `--prepare --watch-stdin` 先驗證資產並回傳 `source_ready`, 收到 `start` 後才綁定連接埠及啟動收集, Rust 原始碼變更後須先重新建置執行檔

在原始碼儲存庫根目錄執行下列指令, 將 `<fixture-home>` 換成已存在的隔離資料目錄, 啟動後從 `listening` 訊息取得 URL

```sh
cargo run --locked --manifest-path rust/service/Cargo.toml -- --codex-home "<fixture-home>" --port 0
```

`/api/instance` 回傳所選資料目錄的識別碼, `/api/history?section=web` 唯讀查詢該目錄的 SQLite v4 歷史資料

`POST /api/refresh` 接受同源的空 JSON 物件, 回傳目前的活動狀態, 啟用 Codex 收集時會排入一輪更新, 重複請求會合併等待中的更新, 未啟用時維持停用

加入 `--codex` 後, 服務監看所選目錄下的 `sessions/`, `/api/snapshot?window=all` 提供對話狀態、模型、最新 token 累計值及直接工具呼叫, 時間範圍為 `1h`、`24h`、`7d` 與 `all`, `/api/activity` 回傳目前的收集及監看狀態, 已保存的對話與 Skills 歷史可由 `/api/history` 查詢

`/api/events?window=all` 串流推送同一版快照及記錄, 最多同時接受 8 條串流, 閒置 15 秒會送心跳, `/api/logs?window=all` 回傳 session 錯誤摘要與 Codex Core 記錄摘要

直接 MCP 與 Web 呼叫的摘要出現在快照的 `mcp` 欄位, 並可從 `/api/history?section=mcp` 或 `/api/history?section=web` 查詢已保存的事件, 寫入時沿用 SQLite 的活動保留天數, 與 session checkpoint 一同提交

`functions.exec` 記錄中的 `tools.<name>(...)` 也會辨識, 變數或運算式只保留呼叫名稱, MCP 與 Web 事件分開顯示內層呼叫及外層耗時, 外層回應只有在辨識到單一呼叫時才會投影至該事件

快照的 `codex.git` 與 `codex.checks` 提供直接及巢狀工具呼叫中的 Git 操作及驗證指令分類, 只保留操作名稱、類別或儲存庫名稱, 以及外層呼叫識別碼與耗時, 各顯示至多 500 筆事件

快照的 `codex.sqlite` 提供從 SQLite CLI、MCP 與記錄中的 Python 程式碼辨識的操作摘要, 外層結果與耗時各自標示, `/api/history?section=sql` 查詢已保存的事件

以 `--codex` 啟用後, Rust 也會唯讀查詢所選資料目錄的 `logs_*.sqlite`, 每輪新增與回補各讀取至多 2,000 筆, 警告及錯誤摘要會出現在快照的 `errors` 欄位, SQL 摘要只採用來源實際記錄的耗時與筆數, 缺值維持缺值, 快照及歷史皆不保存原始本文

若 Core 診斷歷史寫入失敗, `/api/logs` 的 Core 來源會顯示 `storage_health: unavailable`, 保留同批待寫入的 SQL 與錯誤摘要, 於下一次來源更新時重試

session 與 Core 的警告及錯誤摘要可由 `/api/history?namespace=errors&section=events` 查詢, 保存天數沿用活動歷史設定, `/api/snapshot` 與 `/api/logs` 會合併即時事件與最近 24 小時的已保存摘要, 歷史預覽最多讀取 1,000 筆及 512 KiB

session 錯誤摘要與 checkpoint 在同一筆交易提交, 若檔案持續變更而無法固定續讀位置, 先保存摘要並保留已提交的位置

工具失敗摘要也寫入相同交易, 多個內層呼叫共用的回覆歸屬外層工具, 未觀察到呼叫的獨立回覆保留來源位移及雜湊

Core 錯誤與同批 SQL 摘要一起保存, 錯誤歷史的讀寫狀態可查看快照的 `errors.history`

以 `--codex` 啟用後, `codex.sqlite.aggregate` 及 `mcp.history_aggregate` 彙總所選時間範圍內的 SQLite 保留資料, 獨立於事件清單上限, 歷史讀取失敗時可由 `history_health` 判斷彙總不可用

要核對頁面資產, 先以既有建置工具準備固定版本, 再加上 `--assets src/local_activity_monitor`, 服務會在開始監聽前核對 manifest 與雜湊, 可直接核對靜態回應, Rust 已有快照及設定端點, 端點存在不代表完整行為或平台驗收已完成, 大型選取快照的 16 MiB payload 組裝界線仍見前述限制

執行 `cargo test --locked --manifest-path rust/service/Cargo.toml` 驗證 HTTP 行為, 測試使用暫存資料庫與臨時連接埠

## 新共用能力與查詢驗收

八個 API 後端來源分別檢查 on / off、60–86,400 秒間隔、快取、失敗與恢復, 用量頁的 Decisions / 組織費用卡片檢查手動查詢、憑證模式與狀態, 其他入口及自動查詢設定維持隱藏. Dots 只重新讀取所選 home 的有界桌面快取, metadata 停用不讀取, 不接受任意路徑. MCP 本機文件查詢與狀態留在各來源, 內容預設唯讀, 編輯 / 取消 / 儲存維持原有白名單、遮蔽及版本檢查

顯示偏好檢查設定往返 / 還原及舊 pin 能力偵測, 數值格式保留 0 / null 與原始排序值, Tag / badge 外框及分群立即套用. 虛擬捲動核對完整來源、可見 DOM、排序 / 篩選、捲動、停用還原及卸載, 不將分頁與虛擬捲動視為同一方法. 通知核對九方位、三語系 / 窄畫面、歷史上限、換語言保留、Escape 及動態內容清理, 輸入檢查無效 / 重複 binding、文字輸入保護及 destroy

## Cloud 隔離驗收

Cloud 開發只使用獨立的合成 Codex home 與資料庫, 不複製個人 sessions 或登入狀態. 先核對 workbench-ui.json、離線資產及指定來源, 再使用 local-activity-monitor --codex-home <fixture-home> --port 8787 啟動, 不啟用真實 Codex CLI 查詢作為一般健康檢查

Cloud 測試不取代 Windows watcher、真實來源、SSE、Host/Origin 與持久化驗收, 核對受測版本、pin 及實際載入資產, 結果記錄於既有驗收文件

## 跨輪快照的實作前置規格

此節起於 `3c1fd9e` 的實作計畫, 下表已依 `7338ce6` 更新目前狀態, 共同發布與單一查詢 paired capture 已實作, 不代表跨輪 candidate、16 MiB staging 或跨平台驗收完成

### 已確認的資料生命週期

| 範圍 | 現行行為與來源 | 對跨輪處理的限制 |
| --- | --- | --- |
| 來源與選取 | `sessions/mod.rs::SourceMembership` 有 identity / revision, SQLite selection_epoch 與完整 CallSelection 共同界定選取 | generation 可在來源未變更的重掃增加, 不可單憑 generation 取消所有進度 |
| Payload staging | `selected_overflow_projections_with_limits` 分頁保存完整 Projection, 最後重查各來源 signature, 成功後移入 Arc, 再驗證成功共用 allocation | 現有 charge 不是完整 allocation 計量, 不能只改成較小 page 就宣稱移除 staging |
| 目前發布 | worker 依序整理歷史、建立完整 Snapshots, 再 send_replace | 單一 worker 的同步步驟不等於未來跨輪快照有完整版本保證 |
| 明細發布 | Snapshots 擁有 immutable detail index, 同一次 send_replace 發布, 8 個 reader 擷取同批內容 | 共同發布不等於組裝期間所有輸入同時一致, 外部來源仍須驗證 |
| 時間與外部結果 | snapshots 固定 reference / cutoffs, 在四 window 前擷取 account / cloud / public / platform 結果及多數設定 | 單一來源 settings/result 已共用讀鎖, 發布 version 仍不是 8 個來源與其他輸入的共同版本 token |
| 裝置與即時資訊 | fonts / gpus / device_completion 目前在每個 window 內讀取, render 另有即時 monitor 欄位 | 必須先區分應固定於整批的資料與刻意即時更新的欄位, 不直接凍結所有 telemetry |
| 歷史與 active payload | worker 會更新歷史及記憶體 Projection, checkpoint_dirty 僅為布林值 | 同 call ID / timestamp 的 response、error、context 更新仍可能改變輸出, 不可以選取 key 相同視為資料相同 |

### 建議次序與驗收門檻

| 次序 | 有界工作範圍 | 必須保留或新增的證據 |
| --- | --- | --- |
| 1 | 在現有同步流程盤點每個 snapshot 輸入的 owner、讀取點與更新點, 將既有四 window 前的局部擷取整理成私有 capture 介面, 不跨輪、不改 HTTP schema | 固定 reference 的 materialized oracle 對四 window、logs、activity、errors 逐項比對, duplicate / tie / null / disabled / numeric 邊界不變, 不把即時 monitor 欄位混入靜態相等測試 |
| 2 | 依 capture 實際使用範圍補齊 mutation revision, 先保持同步發布 | 對同 ID / timestamp 的回覆、錯誤、context 及刪除/重建來源驗證 revision 改變, 無效或未改變寫入不得假裝資料更新, 每個變更點須有實際 producer 測試 |
| 3 | 決定 candidate 的一致性 token 與取消條件, 再接入已存在的 winner plan | 選取改變、來源 signature 改變、設定/歷史改變、停止/停用時丟棄整個 candidate, 不沿用部分 reducer, 未變更來源的 discovery 重掃仍可完成 |
| 4 | 一次接入一種 owned reducer, 與未修改的 materialized serializer 比對 | winner 必須先定案且每次只觀察一次, 保留 canonical tie、distinct-thread、精確 p99、history identity 與排序, 超出暫存預算不得發布部分統計 |
| 5 | 盤點所有 server detail readers 後決定共同發布介面, 再把四 window、logs 與 detail metadata 綁定同一個已驗證 candidate | 測試在每個分頁/驗證/發布交界注入變更, 查詢只取得可識別的舊版或新版, 不把混合資料標為完整, 錯誤與取消時上個可用版本仍保留 |
| 6 | 接入大資料的實際 worker / HTTP 驗證 | 超過現有 payload staging 預算仍完成全量選取, 不降低 count、預覽、保存或精確統計規格, 量測 staging、reducer state、最終四 window 與程序記憶體分開, 同時保留原 5 秒 settings 測試 |

第一步只能整理已存在的讀取邊界, 若需要改變裝置讀取時機、settings/result 的共同版本或 detail 發布方式, 必須先完成對應來源與呼叫者稽核, 不把介面抽取當作行為變更授權

### 現有回歸與新增測試的分界

保留 `snapshot_payload_pages_coalesce_metadata_and_reject_stale_or_cancelled_stage`、`snapshot_payload_scan_bounds_source_and_validation_rounds`、`recent_call_selection_resumes_past_100000_tied_headers` 及 `project.rs` 的兩個 snapshot winner 規格測試, 不重寫 oracle 或以新實作自我比對

現有測試證明選取/來源 signature 的局部生命週期, 尚未涵蓋所有 producer revision、跨輪 reference/cutoff、歷史/設定一致性及同一發布版本的 detail 查詢, 新測試應落在真實 mutation / publication 呼叫點, 不新增只有假想 stamp 比較的測試

上述新增驗收均為待執行, 不能以現有 service / HTTP 通過數或單次 selector 微型量測代替

### 查詢 producer 的現有 version 邊界

依 `b7d9b88` 的 `account.rs::Shared` 與各查詢 caller 稽核, account、cloud、public、platform 使用同一種 Shared, worker 比較 version 來決定是否重建快照

- configure 僅在 Settings 改變時 signal, 相同設定與單純讀取不增加 version
- publish 每次寫入 result、更新 completed 後都 signal, 相同 cached result 也增加 version, 此值代表查詢/設定事件, 不是僅在內容改變時增加的 revision
- account、public、platform 的快取命中會 publish, cloud 僅首頁 cursor=None 會 publish, 分頁結果不覆寫首頁的 shared result
- signal 使用非阻塞 wake try_send, queue 已滿不阻止 version 增加, worker 仍以比較 version 偵測事件
- settings、result、completed 各自上鎖, 寫入釋放鎖後才增加 version, 現有 before/after version 相同不能作為共同 atomic capture 的證據, 這是來源碼生命週期限制, 未以此宣稱已觀察到錯誤快照

後續一致性設計不可直接重用此 counter 當內容 revision, 也不可為了取消較少 candidate 而移除 cached publish 的 completion / wake 行為, 必須另外決定與測試 settings/result 的完整 mutation 邊界, 此輪保留全部 runtime 行為

### Active / history mutation producer 稽核

依 `4caa994` 的同步流程盤點, 下列為已讀取的實際修改點與驗收缺口, 尚未加入新 revision

| Producer / reader | 目前行為 | 跨輪一致性要求 |
| --- | --- | --- |
| `Session::read` / `capture_anchor` | restore 可替換整個 Projection, identity / 縮短 / prefix 或同長度修改會 reset, 讀取完整行後 consume, capture_anchor 設定 checkpoint_dirty | 讀取回傳 changed 也涵蓋原始位元組與 reset, 不能直接當語意內容 revision, 未完成行與 malformed 仍須保留 cursor 狀態 |
| `Projection::consume` / `Call::complete` | 同 call ID 的 output 可改 completed、failure、檔案位元組、SQL / MCP 結果與 response locator | call ID、call time、數量皆相同仍可能改變統計與明細, consume 的 bool 是解析結果, 不是 mutation 判斷 |
| `Projection::context` | when >= context_at 可替換 model / effort / execution, 相同 timestamp 可接受後到資料 | 不可只依 updated 的最大時間戳決定 candidate 有效性 |
| `retire_call` / `Collector::bound_errors` | 修剪呼叫、保留待寫 observations、限制錯誤 preview, 可在沒有新 timestamp 時改變集合 | revision 需涵蓋 retention 與工作集調整, 保留既有未儲存資料保護 |
| `Checkpoints::save` | dirty / retired 且有 anchor 才產生 VersionedAnchor revision+1, 成功且非 conflict 才更新 signature 並清除 dirty / pending | checkpoint signature 是已提交來源的版本, 不涵蓋尚未成功保存的 active 修改, checkpoint_dirty 布林值不能區分兩次修改 |
| `Store::commit_checkpoints` | Immediate transaction 檢查 expected signature, conflict source 不寫入, calls / cursor 與 observation batch 依既有 transaction 保存 | 保留 conflict / rollback 語意, 不得在失敗後替 candidate 標記成已提交 |
| `refresh_activity` / `refresh_error_history` / `refresh_skill_history` | 從持久層更新快照使用的 retained view / error / skill 資料, error 讀取失敗保留舊值並設定讀取錯誤 | history 與健康狀態亦屬輸入, 不能只追蹤 working session 變更 |
| `select_recent_calls` | active 部分 hash 包含路徑、call 數量、ID 與 call time | 足以描述選取用的 key, 不是整個 payload 的內容 hash |
| `selected_detail_index_with_limit` | 另含 anchor / checkpoint signature、request / response position、workdir metadata 等 | 此明細 staging 的 token 與選取 hash 不同, 不可視為共同 windows / detail 發布版本 |

最小後續設計應先區分 active 尚未保存變更、已提交 checkpoint、retained history 與設定/查詢事件四種來源, 再選擇共同 capture / mutation 邊界, 不是在任一現有 bool 或 counter 外加一層名稱即可完成

已新增 producer 測試驗證同 call key / timestamp 的回覆錯誤及 context 修改, 並確認有效但無 payload 的行可回傳 true 而不修改 Projection, 此證據不代表 source replacement、transaction conflict、history refresh 或跨輪取消的完整驗收, 這些仍需依原有專項測試與後續共同發布測試分別確認

### 共同 capture / mutation 邊界設計草案

此設計起於 `2427f5e` 的 producer 與 server reader 稽核, 共同 publication 部分已實作並通過限定範圍驗證, 下列跨輪 capture / mutation 部分仍為待整合規格, 不視為全部架構驗收

優先採用既有 Snapshots 作為共同發布擁有者, 保存該批 immutable detail index 的 Arc, 不新增另一套全域 counter 或第二個發布 channel

1. Worker 同步段完成 detail_index 與 overflow readiness 後建立同一批 Snapshots, index 在 send_replace 前不得先寫入獨立公開位置, 無 detail 變更的批次重用前一個 immutable index
2. Server 每次 detail request 從一次 watch borrow 取得所需摘要/事件及同批 index, 複製或 clone Arc 後釋放 borrow, 不持有 watch / index 寫鎖進行檔案或 SQLite I/O
3. 保留現有 live disable、metadata / mask、來源 identity、locator hash 與 CheckpointGuard, 共同發布不能讓已被修改/刪除的實體來源通過檢查, 亦不承諾跨兩個 HTTP request 保留舊來源內容
4. Disabled 啟動與執行中停用均需覆蓋, 舊 index 不得藉由共同擁有者繞過停用 guard, version 維持既有 snapshot 發布次序用途

| 已盤點的 detail reader | 目前讀取組合 | 串接時的要求 |
| --- | --- | --- |
| `/api/codex/sql` | all-window SQLite event + detail candidates, diagnostics 另有既有來源 | 只合併 snapshot-backed 選取, 保留 diagnostics 來源驗證 |
| `/api/codex/error` | error_details + detail candidates | 同批事件與 locator, 保留歷史/即時 winner 差異 |
| `/api/codex/git`、`/api/codex/check`、`/api/codex/skill`、`/api/codex/jev` | window/history 選取 + detail candidates | 不更改 fallback、限制、thread / call key 或敏感內容遮罩 |
| `/api/codex/mcp` | 先讀 candidates, 再讀 all-window event | 改為一次共同發布讀取, 不能只調換兩次獨立讀取的先後 |
| `/api/codex/tool`、`/api/codex/context`、`/api/codex/agent-message` | detail candidates 與各自的選取/權限 guard | 保留每個 endpoint 既有允許範圍, 不新增隱含資料查詢 |

此步只解決 publication 邊界, 尚不代表跨輪組裝的輸入一致性或移除 16 MiB staging

跨輪 candidate 的建議邊界如下, 不提前建立未被 candidate 使用的 revision

- active 與 retained history 由 worker 擁有, 在一個同步 capture 點建立 candidate 的 immutable 輸入或實際 producer revision, 不能單用 call time / checkpoint_dirty / 已提交 signature 代替尚未保存的 payload
- 查詢 settings/result 必須在共同狀態鎖內擷取或具備涵蓋每個寫入起訖的協定, 現有 signal counter 不符合此條件, scheduler 的事件與 completed 行為須原樣保留
- 設定 capture 尚未提供共同 revision, 需對照真實設定 setter/持久化完成點決定何時取消 candidate, 不在失敗寫入後假裝採用了新設定
- overflow 每頁保持已選取 keys 與 signature 驗證, retained history 必須來自同批已確定的 view, 外部 SQLite 寫入與來源 replacement 不能用單一程序記憶體 counter 偵測
- reference / cutoffs 固定於 candidate, discovery generation 在無資料變更時增加不能取消進度, 持續來源變更下的完成/取消策略仍須以實際大資料測試確認

最小整合驗收: 在既有實際 worker 與 route 讀取點控制兩次發布, 證明 request 取得同批 event/index, 在來源已改變時仍拒絕過期 locator, 停用後仍拒絕內容, 舊可用快照在候選失敗時保留, 再跑完整 service / serial HTTP, 不用只比較假想 stamp 的測試代替

### 共同發布的已實作範圍

後續實作已將 detail index 移入 Snapshots 的 immutable Arc, 取代獨立 Monitor / server State RwLock, worker 在同一個 send_replace 發布 windows、logs、error_details 與 index, 沒有 index 變更時重用現有 Arc

前述 8 個 reader 均先擷取同批 all-window、error_details Arc 與 index Arc, 釋放 watch borrow 後才執行既有查詢與來源 I/O, error map 不在每次 request 完整複製, live disable、mask、locator 與 checkpoint guard 保留

這取代前文描述的獨立 detail_writer 發布方式, 但跨輪 capture / mutation revision、候選取消及 16 MiB staging 仍未完成, 同一發布物件不保證所有外部輸入在同一時刻產生, 也不保證跨 request 或實體來源變更後仍能讀取舊明細

### 單一查詢來源的共同讀取邊界

account::Shared 的 settings 與 retained result 已合併到同一個 RwLock<QueryState>, capture 在一次 read lock 內複製 Settings 與 result, SnapshotQueries 的 8 個來源各自使用這個 paired capture

原 settings / result getter、configure 的 no-op / force_due、publish 的 completed 與每次 signal 行為保留, cached 相同結果仍通知, event version 不改作內容 revision, 不新增外部查詢

相較最初的擷取順序, public / platform 現在各來源 settings/result 成對讀取, 不再先讀完整 settings array 再讀 results array, 不增加 result clone 次數, 不持有多來源鎖

此步使單一來源的當前設定與保留結果具有共同讀取點, 但設定變更不會清除既有結果, 查詢仍可能在先前設定下啟動, 因此不代表 result 的設定 provenance、completed 的原子讀取、8 個來源共同時點或跨輪版本 token, 更不能直接將 signal counter 當 candidate revision

### 已驗證 payload stage 的共用擁有權

selected_overflow_projections 在來源 signature 全部驗證成功後, 將 mutable Projection vector 移入 immutable Arc, 後續成功再驗證只 clone Arc, snapshots 借用同一個 slice 進行既有合併, 不再為每次回傳深複製整個 staged payload

正在讀頁或驗證的 candidate 仍不能回傳, signature 不符、儲存讀取失敗、選取改變、停用或取消仍清除 cache 擁有權, 已交付的舊 Arc 不被改寫, 下一次新 candidate 仍獨立重建

原 16 MiB charge、每輪頁數與 signature 批次、source limit、保留策略及精確統計皆未調整, staging 仍保存完整 payload, 合併 thread 與最後四個 window 仍可能另有配置, 不將移除一次 deep clone 描述成已解決 staging 容量或程序記憶體上限

### Snapshot 輸入 owner 與剩餘讀取邊界

依 `7338ce6` 的 snapshots、error_windows、render 與直接 helper 原始碼稽核, 下表區分已擷取狀態、每個 window 的額外讀取及刻意即時資料, 不把直接 self 欄位清單當作完整一致性 token

| 輸入家族 | 目前 owner / 讀取點 | 跨輪處理前的要求 |
| --- | --- | --- |
| active / overflow / selected calls | worker 的 files / membership, checkpoint reader 與已驗證 Arc stage, snapshots 內合併 threads | 保留來源與 winner 順序、未保存 payload、選取與 signature 邊界, 不以同 call key 判定內容相同 |
| retained activity / error / skill / API events | worker 的 activity_view/history、error_history、skill_history、api_events、core / desktop / JEV error 集合 | candidate 固定本批使用的 view 與健康狀態, 外部持久層更新不由記憶體 counter 自動涵蓋 |
| thread / project / schedule / plugin / worktree / dots metadata | worker 更新的 thread_metadata、project_details、schedules、plugins、worktrees、dots 與相應 reports | 不只 capture calls, 相同 call 集合下 metadata 與狀態仍會改變輸出 |
| tracking / MCP source / customization / feature flags | SnapshotSourceSettings、applied flags 與 tool/files atomics | 目前只有部分同步擷取, 尚未提供共同設定版本, error_windows 還會另讀 mcp_sources |
| account / cloud / public / platform | SnapshotQueries 在四 window 前取得 8 個獨立 paired capture | 不宣稱 8 個來源共同時點或結果對目前設定的 provenance |
| journal / debug | snapshots 開頭同一 journal lock 擷取 events/entries/source, error_windows 與 logs overview 共用該 owned capture, debug 仍另外擷取 | journal 三個視圖共用單次讀取邊界, 不代表與 debug、其他來源或跨輪 candidate 具有共同版本 |
| JEV telemetry | 每個 window 呼叫 jev::snapshot, 重新讀 config, active 路徑開唯讀 SQLite 並查 summary/recent/series | 這是實際 I/O, 不是 worker 已保存的 Value, 後續須設計同批來源快照與設定/DB identity 驗證, 保留唯讀、安全旗標及錯誤行為 |
| credential availability | 每個 window 呼叫 platform::credentials 讀環境變數並只輸出可用狀態 | 不新增憑證輸出或查詢, 若要固定整批讀取點須單獨驗證現有結果語意 |
| fonts / GPUs / device completion | 每個 window 取得共用讀鎖與 ready_at | 保留初始化完成語意, 不與 render 的即時更新混為同一種靜態輸入 |
| render telemetry | Snapshots::render 依 uptime、traffic revision、collection revision 更新 cache, 並取 memory / process usage | 仍屬刻意即時資料, 不放進靜態 payload 相等 oracle 或藉此取消所有 candidate |
| source registry / worktree snapshot | source_registry::augment 整理已載入的參數與 snapshot, worktrees::snapshot 複製既有 result/items | 此兩者不是新增檔案 I/O, 應連同其真正上游狀態驗證, 不重複發明來源 revision |

下一個跨輪整合步驟仍須處理上述實際讀取邊界, 尤其 JEV 的多次讀取, 不能只為 active calls 加 counter 就宣稱完整 snapshot 一致, owner 稽核後已將 journal 合併為開頭一次擷取, 未變更 JEV、credentials 或 render 的讀取時機與查詢次數
