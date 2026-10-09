# 錯誤觀察

右上角"錯誤"顯示最近 24 小時已載入的 error 事件數, 點擊進入錯誤與 Log 的錯誤紀錄子頁並選擇錯誤等級. 警告另列, 可切換等級查看

## 來源與判定

| 來源 | 判定與可取得 metadata |
| --- | --- |
| Codex App log | error / warning / warn 級別, 時間, 模組, conversationId / threadId, errorCode, errorName, method. 送出失敗與串流中斷依事件標題辨識 |
| Codex Core SQLite log | ERROR / WARN 級別, 時間, target, thread_id. 串流重試與工具 / MCP 模組保留診斷分類 |
| Session event | error, turn_aborted, turn_failed 或以 `_failed` 結尾的事件, code 與對話 ID |
| 工具回傳 | isError, 非空 error, 明確 failed / timeout 等狀態, 非零 exit_code 或標準 `Process exited with code` 行 |
| Jev telemetry | 選定 window 內 recent operation 的 HTTP / network / response error attempt, HTTP status 與嘗試序號 |
| 觀察程式 | 整理例外類型, HTTP 錯誤狀態. 重複的連續同類事件合併, 累計計數另列在程式狀態 |

工具回傳只有一個可辨識 MCP 呼叫時才關聯該來源. exec 混合多個呼叫的失敗歸於整次 exec. 不將回傳內容中的一般"error"字詞視為工具失敗, 不從工具回傳時間推論命令成功

表格的每一筆是來源事件. 同一個送出失敗可能同時在 App, Core 或工具紀錄出現, 不把這些筆數解讀成不同使用者送出的次數. 連線重試保留 warning 級別, 不直接當成最終失敗

## 資料邊界

不保存 errorMessage, errorStack, 完整命令, prompt, response 或 HTTP error body. 明細顯示原因分類, 已選取的 metadata, 錯誤類型, HTTP / exit code, Request / Trace / Call ID, 來源檔名 / 紀錄 ID 與相關事件, 未知 code 保留原始識別名稱. 找得到對話 / MCP 操作時提供查看按鈕, 不從私人字串補出未知欄位

Desktop log 只從級別與固定 key-value 欄位取值, 排除引號內的 errorMessage / stack 內容, 避免把內文中的 ID 當成對話 ID. Core body 僅短暫讀取片段供分類, 不加入 snapshot 或其他資料庫

## 路徑與容量

- Windows: `%LOCALAPPDATA%/Codex/Logs`
- macOS: `~/Library/Logs/Codex`
- Linux: `$XDG_STATE_HOME/Codex/Logs` 或 `$XDG_CONFIG_HOME/Codex/logs`, 缺少環境變數時使用 `~/.local/state` / `~/.config`
- Core: 目前 CODEX_HOME 下的 `logs_*.sqlite`, 動態選取檔案並驗證 logs schema

Desktop 最多追蹤 8 個近期 log, 初始每份 256 KiB 尾端, 後續差異讀取, 差異讀取與歷史回補每輪各最多 1 MiB, 未完成片段每檔最多 64 KiB. Core 每輪選取最近最多 2000 列 ID, 歷史分批回讀每輪最多 2000 列並限制在最近 24 小時, SQLite 讀取預算為 0.08 秒. 輸出的診斷事件最多 1000 筆, 完整錯誤清單也最多 1000 筆, 超過保留最新事件

找不到目錄顯示"未找到", DB schema 改變顯示"格式未支援", 權限 / SQLite 讀取失敗顯示"無法讀取". 不寫入或修改 Codex log. 平台目錄探索與 fixture 已驗證, 原生 macOS / Linux App log 的實際格式仍需在目標環境確認

"對話與工具錯誤"switch 停止診斷讀取與 session / tool error 解析. 重新啟用時回補目前選取尾端, 觀察程式本身的錯誤仍保留. 診斷事件最多 1000 筆保存在記憶體. 錯誤摘要另外保存至 `monitoring/lam-history.sqlite3`, 不保存原始訊息. 記憶體工作集合上限 1000 筆 / 512 KiB, SQLite 歷史不依筆數刪除, 預設保存 90 天, 設定 0 時不自動刪除. 既有 `error-history.json` 僅供首次匯入, 原檔保留. 重整 / 重啟後保留, 新事件依識別去重. 本程式 Log 只寫啟動, 設定, 失敗 / 恢復等事件, 每份 64 KiB 與一份輪替檔, 最多 128 KiB

