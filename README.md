# Local Activity Monitor

僅供本機使用的 Jev / Codex 活動監看頁面; 使用 Python 標準函式庫與原生網頁, 不需 Node, GPU, API Key 或雲端服務

畫面標示為 **本機觀察統計**; 不代表 Provider 帳戶全部用量, 剩餘額度或費用

## 安裝與啟動

需要先有 Python 3.10+; Windows 雙擊 `Start.cmd`, macOS 雙擊 `Start.command`, Linux 執行 `sh start.sh`

首次啟動若尚未安裝, 入口會詢問 `Install and start now? [y/N]`; 輸入 `y` 後建立 repo 內的 `.venv`, 安裝 `requirements.txt`, 接著開啟監看頁面; Enter / `n` 取消且不安裝. 已安裝時直接啟動, 不重複安裝或自動升級; 不會自動下載 Python 本身

若偏好手動安裝, 以下指令在此 repo 執行

Windows:

```powershell
python -m venv .venv
.venv\Scripts\python.exe -m pip install -r requirements.txt
.venv\Scripts\local-activity-monitor.exe --codex --open
```

macOS / Linux:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/local-activity-monitor --codex --open
```

不需打包成執行檔; 啟動入口會使用 repo 的 `.venv` 並開啟預設瀏覽器. 安裝失敗會保留錯誤, 下次缺少套件時可再確認重試; 已存在但損壞的 `.venv` 不會自動刪除或覆寫. 日後拉取新版原始碼, 需自行重新執行上述安裝指令

macOS 若下載的檔案未保留執行權限, 在 repo 執行一次 `chmod +x Start.command`; 系統若要求安全性確認, 依 macOS 的提示檢查來源並處理, 不需停用系統防護

`requirements.txt` 安裝此專案本身; runtime 只使用 Python 標準函式庫, build 相依由 `pyproject.toml` 管理; 首次安裝可能需要網路下載 build 工具; 不會自動啟用 Jev 紀錄或安裝 Codex

入口皆加入 `--codex --open`; CLI 不加 `--codex` 時不讀 Codex sessions; 入口會保持服務終端開啟, 啟動失敗時保留錯誤訊息; 若重複開啟導致 port 已被使用, 使用既有頁面或先停止原本的服務

預設頁面是 `http://127.0.0.1:8787/`; 只能從本機連線; `--port 8790` 可指定其他 port; 終端按 Ctrl+C 停止, 關閉瀏覽器分頁不會停止服務

也可安裝已建置的 `local_activity_monitor-0.1.0-py3-none-any.whl`, 並從任意資料夾呼叫環境中的 `local-activity-monitor`; 不必保留 checkout

## Jev 紀錄預設關閉

需要含 `jev_telemetry.py` 的新版 Jev client, 由 `codex-setup` 的 Jev wheel 或 managed Skill 安裝提供; 本 repo 不維護第二份 API client

```text
local-activity-monitor --enable-jev --configure-only
local-activity-monitor --codex --open
```

第一行明確啟用 metadata 紀錄, 只設定本機檔案; 不查 Jev API, 不讀 Key, 不送出專案資料; Jev CLI 與 MCP 讀取同一份設定

- 設定檔: `$CODEX_HOME/monitoring/jev-monitor.json`, 未設定 `CODEX_HOME` 時使用 `~/.codex`
- Windows 資料庫: `%LOCALAPPDATA%/local-activity-monitor/state/jev.sqlite3`
- macOS 資料庫: `~/Library/Application Support/local-activity-monitor/state/jev.sqlite3`
- Linux 資料庫: `$XDG_DATA_HOME/local-activity-monitor/state/jev.sqlite3`, 未設定絕對位置時使用 `~/.local/share`
- `--codex-home` 與 `--database` 可指定實際位置; Jev 與 dashboard 必須使用相同的 `CODEX_HOME`; Windows Store 虛擬化環境要確認兩個程序看到同一個資料庫

已開啟的 Jev MCP process 必須重新載入才會使用新版程式; 設定啟用後, 新版 client 每次操作讀取 opt-in 狀態; 不會回補舊 client 的歷史紀錄

```text
local-activity-monitor --disable-jev --configure-only
```

停用會保留已有紀錄; 個別 Jev process 也可設定 `JEV_TELEMETRY=0`; 清除歷史時先停用紀錄, 關閉 dashboard 與 Jev process, 再自行處理指定資料庫及 SQLite sidecar; 本工具不自動清除資料

## 顯示範圍

| 來源 | 觀察內容 | 限制 |
| --- | --- | --- |
| Jev | Rank / Evaluate / Doctor 的 status, model, latency, HTTP 嘗試, body bytes, provider 回傳的 input/output tokens | 一個操作完成時寫一列; 程序強制終止或資料庫不可寫時可能缺漏; 未知 usage 為 null |
| Codex | 近期 thread ID, model, 最新累計 counters, 直接工具名稱與次數 | 首次只讀近期檔案尾端, 再讀新增紀錄; 不是全部歷史; fork 可能繼承 token counters |

Jev 不保存 query, rubric, state, candidates, answers, credential, header, 原始錯誤或私有來源路徑; 紀錄失敗不改變 Jev 的原本結果或錯誤處理; HTTP error body 不另讀取

Request bytes 是每次 HTTP 嘗試的 JSON body 大小; response bytes 只計已讀取內容, 未知部分另列; 不是 TLS / header / 網路流量或費用

Codex collector 只從 JSONL 選取白名單 metadata, 不將原始紀錄, 對話文字, 指令或工具參數送到頁面; 不讀 `auth.json`, 憑證檔或 shell history; `exec` 內部 MCP 呼叫無可靠直接欄位時不推測

Token 使用每個 thread 最新累計快照; 不將每次快照或 last-turn counter 相加; cached input 與 reasoning output 是子項目, 不額外加到 total; 不彙整成帳戶總用量

每 4 秒更新 collector cache, 檔案清單每 15 秒盤點; 預設最多 20 個近期 session 檔案, 初次每個檔案最多讀 1 MiB 尾端, 每輪新增讀取總預算 8 MiB; 處理部分行, 檔案截短與損壞 JSON; 程序重啟後重新開始本次觀察範圍

## 後續增加來源

每個來源各自實作 collector, 回傳 `source`, `health`, `scope`, timestamps 與必要的摘要; 加入 `Dashboard` 的本機 snapshot 與對應畫面; 共用 server 與 UI, 不需要複製 Jev 邏輯或導入通用 plugin framework

來源需要背景服務, credential 或 native 相依時, 再依該來源提供選用設定與相依; 目前只包含 Jev / Codex, 尚未加入其他監控器

## 開發與驗證

```text
python -m unittest discover -s tests -v
python -m pip wheel --no-deps --wheel-dir dist .
```

測試使用臨時 metadata, 不呼叫付費 API; HTTP guard 直接測試實際 handler. Windows 已驗證首次建立環境, 安裝套件及啟動入口; 6 項安裝流程與 5 項 metadata / HTTP 檢查通過. 瀏覽器穩定呈現尚未完成驗證; macOS / Apple Silicon / Linux 尚未在原生環境驗證, macOS 入口目前僅完成 shell 語法檢查

Jev usage 欄位來源見 [TypeSafe API 文件](https://docs.typesafe.ai/api); 本工具只觀察 client 回應, 不實作 Provider 帳戶用量查詢
