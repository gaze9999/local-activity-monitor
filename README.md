# Local Activity Monitor

監測本機 Codex 與 MCP 活動的瀏覽器介面, 提供對話、用量、工具、專案及診斷明細. 後端使用 Python 3.10+ 標準函式庫, 前端使用原生 HTML / JavaScript / CSS 與 Workbench UI

0.x 以提交與 minor 版本 tag 交付, 建置包保存在 [Actions artifacts](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml). GitHub Release 從 1.0.0 起提供

## 功能

| 主頁 | 內容 |
| --- | --- |
| 總覽 | 可選擇及排序的圖表、帳戶用量與活動摘要 |
| 用量與額度 | 模型 Token、帳戶額度剩餘比例、重設時間與 credits |
| 對話 | 對話列表、子代理程式、Context、排程與 My dots |
| 專案 | 專案列表、Git 操作、工作樹與驗證活動 |
| 工具 | 呼叫與耗時、MCP、技能與 Plugins |
| 資料操作 | 網路參考、檔案操作與 SQL 紀錄 |
| 錯誤與 Log | 錯誤分類、來源 Log 與關聯操作 |
| 監測程式 | 收集進度、CPU 時間、讀取量、硬體與頁面效能 |

介面支援繁體中文、English 與日本語, 提供深淺模式、字型、強調色、圖表形式、篩選、分頁及拖曳排序. 偏好設定可匯出與匯入 JSON, 操作明細按需載入

目前功能與範圍見 [功能清單](docs/features.md)

## 從原始碼啟動

需要 Python 3.10+. 首次取得 private 的 Workbench UI 來源時, 需要 Git 與該儲存庫的既有存取權限

```sh
git clone https://github.com/gaze9999/local-activity-monitor.git
cd local-activity-monitor
```

- `launch-cli.cmd`: Windows 入口
- `launch-cli.command`: macOS 入口
- `tools/launch-cli.py`: 各系統共用的原始碼入口, 免安裝包以 `tools/launch-portable.py` 打包成執行檔

入口優先使用儲存庫內的 `.venv`, 其次使用已安裝的 Python, 自動建置頁面並開啟 `http://127.0.0.1:8787/`. 終端按 Ctrl+C 停止服務. 可加入 `--port 8790` 指定連接埠, `--codex-home` 指定 Codex 資料目錄

相同連接埠及 `CODEX_HOME` 已有 LAM 時, 新入口會開啟既有服務. 其他程式占用連接埠時會顯示錯誤. macOS 入口需要執行權限時, 執行 `chmod +x launch-cli.command`

缺少 Python 時可使用 `launch-cli.cmd --install-python` 或 `sh launch-cli.sh --install-python`, 確認後透過 Python Install Manager / winget、Homebrew 或 apt 安裝

### 啟動排查與完整停止

Windows 的 `launch-cli.cmd` 在入口結束後會顯示退出碼並等待按鍵, 包含退出碼 0. 顯示 `already_running` 表示重複啟動已轉用原本的服務, 應保留原本的監測視窗. 顯示 `listening` 才表示本次已開始監聽

原始碼入口會將最近一次啟動階段寫入 `.local/startup-status.json`, 包含 Python 版本與位置, 程序 ID, 更新時間, 已確認的 loopback URL 與可取得的退出碼. 不保存啟動參數或完整 Log. 狀態檔是最近一次寫入的結果, 強制關閉視窗後可能停留在 `listening`, 不能單憑此檔判斷服務仍在運作

Windows 原始碼的 `.cmd` 入口另保存 `.local/startup-logs/startup-*.log`, 從 Python 啟動前記錄 bootstrap, 建置與監測程序的 stdout / stderr, 包含錯誤堆疊及可取得的退出碼. 每行寫入後立即刷新, 強制關閉時仍保留已寫入內容, 沒有 `wrapper_exit` 或退出碼為 `unknown` 時表示未取得完整退出結果

