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

要核對頁面資產, 先以既有建置工具準備固定版本, 再加上 `--assets src/local_activity_monitor`, 服務會在開始監聽前核對 manifest 與雜湊, 可直接核對靜態回應, 完整快照欄位及設定端點待移植

執行 `cargo test --locked --manifest-path rust/service/Cargo.toml` 驗證 HTTP 行為, 測試使用暫存資料庫與臨時連接埠

## 新共用能力與查詢驗收

八個 API 後端來源分別檢查 on / off、60–86,400 秒間隔、快取、失敗與恢復, 用量頁的 Decisions / 組織費用卡片檢查手動查詢、憑證模式與狀態, 其他入口及自動查詢設定維持隱藏. Dots 只重新讀取所選 home 的有界桌面快取, metadata 停用不讀取, 不接受任意路徑. MCP 本機文件查詢與狀態留在各來源, 內容預設唯讀, 編輯 / 取消 / 儲存維持原有白名單、遮蔽及版本檢查

顯示偏好檢查設定往返 / 還原及舊 pin 能力偵測, 數值格式保留 0 / null 與原始排序值, Tag / badge 外框及分群立即套用. 虛擬捲動核對完整來源、可見 DOM、排序 / 篩選、捲動、停用還原及卸載, 不將分頁與虛擬捲動視為同一方法. 通知核對九方位、三語系 / 窄畫面、歷史上限、換語言保留、Escape 及動態內容清理, 輸入檢查無效 / 重複 binding、文字輸入保護及 destroy

## Cloud 隔離驗收

Cloud 開發只使用獨立的合成 Codex home 與資料庫, 不複製個人 sessions 或登入狀態. 先核對 workbench-ui.json、離線資產及指定來源, 再使用 local-activity-monitor --codex-home <fixture-home> --port 8787 啟動, 不啟用真實 Codex CLI 查詢作為一般健康檢查

Cloud 測試不取代 Windows watcher、真實來源、SSE、Host/Origin 與持久化驗收, 核對受測版本、pin 及實際載入資產, 結果記錄於既有驗收文件
