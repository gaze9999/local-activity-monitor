# 程式架構與擴充

## 執行環境

Python 3.10+ 標準函式庫, 原生 HTML / JavaScript / CSS, setuptools package assets. 服務綁定 loopback, `Host` / `Origin` / `Sec-Fetch-Site` 驗證保留. 來源資料庫採唯讀連線, LAM 的回補資料庫使用交易寫入

| 檔案 | 責任 |
| --- | --- |
| 根目錄平台入口 / `tools/launch-cli.py` | 使用既有 Python 直接啟動原始碼, 預設不安裝 |
| `tools/watch.py` | Python / frontend 修改穩定後建置頁面, 成功時重啟自己建立的服務子程序 |
| `server.py` | loopback HTTP, snapshot cache, 設定驗證與 collector 協調 |
| `collectors.py` | Jev metadata DB, Codex JSONL 差異讀取, 時間 / counters / 工具摘要 |
| `codex_metadata.py` | 選取 catalog, session index 與 app state 中的名稱 / 分類 / 專案 |
| `operation_records.py` | 只解析 literal 呼叫, Git / Skill / 驗證 / 檔案操作 |
| `agent_messages.py` | 代理通訊 metadata 與按需傳送參數投影, 不將訊息或 Context 本文加入一般快照 |
| `payload_detail.py` | 按請求設定內容遮蔽、結構投影與分頁版本, Context 只選公開可見內容 |
| `mcp_records.py` | 來源發現, 通用 MCP metadata, 已知欄位摘要, Web 參考 URL |
| `monitor_state.py` | 有限容量的效能樣本, 程序資訊與狀態事件 |
| `error_records.py` | 唯讀 App / Core Log, 錯誤與 SQL 診斷 metadata, 分批歷史回補 |
| `error_history.py` | 最近 24 小時記憶體摘要, 歷史 metadata 另存 SQLite |
| `thread_state.py` | 已確認 lifecycle 與 Skills 讀取 checkpoint, 原子保存與去重 |
| `history_store.py` | SQLite 回補資料、既有 JSON 匯入、版本遷移及備份 |
| `process_lifecycle.py` | CLI 終止訊號與 Windows 子程序生命週期 |
| `sqlite_records.py` | literal SQLite 操作辨識, 不執行 SQL 或來源程式碼 |
| `usage_records.py` | 額度視窗與 credits 白名單投影 |
| `codex_account.py` | 選配的官方帳戶唯讀查詢, 有期限快取與獨立失敗處理 |
| `codex_plugins.py` | 選取 Plugins 設定與快取版本 metadata, 不執行外掛 |
| `frontend/locales.json` | English / 日本語介面與 tooltip |
| `frontend/` | LAM 頁面來源, tab, 表格 / popup, lazy loading, 圖表, 外觀與欄位投影 |
| `tools/build_frontend.py` | WBUI tag / Release 解析, 資產準備, LAM 頁面建置與失敗復原 |
| `frontend_assets.py` | 驗證產出的 _web/ manifest、檔案白名單與 SHA-256 |

## 來源發現

從當前 `CODEX_HOME/config.toml` 選取 MCP section 名稱與 enabled 狀態, 不回傳 command, env 或 credentials. description 與已設定 README 提供用途, README 最多讀取 64 KiB, 用途快取 60 秒. Python 3.11+ 使用 `tomllib`, Python 3.10 使用只辨識 section / enabled / description 的相容解析, 跳過 multiline 字串

Session 中出現 `mcp__server__tool` 時自動建立來源. 已啟用的 Codex App / Review / Security / CUA plugins 也會提供對應來源. 來源來自設定或既有紀錄, 不執行背景 API 探測. 缺少來源的電腦不會顯示其項目, 原有瀏覽器偏好也不會憑空建立來源

Plugins 清單另選取設定與快取 manifest 的名稱, 供應商, 版本, 啟用狀態與技能 / MCP 數量, 設定啟用狀態與快取分列. 子代理程式透過唯讀 thread_spawn_edges 補齊關聯 metadata, 最多 500 個相關對話, 每份狀態回查最多 64 KiB 並共用每輪 8 MiB 預算

