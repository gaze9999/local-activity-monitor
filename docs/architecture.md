# 程式架構與擴充

## 執行環境

Python 3.10+ 標準函式庫, 原生 HTML / JavaScript / CSS, setuptools package assets. 服務綁定 loopback, `Host` / `Origin` / `Sec-Fetch-Site` 驗證保留. 資料庫採唯讀連線

| 檔案 | 責任 |
| --- | --- |
| `launch.py` / 平台入口 | 首次安裝, 使用 repository venv 啟動 watcher |
| `watch.py` | Python 原始碼儲存穩定後重啟自己建立的服務子程序 |
| `server.py` | loopback HTTP, snapshot cache, 設定驗證與 collector 協調 |
| `collectors.py` | Jev metadata DB, Codex JSONL 增量讀取, 時間 / counters / 工具摘要 |
| `codex_metadata.py` | 選取 catalog, session index 與 app state 中的名稱 / 分類 / 專案 |
| `operation_records.py` | 只解析 literal 呼叫, Git / Skill / 驗證 / 檔案操作 |
| `mcp_records.py` | 來源發現, 通用 MCP metadata, 已知欄位摘要, Web 參考 URL |
| `monitor_state.py` | 有限容量的效能樣本, 程序資訊與狀態事件 |
| `error_records.py` | 唯讀 App / Core Log, 錯誤與 SQL 診斷 metadata, 分批歷史回補 |
| `error_history.py` | 最近 24 小時錯誤摘要, 1000 筆 / 512 KiB 循環保存 |
| `thread_state.py` | 已確認 lifecycle 與 Skills 讀取 checkpoint, 原子保存與去重 |
| `sqlite_records.py` | literal SQLite 操作辨識, 不執行 SQL 或來源程式碼 |
| `usage_records.py` | 額度視窗與 credits 白名單投影 |
| `web/locales.json` | English / 日本語介面與 tooltip |
| `web/` | tab, 表格 / popup, lazy loading, 圖表, 外觀, 文件 / 文案編輯與通用表格排序 / 分頁 |

## 來源發現

從當前 `CODEX_HOME/config.toml` 選取 MCP section 名稱與 enabled 狀態, 不回傳 command, env 或 credentials. description 與已設定 README 提供用途, README 最多讀取 64 KiB, 用途快取 60 秒. Python 3.11+ 使用 `tomllib`, Python 3.10 使用只辨識 section / enabled / description 的相容解析, 跳過 multiline 字串

Session 中出現 `mcp__server__tool` 時自動建立來源. 已啟用的 Codex App / Review / Security / CUA plugins 也會提供對應來源. 來源來自設定或既有紀錄, 不執行背景 API 探測. 缺少來源的電腦不會顯示其項目, 原有瀏覽器偏好也不會憑空建立來源

已知來源使用分類預設, 新來源使用通用分類. 來源分類與工具說明可以在頁面修改. 呼叫若將 namespace 與 name 分開儲存, 會合併完整識別名稱. 不同 MCP 的同名工具分開統計. 新工具沒有特定 adapter 時仍顯示名稱, 操作, 時間, 資源 ID 及可辨識的結果

## 輸入 / 回傳資料

Snapshot 不包含 prompt, 完整命令, 任意 MCP input / output 或文件本文. MCP request 只保留白名單 metadata, 例如文件副檔名, OCR 模式, 寫入開關與資源 ID. response 只保留狀態, 數值 / boolean 與明確的數量摘要

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

`functions.exec` 內的工具是靜態辨識位置, 不是執行次數. 個別工具耗時維持 null, 另保留整次 exec 耗時. 多個工具共用的回傳不分配給其中任一工具. 單一呼叫才解析獨立摘要. 分類包含依名稱推斷的讀取 / 寫入 / 部署, 原始工具名稱一直保留

Skill 檔案依 session 中已觀察的 literal SKILL.md 路徑取得. Endpoint 接受 Skill 名稱與可選的目錄內相對文件名稱. 相對名稱必須存在於當次列出的可讀文件中, 讀取前再確認解析位置仍在該 Skill 目錄內. 排除 UNC, symlink, 隱藏目錄, credential 檔與快取目錄. 每次最多列出 200 個檔案, 掃描 1000 個項目與 4 層子目錄. Python 與其他非文件檔只取得 metadata, 不讀取原始碼. Markdown / TXT / RST / AsciiDoc 每份最多讀取 256 KiB, 顯示最多 32768 字元並遮蔽 credentials, hash 對應讀取片段. 一般 refresh 不掃描 Skill 目錄

