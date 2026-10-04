# 錯誤觀察

右上角「錯誤」顯示最近 24 小時已載入的 error 事件數, 點擊進入錯誤紀錄 Tab 並選擇錯誤等級; 警告另列, 可切換等級查看

## 來源與判定

| 來源 | 判定與可取得 metadata |
| --- | --- |
| Codex App log | error / warning / warn 級別, 時間, 模組, conversationId / threadId, errorCode, errorName, method; 送出失敗與串流中斷依事件標題辨識 |
| Codex Core SQLite log | ERROR / WARN 級別, 時間, target, thread_id; 串流重試與工具 / MCP 模組保留診斷分類 |
| Session event | error, turn_aborted, turn_failed 或以 `_failed` 結尾的事件, code 與對話 ID |
| 工具回傳 | isError, 非空 error, 明確 failed / timeout 等狀態, 非零 exit_code 或標準 `Process exited with code` 行 |
| Jev telemetry | 選定 window 內 recent operation 的 HTTP / network / response error attempt, HTTP status 與嘗試序號 |
| 觀察程式 | 整理例外類型, HTTP 錯誤狀態; 重複的連續同類事件合併, 累計計數另列在程式狀態 |

工具回傳只有一個可辨識 MCP 呼叫時才關聯該來源; exec 混合多個呼叫的失敗歸於整次 exec. 不將回傳內容中的一般「error」字詞視為工具失敗, 不從工具回傳時間推論命令成功

表格的每一筆是來源事件. 同一個送出失敗可能同時在 App, Core 或工具紀錄出現, 不把這些筆數解讀成不同使用者送出的次數. 連線重試保留 warning 級別, 不直接當成最終失敗

## 資料邊界

不保存 errorMessage, errorStack, 完整命令, prompt, response 或 HTTP error body. 明細顯示已選取的 metadata 與通用中文說明, 未知 code 保留原始識別名稱; 找得到對話 / MCP 操作時提供查看按鈕, 不從私人字串補出未知欄位

Desktop log 只從級別與固定 key-value 欄位取值, 排除引號內的 errorMessage / stack 內容, 避免把內文中的 ID 當成對話 ID. Core body 僅短暫讀取片段供分類, 不加入 snapshot 或其他資料庫

## 路徑與容量

- Windows: `%LOCALAPPDATA%/Codex/Logs`
- macOS: `~/Library/Logs/Codex`
- Linux: `$XDG_STATE_HOME/Codex/Logs` 或 `$XDG_CONFIG_HOME/Codex/logs`, 缺少環境變數時使用 `~/.local/state` / `~/.config`
- Core: 目前 CODEX_HOME 下的 `logs_*.sqlite`, 動態選取檔案並驗證 logs schema

Desktop 最多追蹤 8 個近期 log, 初始每份 256 KiB 尾端, 後續增量讀取, 每輪最多 1 MiB, 未完成片段每檔最多 64 KiB. Core 每輪選取最近最多 2000 列 ID, 不搜尋整個歷史 body; 輸出的診斷事件最多 1000 筆. 完整錯誤清單也最多 1000 筆, 超過保留最新事件

找不到目錄顯示「未找到」, DB schema 改變顯示「格式未支援」, 權限 / SQLite 讀取失敗顯示「無法讀取」; 不寫入或修改 Codex log. 平台目錄探索與 fixture 已驗證, 原生 macOS / Linux App log 的實際格式仍需在目標環境確認

「對話與工具錯誤」switch 停止診斷讀取與 session / tool error 解析; 重新啟用時回補目前選取尾端, 觀察程式本身的錯誤仍保留. 診斷事件存在記憶體, 重新啟動後從有限範圍重新建立, 不另外寫入歷史 DB

## 對話狀態與雲端缺值

Codex 工作狀態由 task_started / task_complete 判定. 開始事件超出初始尾端時, 在 session 每輪 8 MiB 共用預算內反向回查, 找到最近 lifecycle 後停止; 明細標示回查中或工作事件來源. 完成事件自身帶 started_at / completed_at 時可補出耗時, 不需從對話建立時間估算

純 ChatGPT 雲端對話由本機 catalog 提供名稱, 時間與分類. catalog 若有 Model / Reasoning / async status 欄位就依有效型別讀取, 沒有 Codex session 或來源未提供的 token / 工具資料維持未知, 不呼叫 Provider API 估算