官方帳戶來源預設關閉, 啟用時建立自己的 Codex app-server stdio 子程序, 只發送 initialize / initialized 與三個 account 唯讀方法, 結束後釋放程序及串流. 使用既有登入, 不讀取 auth.json, 不將 email, 帳戶 ID 或 credentials 投影至頁面. 成功與失敗均快取至少 60 秒, 整次逾時 10 秒, stdout 上限 1 MiB. rateLimitsByLimitId 優先於 legacy rateLimits, primary / secondary 可為 null, 視窗名稱依 windowDurationMins, 不假設 primary 為短期額度. account/usage/read 的帳戶統計與本機對話累計分開

Work、My dots 與排程使用本機來源, 雲端清單尚未串接

已知來源使用分類預設, 新來源使用通用分類. 來源分類與工具說明可以在頁面修改. 呼叫若將 namespace 與 name 分開儲存, 會合併完整識別名稱. 不同 MCP 的同名工具分開統計. 新工具沒有特定 adapter 時仍顯示名稱, 操作, 時間, 資源 ID 及可辨識的結果

## 輸入 / 回傳資料

代理訊息依已讀取的工具呼叫或已記錄訊息標頭投影 metadata, 不保存訊息本文. 傳送明細只讀取已觀察 Thread / Call / index 對應的參數及外層工具回覆, 呼叫辨識與實際送達分開. 沒有 Call ID 的接收訊息只連到該對話近期 Context, 不宣稱取得單筆傳送內容

Context 按需選取最多 1 MiB 的來源片段與最近 32 則可見訊息, 指定 Call ID 時範圍止於該呼叫之前. `visible_context_record()` 只選 user / assistant 訊息及明確的公開思考摘要, 排除 analysis、完整或加密推理. 回補未完成與未記錄分別保留 `pending` 與 `not_recorded`, 不重建完整模型 Context

`payload_masking` 使用預設 true 的 ContextVar, HTTP 明細請求以 `mask_payloads()` 套用 `mask=0|1`, 結束時重設, 不改變一般 snapshot 的敏感資料投影. 前端 contentMasking 與舊 sqlMasking 相容同步, 切換時清除明細快取. 分頁 revision 含遮蔽模式, 不混接不同模式的資料. 文件、專案與來源檔案的原有選取及編輯規則不受此開關改變

Snapshot 不包含 prompt, 完整命令, 任意 MCP input / output 或文件本文. MCP request 只保留白名單 metadata, 例如檔案副檔名, OCR 模式, 寫入開關與資源 ID. response 只保留狀態, 數值 / boolean 與明確的數量摘要

已保存操作的 MCP / SQL 明細可按需透過來源 offset 讀取, 舊操作索引依每輪讀取預算向前回補, 索引不保存本文. 巢狀呼叫含動態參數時保留遮蔽後的記錄程式碼, 不執行 expression. 多個工具共用的回傳標示為外層工具回覆, 不分配成個別工具結果. 解析記錄中的 Python 不將其字串跳脫警告輸出為 LAM 程式警告

對話的 Model 與 Reasoning 等級依最新 turn context 或 thread_settings_applied 時間合併, 多份相同 thread 的 session 不以工具紀錄時間覆蓋 Model 設定. `reasoning_effort` 缺少時可由本機 catalog 補齊. `execution` 只保存 Provider, CLI 版本, context window, 審核 / Sandbox / 協作模式, Agent 資訊, 上層 Thread ID 與 Git 分支 / commit, 服務等級, Reasoning 摘要與審核來源. Sandbox JSON 僅取 mode, 不回傳 writable roots. 新的 reasoning 名稱依有效字串保留, 前端動態建立篩選選項

回傳可為 JSON 字串, `structuredContent` 或單一 `content[].text` JSON. 其他格式仍保留工具紀錄. 欄位新增, 缺漏或型別不符不使來源清單消失. 新增特定摘要時在 `request_metadata` / `response_metadata` 加入型別檢查與白名單欄位, 並用 fixture 覆蓋既有與缺漏格式