啟動 Log 在運作中循環輪替, 每個檔案最多 1 MiB, 滿額後依序保留 `.log.1` 至 `.log.3`, 最新訊息持續寫入 `.log`. 超長單行會截斷, 啟動時保留最近 10 組 Log, 每組最多 4 MiB, 使用中的舊記錄不強制刪除. 可辨識的 API key, Bearer, 密碼與 URL 認證資訊會遮蔽, 記錄只保存在忽略追蹤的 `.local/`, 不複製 Codex 對話或模型回應

運作事件另保存於目前 `CODEX_HOME/monitoring/local-activity-monitor.jsonl`, 每份最多 64 KiB, 滿額時輪替到 `.jsonl.1`, 合計最多 128 KiB. 成功整理時每 60 秒記錄一次運作狀態, 也記錄整理失敗與恢復, HTTP 錯誤, 服務重啟與正常停止. 瀏覽器的畫面更新錯誤與未處理錯誤會保存類型, 程式檔名, 函式, 行列及前端版本, 同一錯誤每分鐘最多一筆, 每分鐘合計最多 20 筆. 可在"錯誤與 Log → 來源 Log"查看並開啟明細, 不保存錯誤本文或對話內容

| 畫面或狀態 | 處理方式 |
| --- | --- |
| `already_running` | 使用原本的監測視窗與網頁, 不需要啟動第二個服務 |
| `build_frontend` / `failed` | 查看上方建置錯誤, 核對 Workbench UI 存取權限與固定版本, 保留既有資產 |
| Python 不可用或 `.venv` 不完整 | 核對現有 runtime 或 `.venv`, 不直接刪除既有環境, 需要安裝時使用上述入口 |
| 連接埠已被其他程式占用 | 以 `launch-cli.cmd --port 8790` 啟動, 不終止未確認用途的程序 |
| `watcher` / `failed` 或啟動後立即結束 | 保留退出碼與畫面上的錯誤, 對照狀態檔的 `service_status` 判斷是否曾開始監聽 |
| 畫面更新失敗 | 重新整理頁面取得最新前端, 在來源 Log 查找"畫面執行錯誤"並開啟明細, 保留前端版本與行列 |
| 視窗仍直接消失 | 在既有終端執行 `cmd /k launch-cli.cmd`, 核對是否使用本儲存庫入口, 並保留上方錯誤與狀態檔 |

完整停止時優先在監測終端按 Ctrl+C, 若 Windows 詢問是否終止批次工作, 輸入 `Y`. 直接關閉啟動視窗也會一併結束這次入口所啟動的 watcher, HTTP 服務與子程序, 不需要逐一結束 Python. Windows 使用 Job Object 管理子程序, 正常停止則透過控制串流要求服務結束並等待釋放連接埠

只關閉瀏覽器網頁不會停止監測. 重複啟動轉用既有服務時, 關閉新入口視窗也不會停止原本的服務, 要關閉原本持續執行的監測視窗

### Workbench UI 與頁面建置

LAM 頁面來源位於 `frontend/`. CLI 啟動會執行 `tools/build_frontend.py --latest`, 自動取得 WBUI 並產生 `src/local_activity_monitor/_web/` 與 `_workbench/`

- WBUI 0.x 選擇最高的正式 `vMAJOR.MINOR.PATCH` tag
- WBUI 1.0.0 起選擇 GitHub Latest 的已發布非預覽 Release, 需要已登入的 GitHub CLI
- 來源快取位於 `.local/workbench-ui/<revision>`, 建置成功後才將完整 SHA 寫入 `workbench-ui.json`
- 查詢或下載失敗時沿用已驗證資產, 首次取得失敗時依錯誤訊息補齊 Git、認證或來源

以下命令只建置 `workbench-ui.json` 記錄的版本, 用於離線重建與 CI:

```sh
python tools/build_frontend.py
```

`WORKBENCH_UI_PATH` 可指定 WBUI 開發來源. `tools/prepare_ui.py --ensure` 準備已記錄的 WBUI 版本, `--update` 更新乾淨的共用來源 main. 正式 tag 更新由 `build_frontend.py --latest` 處理