Jev payload 仍只在指定呼叫點開時讀取, 遮蔽 credentials 並保留既有混合回傳隔離. Web 只保存 URL, query / fragment 不保留. 檔案操作保存 patch header, shell 明確讀寫位置, 或工具名稱與 path 參數辨識的檔案位置 / workdir / 工具識別, 不取出命令本文或檔案內文

## 範圍與效能

- 初次每個 session 讀取頭行與最多 1 MiB 尾端, 後續增量讀取. 缺少 lifecycle 時優先反向回查工作事件, 再回補最近 24 小時錯誤. 全部 session 共用每輪 8 MiB
- 檔案清單每 15 秒盤點, 預設 20 個 session, 自訂或全部追蹤最多 5000. 增量讀取以輪替游標避免單一大檔占滿額度
- 每個檔案保留最多 8192 個工具 Call ID, 全部檔案合計最多 50000, 超過移除最舊資料. 去除同 ID 的重複呼叫
- 未完成行每檔最多 1 MiB, 合計 8 MiB, 超過時丟棄該片段至下個換行. 額外 session error 每檔 50 筆, 全部最多 1000
- 工具趨勢保留最近 10080 個分鐘 bucket, 工具 / 時間排行最多 20000 個 bucket
- 對話工具與 Jev 明細各 100 筆, 檔案修改與讀取各 200 筆, 全部檔案操作列表最多 1000 筆. Git / Skills / 驗證列表最多 500 筆, MCP 列表最多 1000 筆
- Catalog 最多 2000 個對話 metadata, Jev 趨勢最多 168 個小時 bucket
- 對話全部顯示每批建立 50 列 DOM, 全部新表格預設每頁 20 筆, 排行 10 項, 顯示選項預設 5 / 10 / 20, 可自訂最多 8 個 1 - 200 選項. 圖表最多 240 格

`monitoring/thread-state.json` 保存最多 1000 個已確認 lifecycle checkpoint 與 500 筆 Skills 選定 metadata, 總上限 512 KiB. 保存狀態時間, 開始時間, Thread ID, 檔名與讀取 offset. Skills 保存 Thread / Call ID, Skill 與時間, 不保存 command 或文件位置. lifecycle / Skills 改變時保存, offset 最多每 30 秒更新. 使用原子替換, 拒絕 symlink, 保存失敗保留記憶體觀察

重啟後若初始尾端涵蓋 checkpoint 至最新位置, 可沿用確認狀態. 未涵蓋的離線區間繼續反向回補, 顯示快取來源與確認時間. 新事件優先覆蓋舊狀態. Skill count 依已保留的 500 筆 metadata 計算, 以 Thread / Call ID / Skill 去重

關閉觀察項目會停止對應解析或讀取. 來源開關會略過其摘要並隱藏事件, 開啟後重新解析 session 尾端. 來源名稱仍可供使用者重新開啟. 部分紀錄被截短或起訖缺漏時保持 null

## HTTP 與設定

| Endpoint | 用途 |
| --- | --- |
| `GET /api/snapshot?window=1h\|24h\|7d\|all` | 彙整 snapshot, window 作用於 Jev DB |
| `GET /api/codex/jev?thread=...&call=...&index=...` | 指定呼叫的已遮蔽 Jev 內容 |
| `GET /api/codex/skill?skill=...&file=...` | 已觀察 Skill 的檔案清單與文件 metadata. file 可省略, 指定時只讀取清單內的文字文件 |
| `GET /api/logs` | 目前保留的來源事件 metadata 與來源健康狀態 |
| `GET /api/instance` | 程式識別與 CODEX_HOME hash, 啟動時重用同一個 monitor |
| `POST /api/settings` | interval, max_files, track_all, observations, mcp_sources, mcp_categories, tool_descriptions, mcp_descriptions, mcp_tags, recording, replace_customizations |
| `POST /api/jev/recording` | 經驗證的本機 Jev enabled 開關 |

設定先完整驗證再變更. map 各最多 64 個自訂項目, 工具說明最多 400 字元. `/api/settings` body 上限 256 KiB. 觀察設定只存在目前後端程序, 網頁使用 localStorage 保存並在重新連線後套用. 外觀, 圖表, 表格排序 / 分頁, Tab 順序與介面文案屬於前端. 介面文字由 HTML 純文字與 ui() 標籤集中登錄, 略過數字, 單獨單位與帶有拼接空白的片段. data-copy="ignore" 可排除整個元件. 圖表動態控制項保留原始標籤供即時套用, 不修改原始紀錄值或 ID. 新 UI 元件沿用 ui() 與通用表格控制, 新 Tab 會自動接到目前排序尾端