| 分類 | 目前摘要 |
| --- | --- |
| 文件 / OCR | 副檔名, OCR 模式 / 結果 / 項目 / 省略 / 錯誤, 文字長度 / 截斷, 預覽 / 寫入 |
| 環境 / 驗證 | 差異 / 單邊檔案數, 驗證 run 數與 passed / failed / skipped |
| Review / Security | 狀態, findings 與 severity 數量 |
| 工作流程 / 部署 / App | 工具名稱, 操作類型, 結果與選定資源 ID |
| Runtime / 瀏覽器 | 工具名稱, reset / 操作類型, 結果, 時間與耗時 |
| 通用 | 個別呼叫 / exec 辨識, 已回傳數, 已提供結果 / 錯誤數, 平均 / P99 耗時, 可取得的 retries / tokens |

`functions.exec` 內的工具按靜態呼叫位置計數. 個別工具耗時維持 null, 另保留整次 exec 耗時. 多個工具共用的回傳不分配給其中任一工具. 單一呼叫才解析獨立摘要. 分類包含依名稱推斷的讀取 / 寫入 / 部署, 原始工具名稱一直保留

Skill 檔案依 session 中已觀察的 literal SKILL.md 路徑取得. Endpoint 接受 Skill 名稱與可選的目錄內相對文件名稱. 相對名稱必須存在於當次列出的可讀文件中, 讀取前再確認解析位置仍在該 Skill 目錄內. 排除 UNC, symlink, 隱藏目錄, credential 檔與快取目錄. 每次最多列出 200 個檔案, 掃描 1000 個項目與 4 層子目錄. Python 與其他非文件檔只取得 metadata, 不讀取原始碼. Markdown / TXT / RST / AsciiDoc 每份最多讀取 256 KiB, 遮蔽 credentials 後以每頁 32768 字元顯示, hash 對應讀取片段. 一般 refresh 不掃描 Skill 目錄

Jev payload 仍只在指定呼叫點開時讀取, 遮蔽 credentials 並保留既有混合回傳隔離. Web 只保存 URL, query / fragment 不保留. 檔案操作保存 patch header, shell 明確讀寫位置, 或工具名稱與 path 參數辨識的檔案位置 / workdir / 工具識別, 不取出命令本文或檔案內文

## 範圍與效能

- 初次每個 session 讀取頭行與最多 1 MiB 尾端, 後續差異讀取. 缺少 lifecycle 時優先反向回查工作事件, 再回補最近 24 小時錯誤. 全部 session 共用每輪 8 MiB
- 檔案清單每 15 秒盤點全部 session, 不再依近期數量篩選. 差異讀取以輪替游標與每輪位元組 / 檔案檢查預算避免單一大檔占滿額度, 未讀完的來源持續回補
- 每個檔案保留最多 8192 個工具 Call ID, 全部檔案合計最多 50000, 超過移除最舊資料. 去除同 ID 的重複呼叫
- 未完成行每檔最多 1 MiB, 合計 8 MiB, 超過時丟棄該片段至下個換行. 額外 session error 每檔 50 筆, 全部最多 1000
- 工具趨勢保留最近 10080 個分鐘 bucket, 工具 / 時間排行最多 20000 個 bucket
- 對話工具與 Jev 明細各 100 筆, 檔案修改與讀取各 200 筆, 全部檔案操作列表最多 1000 筆. Git / Skills / 驗證列表最多 500 筆, MCP 列表最多 1000 筆
- Catalog 最多 2000 個對話 metadata, Jev 趨勢最多 168 個小時 bucket
- 對話全部顯示每批建立 50 列 DOM, 全部新表格預設每頁 10 筆, 排行 5 項, 顯示選項預設 5 / 10 / 20, 可自訂最多 8 個 1 - 200 選項. 圖表最多 240 格