本機共用元件修改也可由 `.local/workbench-ui-development.json` 選擇來源, 欄位為 `version: 1`、WBUI checkout 的絕對 `source` 路徑與符合 `workbench-ui.json` 的 `revision`. 開發模式直接提供該 checkout 的資產, watcher 同時監看共用檔案. 正式封裝仍使用已提交的 pin, 開發選擇不寫入封裝或 Git

左上角顯示 LAM 與實際載入的 WBUI 版本. Watcher 監看 Python 與 `frontend/` 修改, 建置成功後重啟自己建立的服務, 建置失敗時保留執行中的服務

WBUI revision 記錄於 `workbench-ui.json`. 自動更新繼續選擇正式 tag, 版本低於目前已建置版本時沿用原版. 明確串接其他 revision 可使用 `python tools/build_frontend.py --revision <完整 SHA>`, 建置成功後才更新版本 pin

關閉 CLI 或按 Ctrl+C 時, 啟動器通知自己建立的 watcher 與服務退出並釋放連接埠. Windows 使用 Job Object 讓隱藏子程序隨所屬 CLI 結束, Linux / macOS 的終止訊號會進入退出清理. 建置暫存目錄在建置完成或失敗後清除, 已取得的 WBUI 快取與必要資料繼續保存

### 安裝 Python 套件

先建置頁面, 再在選定的 Python 環境安裝專案:

```sh
python tools/build_frontend.py --latest
python -m pip install .
local-activity-monitor --codex --open
```

也可安裝 Actions 產出的 wheel. 已安裝的 CLI 使用內附資產, `--codex` 啟用 Codex 活動讀取, `--open` 開啟瀏覽器

## 免安裝 CLI 套件

[Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml) 提供五平台建置, 套件內含 Python、LAM、WBUI 及授權文件. 下載 artifacts 需要 GitHub 存取權限, 檔案受 Actions 保留期限限制

| 套件 | 啟動方式 |
| --- | --- |
| `windows-x64-cli.zip` | 解壓縮後使用 `launch-cli.cmd`, 入口呼叫包內的 `launch-cli.exe` |
| `macos-arm64-cli.tar.gz` | Apple Silicon, 使用 `launch-cli.command` 或 `launch-cli` |
| `macos-x64-cli.tar.gz` | Intel Mac, 使用 `launch-cli.command` 或 `launch-cli` |
| `linux-x64-cli.tar.gz` / `linux-arm64-cli.tar.gz` | 使用 `launch-cli.sh` 或 `launch-cli` |

保留完整解壓縮目錄. 原生入口預設啟用 Codex 並開啟瀏覽器, `--no-codex` 關閉 Codex 讀取, `--no-browser` 關閉自動開啟瀏覽器. 套件使用內附資產, 可離線啟動

## 資料來源

| 來源 | 資料意義 |
| --- | --- |
| Codex JSONL 與本機 catalog | 對話標題、狀態、專案、模型、工具與每個 thread 的最新累計 Token |
| MCP 設定與工具紀錄 | 來源名稱、操作、時間、結果及來源提供的數值摘要 |
| Jev telemetry | 每個完成操作的模型、耗時、HTTP 嘗試、body bytes 與 provider usage |
| App / Core Log | 診斷等級、類型、時間、識別碼與關聯活動 |
| 本機設定與快取 | 排程、My dots、Plugins 與工作樹 metadata |

Token 分析使用每個對話的最新累計快照, Cached input 與 Reasoning output 為子項目. 帳戶額度顯示來源回報的剩餘比例與重設時間, 來源缺值顯示 `--`, 已確認零值顯示 `0`

主設定的「官方帳戶查詢」預設關閉. 啟用後透過已登入的 Codex CLI 取得方案、額度與帳戶用量, 查詢結果至少快取 60 秒. My dots 與遠端活動顯示 Codex 桌面快取的近期活動摘要

一般更新收集選定 metadata, 完整 Git、SQL、工具輸入輸出、Log 與技能文件在開啟明細時受限讀取並遮蔽 credentials. 服務使用 loopback 與 Host / Origin 檢查. 讀取範圍、保存期限與上限見 [資料盤點](docs/data-inventory.md)

### 回補資料