Snapshot 額外提供 `default_settings` 與各 Tab 的來源讀取範圍. MCP 回傳只投影最多 40 個符合用量, credits, 次數或耗時語意的數值, 排除 credentials 與任意帳戶欄位. Token 用量採 thread 最新累計, 額度保留來源視窗, 剩餘百分比與時間, 不推估帳單金額

Snapshot 的 monitor 欄位提供 runtime, uptime, refresh / process CPU 耗時, 本輪 session bytes, 資料保留量, HTTP 回應大小與錯誤計數. 360 個效能樣本及 200 個狀態事件使用 bounded deque, 重新啟動後清空. 整理失敗保留前次 cache, poll 繼續重試, 紀錄只保存例外類型

錯誤觀察使用平台 log 目錄及 CODEX_HOME 的 logs_*.sqlite, 驗證 schema 後唯讀選取 metadata. Desktop 最多 8 個近期 log, 每份初始 256 KiB 尾端, 增量與歷史回補每輪各最多 1 MiB, 未完成行最多 64 KiB. Core 每輪最多最近 2000 列 ID, 歷史回補每輪最多 2000 列. 有 0.08 秒 SQLite 讀取預算, 投影診斷 / SQL metadata, body 只暫讀前 8192 字元供分類, 不保留. 診斷事件與輸出的錯誤清單各最多 1000 筆, source 缺少 / schema 不符顯示健康狀態. 詳細錯誤判定見 [錯誤觀察](error-observation.md)

表格以原始欄位 ID 對應儲存排序, 顯示與拖曳順序, DOM 位移不改變排序欄位. 原始 cells 使用 WeakMap, popup 關閉清除對應 table view. 桌面多欄位在表格範圍橫向捲動, 760 px 以下改成資料卡. 共用 filter / paginate / column 規則套用新增表格. 熱度使用目前頁面數值逐欄正規化, 預設關閉. 設定匯入 / 匯出 version 1, 原始文案作 key, 格式見 [設定檔格式](settings-format.md)

瀏覽器 document.hidden 時略過自動 HTTP / render, 重新顯示後 refresh. 後端保持收集. Cache 只在整理時建立, HTTP 取值時深複製選定 window 後附上最新程式狀態

HTML 內嵌目前 CSS / script 並提供精確 CSP SHA-256, 換行先統一為 LF 以符合 HTML parser. HTTP/1.1 支援連線重用, 保留 Content-Length, 15 秒 socket timeout, 被拒絕的請求關閉連線. assets / snapshot 使用 no-store, 可接受的 response 以 gzip 壓縮. 前端請求包含 15 秒 timeout, 版本更新後重新載入並保留有效偏好. watcher 不修改其他服務程序

## 驗證

使用 checkout 的 src 執行隔離 fixture. PowerShell 設定 `$env:PYTHONPATH="src"` 後執行 `python -m unittest discover -s tests -v`. macOS / Linux 使用 `PYTHONPATH=src python -m unittest discover -s tests -v`, 並以 `node --check src/local_activity_monitor/web/app.js` 檢查語法. UI 需另外驗證來源有 / 無, 設定與文件保存, 明細, 分頁 / 分批載入, 圖表設定與淺 / 深色外觀. 原生套件由各目標系統的 GitHub runner 建置並執行隔離 HTTP smoke test

## 原生發布

`tools/portable.py` 直接啟動 server, 不使用原始碼 watcher 或安裝流程. `tools/build_release.py` 在各原生系統使用固定版 PyInstaller 建置資料夾套件, 收入完整 web assets, 說明與 runtime 授權文件. `tools/smoke_release.py` 以暫存 CODEX_HOME 與 loopback 隨機 port 啟動該執行檔, 核對 HTTP, CSP, 資產, 版本與任意檔案存取邊界.

`.github/workflows/release.yml` 由 workflow_dispatch 對指定 commit 執行 Windows x64, macOS Intel / ARM64, Linux x64 / ARM64 建置, 測試失敗不進入 release 上傳. 原始碼 zip, sdist, wheel 與 SHA-256 一起附加到 draft release, 確認產物後再公開. Build 工具列在 requirements-build.txt, 不加入 runtime 相依.