SQLite 的 threads 分區持續保存已確認 lifecycle checkpoint 與 Skills 選定 metadata, 不依筆數或容量刪除歷史. 記憶體工作集合仍最多 1000 個 lifecycle checkpoint、500 筆 Skills 及合計 512 KiB. 保存狀態時間, 開始時間, Thread ID, 檔名與讀取 offset. Skills 保存 Thread / Call ID, Skill 與時間. lifecycle / Skills 改變時保存, offset 最多每 30 秒更新. 交易保存失敗時保留記憶體觀察

重啟後若初始尾端涵蓋 checkpoint 至最新位置, 可沿用確認狀態. 未涵蓋的離線區間繼續反向回補, 顯示快取來源與確認時間. 新事件優先覆蓋舊狀態. Skill count 依已保留的 500 筆 metadata 計算, 以 Thread / Call ID / Skill 去重

關閉觀察項目會停止對應解析或讀取. 來源開關會略過其摘要並隱藏事件, 開啟後重新解析 session 尾端. 來源名稱仍可供使用者重新開啟. 部分紀錄被截短或起訖缺漏時保持 null

AGENTS.md 的最後修改時間取自實際開啟的檔案, 技能文件沿用既有 metadata 時間. 一般檔案時間只在開啟明細時 stat 已觀察路徑, 拒絕 UNC, symlink 與 credential 檔案, 不加入 snapshot 或 checkpoint

右上 LAM 狀態依 snapshot 請求成敗顯示. Codex 狀態從既有對話用量回報及連線診斷 metadata 投影, 最近 5 分鐘顯示最近有回應或連線錯誤, 缺少近期資料時顯示待確認, 停用 Codex 觀察時顯示觀察停用. tooltip 提供依據及觀測時間, 不呼叫遠端探測 API

## HTTP 與設定

共用 UI 由 `ui_assets.py` 串接 Workbench UI 的標準函式庫載入器, 預設使用建置產生的 `_workbench/`, 可用 `WORKBENCH_UI_PATH` 明確指定開發來源. LAM 頁面來源在根目錄 `frontend/`, 建置工具驗證六個來源檔案並產生 `_web/` 與 SHA-256 manifest, 後端不再讀取來源目錄. 靜態路由維持固定白名單, 首頁合併 JS / CSS 並計算 CSP hash, 版本識別涵蓋實際檔案的 mtime、大小與內容 hash

CLI 啟動時檢查最高的 0.x 正式版本 tag, WBUI 1.0.0 起以既有 GitHub CLI 認證取得 Latest 的已發布非預覽 Release, 使用 Git 的 peeled SHA 解析 annotated tag, 透過既有認證取得該提交並呼叫共用 helper. 候選版本低於目前已建置版本時沿用原版, `--revision` 可明確選擇已確認的完整 SHA. 新的 `_workbench/` 與 `_web/` 都通過驗證後才替換資產並更新 `workbench-ui.json`, 取得失敗時沿用可用版本, 無效或用途不明的既有目錄保留並回報. 檔案修改後的 watcher 建置不查遠端. CI 與發行工具只使用已提交的完整 SHA, 原生程式、wheel、sdist 與原始碼下載包包含兩組建置資產, 不需相鄰 checkout 或網路. 左上角與 LAM 並列的版本取自 WBUI 公開的 `version` 介面