`CODEX_HOME/monitoring/lam-history.sqlite3` 保存 SQL / 網路 / MCP 活動、錯誤摘要、已確認對話狀態與技能讀取紀錄. 首次讀取會匯入既有 `activity-history.json`、`error-history.json` 與 `thread-state.json`, 原檔保留, 後續以 SQLite 資料為準. 程式記錄檔 `local-activity-monitor.jsonl` 與輪替檔繼續保存

SQLite 以交易保存各類資料, 版本升級前產生 `.schema-vN.bak` 備份, 遷移失敗時回復交易. 資料格式與擴充方式見 [架構](docs/architecture.md#回補資料儲存)

### MCP 來源紀錄

MCP 呼叫依設定與 session 紀錄產生. 各來源的額外 telemetry 依其介面讀取, Jev 是其中一個已支援的來源. Jev telemetry 需要含 `jev_telemetry.py` 的 client. 以下操作修改 `CODEX_HOME/monitoring/jev-monitor.json`, Jev CLI 與 MCP 共用此設定:

```sh
local-activity-monitor --enable-jev --configure-only
local-activity-monitor --codex --open
```

已有 Jev MCP 程序需重新載入新版 client. 停用紀錄使用 `--disable-jev --configure-only`, 已有資料保留. 資料庫預設位置:

- Windows: `%LOCALAPPDATA%/local-activity-monitor/state/jev.sqlite3`
- macOS: `~/Library/Application Support/local-activity-monitor/state/jev.sqlite3`
- Linux: `$XDG_DATA_HOME/local-activity-monitor/state/jev.sqlite3`, 未設定時使用 `~/.local/share`

## 開發與建置

先執行 `python tools/build_frontend.py`, 再檢查 checkout 來源

Windows PowerShell:

```powershell
$env:PYTHONPATH = "src"
python -m unittest discover -s tests -v
```

macOS / Linux:

```sh
PYTHONPATH=src python3 -m unittest discover -s tests -v
```

JavaScript 檢查:

```sh
node --check frontend/app.js
node tests/test_runtime_ui.mjs
node tests/test_page_metrics.mjs
node tests/test_usage_projection.mjs
```

建置工具列在 `tools/requirements-build.txt`. `python -m build` 產生 wheel / sdist, `python tools/build_release.py` 產生 `.local/package-tests/` 的本機原生測試包. 平台 CI 使用已提交的 WBUI SHA, 各平台通過測試與服務啟動檢查後保存 artifacts 及 `SHA256SUMS.txt`

0.x 每個新 minor 建立一個 `v0.x.0` tag. 1.0.0 起需明確啟用 workflow 的 `publish`, 產物先附加至草稿 Release, 核對後公開

## 文件

- [AI 入口](AI.md): 依任務查找來源、固定 WBUI 版本、責任與 focused 驗證
- [功能清單](docs/features.md#已實作): 目前功能與資料範圍
- [使用說明](docs/usage.md): 圖表、表格、明細與設定
- [設定檔格式](docs/settings-format.md)、[卡片庫](docs/card-library.md): 欄位、驗證、圖表與自訂卡片
- [文件索引](docs/README.md): 資料範圍、架構、維護及驗證入口

## 授權

LAM 原始碼與文件採 [MIT License](LICENSE). 內附 Workbench UI 與原生套件元件的授權條件見 [NOTICE](NOTICE), WBUI 資產依持有人對 LAM 的授權提供

歷史預設保存 90 天, 可在設定調整保存天數, 0 表示不自動刪除. 資料庫保存量與畫面 / 查詢筆數上限分開, 來源依輪替游標分批回補, 右上角顯示活動紀錄保存期限, 回補進行時顯示進度

Release 內的程式與前端產物由建置流程產生, 不包含使用者資料、暫存檔或監測資料庫. 歷史固定保存於 `CODEX_HOME/monitoring`, 更新程式版本不搬移或重設資料. SQLite 格式升級前備份, 不支援的格式保留原檔並回報無法讀取. 備份執行中的 WAL 資料庫應使用 SQLite backup API, 不只複製單一資料庫檔案, 一般啟動不自動 VACUUM