## 對話狀態與雲端缺值

Codex 工作狀態由 task_started / task_complete 判定. 開始事件超出初始尾端時, 在 session 每輪 8 MiB 共用預算內反向回查, 找到最近 lifecycle 後停止. 最近確認狀態與 offset 保存於 `monitoring/lam-history.sqlite3` 的 threads 分區, 舊 `thread-state.json` 首次匯入後保留, 重啟後沿用, 未涵蓋的離線區間繼續回補. 明細標示回查中或工作事件來源. 完成事件自身帶 started_at / completed_at 時可補出耗時, 不需從對話建立時間估算

純 ChatGPT 雲端對話由本機 catalog 提供名稱, 時間與分類. catalog 若有 Model / Reasoning / async status 欄位就依有效型別讀取, 沒有 Codex session 或來源未提供的 token / 工具資料維持 --, 不呼叫 Provider API 估算


## 模型 API 呼叫監測

"錯誤與 Log"的"模型 API"子頁讀取既有 Codex Desktop / Core 診斷紀錄, 分開統計模型請求, WebSocket 連線與串流重試. `codex.api_request` 表示 HTTP 請求階段, `codex.websocket.request` 表示 WebSocket 送出階段, 送出成功不代表模型回應完成. 已知的 models / discovery 路徑不計入模型請求

明細保留 Model, Provider, 傳輸方式, API 路徑, Request / Trace / Turn ID, HTTP status 與來源提供的耗時及單次 Token. 缺值顯示 `--`, 不從對話累計 Token 推估. 連線與重試紀錄不帶推估的 Token 或耗時, 重試事件數與來源回報的重試次數分開

"模型 API 呼叫監測"開關只控制本機讀取, 不開啟 Codex 的額外 telemetry, 不修改來源設定或呼叫 Provider API. Core 每輪沿用最多 2,000 列的差異與歷史讀取, 只在已識別模組讀取最多 8,192 字元供欄位投影. 記憶體保留最近最多 1,000 筆去重事件, 重啟後從既有來源分批重讀最近 24 小時, 不另存 API 本文或原始診斷. 來源未記錄完整請求時, 呼叫紀錄數只表示已觀察數量

欄位與事件語意參考 [Codex API 傳輸實作](https://github.com/openai/codex/tree/main/codex-rs/codex-api) 及 [API 計數指標](https://github.com/openai/codex/blob/main/codex-rs/otel/src/metrics/names.rs), 實際支援仍依本機紀錄格式

## SQL 診斷與來源 Log

來源 Log 子頁透過 `GET /api/logs` 列出選定事件 metadata, 含原有 info / debug / trace 等級與來源讀取狀態. 原始 Log 留在來源位置, 明細提供對照所需的時間, 模組, 識別碼與原因分類

LAM 的執行紀錄以輪替方式保存於 `monitoring/local-activity-monitor.jsonl` 及 `.jsonl.1`, 每份最多 64 KiB. 瀏覽器透過 loopback `POST /api/diagnostics` 回報已驗證的錯誤類型, 發生階段, 前端版本與最多 8 個程式位置, 不接受錯誤本文, 任意網址或私人路徑. 同一錯誤每 60 秒去重, 全部瀏覽器合計每分鐘最多 20 筆. 來源 Log 關閉時不寫入磁碟, 只保留原有有界記憶體事件

Windows 啟動記錄檔存於專案的 `.local/startup-logs/`, 執行中每份達 1 MiB 時輪替, 同次啟動保留主檔與最多 3 份備份, 目錄保留最近 10 次啟動. 啟動、前端建置、程序結束與錯誤輸出會持續寫入, 關閉啟動視窗時保留已寫入的記錄檔

Core Log 若提供可辨識的 SQL / SQLite 模組, 可選取操作類型, 個別耗時, 影響 / 回傳列數與錯誤. SQL 操作頁合併工具與診斷紀錄. 工具中的 sqlite3 命令, literal Python SQLite 與 MCP SQL 參數也可辨識, SQL 本文在點開操作明細時讀取並遮蔽可辨識的憑證, 不加入 snapshot 或內容資料庫, 不讀取查詢結果, 不執行來源程式碼. 沒有可辨識紀錄時 0 表示目前觀察到的操作數