| Endpoint | 用途 |
| --- | --- |
| `GET /api/snapshot?window=1h\|24h\|7d\|all` | 彙整 snapshot, window 作用於 Jev DB |
| `GET /api/codex/jev?thread=...&call=...&index=...` | 指定呼叫的已遮蔽 Jev 內容 |
| `GET /api/codex/sql?id=...` | 已觀察 SQL 操作的指令內容, 點開明細才讀取並遮蔽憑證 |
| `GET /api/codex/skill?skill=...&file=...` | 已觀察 Skill 的檔案清單與文件 metadata. file 可省略, 指定時只讀取清單內的文字文件 |
| `GET /api/codex/file?thread_id=...&call_id=...&path=...` | 只對已觀察且識別相符的檔案操作按需查詢最後修改時間, 不讀取內容 |
| `GET /api/codex/file-summary?window=...` | 對所選範圍的已取得檔案事件查詢最多 100 個位置的大小與更新時間 |
| `GET /api/codex/tool?thread_id=...&call_id=...` | 已取得工具呼叫的 JS, patch 與輸入 / 輸出明細 |
| `GET /api/codex/mcp?thread_id=...&call_id=...&index=...` | 可獨立識別的 MCP 呼叫輸入 / 輸出 |
| `GET /api/codex/context?thread_id=...&call_id=...&mask=1` | 已載入對話的可見訊息與公開思考摘要, call_id 可省略, 不回傳完整模型 Context |
| `GET /api/codex/agent-message?thread_id=...&call_id=...&index=...&mask=1` | 已觀察代理通訊的傳送參數、外層回覆與可取得的呼叫前 Context |
| `GET /api/mcp/files?server=...` | 來源設定, 套件 metadata 與已取得參數提供的本地文件清單 |
| `GET /api/mcp/files?server=...&document=...` | 清單內檔案的已遮蔽文字, 格式與版本 hash |
| `POST /api/mcp/file` | 白名單資料檔的格式檢查, hash 比對與原子儲存 |
| `GET /api/logs` | 目前保留的來源事件 metadata 與來源健康狀態 |
| `GET /api/instance` | 程式識別與 CODEX_HOME hash, 啟動時重用同一個 monitor |
| `POST /api/settings` | interval, observations, mcp_sources, mcp_categories, tool_descriptions, mcp_descriptions, mcp_tags, recording, replace_customizations. 舊 max_files / track_all 設定仍接受相容格式, 不再限制來源數量 |
| `POST /api/jev/recording` | 經驗證的本機 Jev enabled 開關 |

設定先完整驗證再變更. map 各最多 64 個自訂項目, 工具說明最多 400 字元. `/api/settings` body 上限 256 KiB. 觀察設定只存在目前後端程序, 網頁使用 localStorage 保存並在重新連線後套用. 外觀, 圖表, 表格排序 / 分頁, Tab 順序與介面文案屬於前端. 介面文字由 HTML 純文字與 ui() 標籤集中登錄, 略過數字, 單獨單位與帶有拼接空白的片段. data-copy="ignore" 可排除整個元件. 圖表動態控制項保留原始標籤供即時套用, 不修改原始紀錄值或 ID. 新 UI 元件沿用 ui() 與通用表格控制, 新 Tab 會自動接到目前排序尾端

Snapshot 額外提供 `default_settings` 與讀取器的來源資訊. `sources` 根據當次 collector 結果組合實際位置, health, 選取欄位與上限, 各讀取器保留實際讀取數量與回補狀態. 上限與執行讀取共用 constants, 不額外重掃來源或載入 payload. 頁面依主 / 子 Tab 的資料相依篩選來源, 詳細內容只在展開區塊時建立 DOM. MCP 回傳只投影最多 40 個符合用量, credits, 次數或耗時語意的數值, 排除 credentials 與任意帳戶欄位. Token 用量採 thread 最新累計, 額度保留來源視窗, 剩餘百分比與時間, 不推估帳單金額

Snapshot 的 monitor 欄位提供處理器名稱, 實體與可用記憶體容量, GPU 型號 / 專用容量 / 共享上限 / 驅動版本及 runtime, uptime, refresh / process CPU 耗時, 當次 session bytes, 資料保留量, HTTP 回應大小與錯誤計數. GPU 在首次硬體收集階段查詢, 最多 16 張. Windows 使用 System32 的 DXGI 唯讀 API, 區分專用容量與共享上限, 32 位元程序不提供容量, 不採用只有 uint32 的 WMI AdapterRAM. Linux 使用已安裝的 nvidia-smi 或 DRM sysfs, 型號按需由已安裝的 lspci 補充, 外部查詢合計預算 3 秒, 不安裝工具. macOS 使用 system_profiler 的顯示卡資料, Apple M 系列標示統一記憶體, 不將共享容量當成專用容量. 每個外部指令最多 2 秒, 接受輸出最多 128 KiB. 權限, 工具, schema 或驅動欄位不足時保留未知, 不使用 kernel 或 macOS 版本冒充 GPU 驅動版本. 記憶體容量最多每 5 秒查詢一次, macOS 可用容量目前保持未知. 360 個效能樣本及 200 個狀態事件使用 bounded deque, 重新啟動後清空. 整理失敗保留前次 cache, poll 繼續重試, 紀錄只保存例外類型

錯誤觀察使用平台 log 目錄及 CODEX_HOME 的 logs_*.sqlite, 驗證 schema 後唯讀選取 metadata. Desktop 最多 8 個近期 log, 每份初始 256 KiB 尾端, 差異與歷史回補每輪各最多 1 MiB, 未完成行最多 64 KiB. Core 每輪最多最近 2000 列 ID, 歷史回補每輪最多 2000 列. 有 0.08 秒 SQLite 讀取預算, 投影診斷 / SQL metadata, body 只暫讀前 8192 字元供分類, 不保留. 診斷事件與輸出的錯誤清單各最多 1000 筆, source 缺少 / schema 不符顯示健康狀態. 詳細錯誤判定見 [錯誤觀察](error-observation.md)

表格以原始欄位 ID 對應儲存排序, 顯示與拖曳順序, DOM 位移不改變排序欄位. 原始 cells 使用 WeakMap, popup 關閉清除對應 table view. 桌面多欄位在表格範圍橫向捲動, 760 px 以下改成資料卡. 共用 filter / paginate / column 規則套用新增表格. 熱度使用目前頁面數值逐欄正規化, 預設關閉. 設定匯入 / 匯出 version 1, 原始文案作 key, 格式見 [設定檔格式](settings-format.md)

瀏覽器 document.hidden 時略過自動 HTTP / render, 重新顯示後 refresh. 後端保持收集. Cache 只在整理時建立, HTTP 取值時深複製選定 window 後附上最新程式狀態

HTML 內嵌目前 CSS / script 並提供精確 CSP SHA-256, 換行先統一為 LF 以符合 HTML parser. HTTP/1.1 支援連線重用, 保留 Content-Length, 15 秒 socket timeout, 被拒絕的請求關閉連線. assets / snapshot 使用 no-store, 可接受的 response 以 gzip 壓縮. 前端請求包含 15 秒 timeout, 後端重啟後重新取得資料並套用偏好, 前端資產版本穩定變更後才重新載入, 已開啟明細時延後. watcher 不修改其他服務程序

## 驗證

使用 checkout 的 src 執行隔離 fixture. PowerShell 設定 `$env:PYTHONPATH="src"` 後執行 `python -m unittest discover -s tests -v`. macOS / Linux 使用 `PYTHONPATH=src python -m unittest discover -s tests -v`, 並以 `node --check frontend/app.js` 檢查語法. UI 需另外驗證來源有 / 無, 設定與文件保存, 明細, 分頁 / 分批載入, 圖表設定與淺 / 深色外觀. 原生套件由各目標系統的 GitHub runner 建置並執行隔離 HTTP smoke test

## 原生 CLI 建置

`tools/launch-portable.py` 直接啟動 server, 不使用原始碼 watcher 或安裝流程. `tools/build_release.py` 在各原生系統使用固定版 PyInstaller 建置資料夾套件, 收入完整 _web/ 與 _workbench/ 建置資產, 說明與 runtime 授權文件. `tools/smoke_release.py` 以暫存 CODEX_HOME 與 loopback 隨機 port 啟動該執行檔, 核對 HTTP, CSP, 資產, 版本與任意檔案存取邊界

GPU 容量語意依 [Microsoft DXGI](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/ns-dxgi-dxgi_adapter_desc), [NVIDIA SMI](https://docs.nvidia.com/deploy/nvidia-smi/), [Linux AMDGPU sysfs](https://docs.kernel.org/gpu/amdgpu/driver-misc.html) 與 [Apple silicon 架構](https://developer.apple.com/videos/play/wwdc2020/10686/) 處理
## 活動保留與按需檔案資訊

`activity_history.py` 保存 SQL, 網路與 MCP metadata, 預設 90 天, 可設定 0 - 3650 天, 0 表示不自動刪除. 保存不依筆數或容量刪除歷史, 超過期限的事件每輪最多清理 500 筆, checkpoint 保留供來源狀態回查. 記憶體仍保留 SQL 500 筆、網路與 MCP 各 1000 筆及合計 1 MiB 的工作集合, 這些限制不代表資料庫保存上限. 合併更新時保留已取得的回傳與指標, 快取格式 version 3, 可讀取既有 version 1 / 2, 明確保存的舊期限沿用原設定

快取位於 SQLite 的 activity 分區, 選取操作分類, 計次, 耗時, 公開參考網址與來源提供的數值指標. 輸入, 輸出與 SQL 文字由明細 API 按需取得. 停用對應檢查時停止顯示該資料, 恢復後沿用仍在保存範圍內的摘要

## 回補資料儲存

`history_store.py` 使用 `CODEX_HOME/monitoring/lam-history.sqlite3`, `application_id` 為 LAM 識別碼, `user_version` 目前為 3. `history_state` 保存 activity / errors / threads 分區的格式及設定, `history_items` 逐筆保存資料, 依分區、項目類型與時間建立索引. 各分區按穩定項目識別碼 UPSERT, 未變動的 payload 不重寫, 不再每輪刪除整個分區. SQLite 採 WAL 與短交易, 寫入等待上限 0.2 秒, 讀取連線使用 mode=ro 與 query_only, 讀取不占用收集器的更新鎖

首次載入先檢查 SQLite 分區, 尚無資料時讀取既有 JSON, 經欄位白名單、型別、去重與期限規則投影後匯入. JSON 原檔保留, 已匯入分區以 SQLite 為準. 512 KiB / 1 MiB 限制只套用記憶體工作集合, 不限制 SQLite 歷史保存筆數, 資料庫另包含索引及頁面空間

欄位新增、刪除或轉換在 `MIGRATIONS` 加入下一版的明確步驟並更新 `SCHEMA_VERSION`. 升級前使用 SQLite backup 建立 `.schema-vN.bak`, 驗證備份後在同一交易執行步驟及更新版本. 失敗時回復資料與版本, 保留備份. 不支援的版本或其他程式的資料庫會回報不可用, 保持原檔

CLI 結束保留此資料庫、遷移備份、既有 JSON 與程式 journal. Windows Job Object 的 handle 由程序保有至系統結束程序, 子程序繼承其歸屬. 啟動器與 watcher 另用標準輸入控制正常退出, SIGTERM / SIGHUP / SIGBREAK 進入既有清理流程

明確指定 WBUI revision 的建置先驗證目前資產, 再以共用 helper 準備候選版本, 成功後更新 pin. `--latest` 比較已驗證 WBUI 的實際版本, 不以較舊 tag 降版. 初次建置先取得既有 pin, 再檢查正式 tag

`/api/codex/file-summary` 只接收既有時間範圍, 對已取得的檔案事件選取最多 100 個不同位置讀取檔案 metadata. 單一檔案 API 仍使用 Thread / Call / 已取得路徑, 保留 UNC, symlink 與憑證檔拒絕規則. 檔案本文由既有文件明細與工具輸入輸出 API 按需提供

## 共用卡片與內容呈現

前端 cardLibrary 保存卡片的來源, 穩定識別碼, 類別, 支援的圖表形式與共用控制項. chartViews 保存各實例的參數與當次資料投影, 總覽副本沿用同一呈現函式並套用自己的範圍. summaryLibrary 保存已載入的摘要指標, 顯示數量, 順序與名稱由瀏覽器偏好管理. 新卡片沿用既有 renderer 與設定保存流程, 不增加資料讀取來源

payload_detail.py 完成遮蔽與結構投影後建立含 revision 的內容頁, 原有受限明細 API 加上 lazy=1 時回傳分頁. offset 與 revision 只用於已確認的同一工具或診斷紀錄, 前端逐頁建立語法顏色或 Markdown 預覽. Markdown 以文字節點與明確允許的連結建立 DOM, 不執行文件中的 HTML 或程式碼

通用 parser、語法顏色、Markdown 預覽與輸出檢視器由 Workbench UI 提供, LAM 只管理已遮蔽的資料、受限明細 API、內容頁版本與卸載時機. 同一檢視器套用工具、MCP、Git、驗證與錯誤明細, 解開 text 物件與 output / stdout / stderr, 混合 JSON 與 diff 分開顯示, 提供原文切換. 跨頁 JSON 完整讀取後才展開, 內容分批建立 DOM, 關閉明細釋放元件與觀察器, 不改變外層工具回覆的來源判定

## 分批收集與畫面載入

HTTP 服務提供等待狀態後, 單一收集 worker 依序讀取 session、統計投影、工作樹、帳戶、診斷、歷史、MCP、Jev 與硬體資訊. 定期檢查在來源及投影之間短暫讓出執行時間, session 在有新資料的區塊之間也讓出執行時間. 快照及活動狀態讀取使用最近已發布結果, 不等待收集鎖. 收集取消保留上一輪完整結果

前端只更新目前分頁內容, 已選的總覽副本保留自己的設定. 圖表接近可視範圍後才繪製, 每次排程只處理一張圖表, 切換分頁或更新資料時以最新工作取代尚未執行的工作. 圖表設定欄位在第一次點開齒輪時建立, 摘要與圖表預留顯示空間. 資料明細仍沿用原有按需讀取的路徑與權限檢查

監測頁使用瀏覽器原生 PerformanceObserver 顯示 TTFB、FCP、LCP、CLS、長任務與最慢互動. 首次資料呈現取首批資料更新 DOM 後的下一個畫面回呼時間, 圖表仍按需繪製. 只保存目前頁面的數值, 重新載入後重算, 不保存 DOM、URL 或事件內容, 不送至後端. LCP 取首次進入背景前的最後樣本, CLS 取間隔未達 1 秒且總長未達 5 秒的最大位移群組, 排除近期輸入造成的位移. Event Timing 使用 interactionId 大於零的最大 duration, 不標示為 INP. 缺少樣本或 API 支援時維持缺值, 參考 [CLS 定義](https://web.dev/articles/cls) 與 [Event Timing](https://www.w3.org/TR/event-timing/)

`GET /api/history` 依 namespace 與 section 查詢歷史, 每頁 1 - 200 筆, 可使用回傳的 next_cursor 依時間與穩定識別碼接續查詢, 最後一頁回傳 null. offset 上限 1000000, 可指定 since, 資料庫計算 total, 受來源檢查開關及現有 Host / Origin 限制. 這個 API 的保存範圍與畫面最近 24 小時等查詢範圍分開

本機與遠端共用的 HTTP 查詢 / SSE 通知設計見 [遠端存取](remote-access.md). 收集批次完成寫入與投影後發布資料版本, 前端合併變動通知再查詢目前畫面需要的資料, WBUI 局部更新並保留分頁、捲動及 modal. 本輪只納入設計, 實際仍採定時 HTTP 查詢, 尚未開放對外監聽或串流通知

`session_cursors` 保存每個來源最新的 metadata-only 讀取狀態, 供重啟恢復. `session_calls` 另存有界呼叫工作集合, 個別變動以 Call ID 更新, 不把整批呼叫重寫至 cursor JSON. 退休呼叫先保存白名單活動歷史, 再移除恢復用工作集合. 這兩表與 threads 的最新 lifecycle checkpoint 是來源恢復狀態, 不是活動歷史, 不依 90 天活動期限刪除. SQL / 網路 / MCP / Skills / 錯誤活動歷史預設保存 90 天, 設定 0 時不自動刪除

資料庫端 `aggregate()` 計算保存範圍的總數、SQL 分類、MCP 直接呼叫與程式碼辨識數量及時間序列, 不載入完整 payload 至 Python. 分類最多回傳 200 項並標示截斷, 時間序列最多 1440 格, 跨較長期間時以分鐘整數倍合併, 未提供時間的紀錄只列入總數
