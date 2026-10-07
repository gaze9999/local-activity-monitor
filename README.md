# Local Activity Monitor

監測本機 Codex、Jev 與 MCP 活動的瀏覽器介面, 提供對話、用量、工具、專案及診斷明細. 後端使用 Python 3.10+ 標準函式庫, 前端使用原生 HTML / JavaScript / CSS 與 Workbench UI

目前版本 `0.7.0`. 0.x 以提交與 minor 版本 tag 交付, 建置包保存在 [Actions artifacts](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml). GitHub Release 從 1.0.0 起提供

## 功能

| 主頁 | 內容 |
| --- | --- |
| 總覽 | 可選擇及排序的圖表、帳戶用量與活動摘要 |
| 用量與額度 | 模型 Token、帳戶額度剩餘比例、重設時間與 credits |
| 對話 | 對話列表、子代理程式、本機排程與 My dots |
| 專案 | 專案列表、Git 操作、工作樹與驗證活動 |
| 工具 | 呼叫與耗時、MCP、技能與 Plugins |
| 資料操作 | 網路參考、檔案操作與 SQL 紀錄 |
| 錯誤與 Log | 錯誤分類、來源 Log 與關聯操作 |
| 監測程式 | 收集進度、CPU 時間、讀取量、硬體與頁面效能 |

介面支援繁體中文、English 與日本語, 提供深淺模式、字型、強調色、圖表形式、篩選、分頁及拖曳排序. 偏好設定可匯出與匯入 JSON, 操作明細按需載入

## 從原始碼啟動

需要 Python 3.10+. 首次取得 private 的 Workbench UI 來源時, 需要 Git 與該儲存庫的既有存取權限

```sh
git clone https://github.com/gaze9999/local-activity-monitor.git
cd local-activity-monitor
```

| 系統 | 啟動方式 |
| --- | --- |
| Windows | `launch-cli.cmd` 或 `launch-cli.ps1` |
| macOS | 雙擊 `launch-cli.command` 或執行 `sh launch-cli.sh` |
| Linux | `sh launch-cli.sh` |

入口優先使用儲存庫內的 `.venv`, 其次使用已安裝的 Python, 自動建置頁面並開啟 `http://127.0.0.1:8787/`. 終端按 Ctrl+C 停止服務. 可加入 `--port 8790` 指定連接埠, `--codex-home` 指定 Codex 資料目錄

相同連接埠及 `CODEX_HOME` 已有 LAM 時, 新入口會開啟既有服務. 其他程式占用連接埠時會顯示錯誤. macOS 入口需要執行權限時, 執行 `chmod +x launch-cli.command`

缺少 Python 時可使用 `launch-cli.cmd --install-python` 或 `sh launch-cli.sh --install-python`, 確認後透過 Python Install Manager / winget、Homebrew 或 apt 安裝

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

左上角顯示 LAM 與實際載入的 WBUI 版本. Watcher 監看 Python 與 `frontend/` 修改, 建置成功後重啟自己建立的服務, 建置失敗時保留執行中的服務

本版固定 WBUI 0.4.0 的已確認 commit. 自動更新繼續選擇正式 tag, 版本低於目前已建置版本時沿用原版. 明確串接其他 revision 可使用 `python tools/build_frontend.py --revision <完整 SHA>`, 建置成功後才更新版本 pin

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
| `windows-x64-cli.zip` | 解壓縮後使用 `launch-cli.cmd`、`launch-cli.ps1` 或 `launch-cli.exe` |
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

主設定的「官方帳戶查詢」預設關閉. 啟用後透過已登入的 Codex CLI 取得方案、額度與帳戶用量, 查詢結果至少快取 60 秒. 雲端 Work、My dots 與排程清單尚未串接

一般更新收集選定 metadata, 完整 Git、SQL、工具輸入輸出、Log 與技能文件在開啟明細時受限讀取並遮蔽 credentials. 服務使用 loopback 與 Host / Origin 檢查. 讀取範圍、保存期限與上限見 [資料盤點](docs/data-inventory.md)

### 回補資料

`CODEX_HOME/monitoring/lam-history.sqlite3` 保存 SQL / 網路 / MCP 活動、錯誤摘要、已確認對話狀態與技能讀取紀錄. 首次讀取會匯入既有 `activity-history.json`、`error-history.json` 與 `thread-state.json`, 原檔保留, 後續以 SQLite 資料為準. 程式記錄檔 `local-activity-monitor.jsonl` 與輪替檔繼續保存

SQLite 以交易保存各類資料, 版本升級前產生 `.schema-vN.bak` 備份, 遷移失敗時回復交易. 資料格式與擴充方式見 [架構](docs/architecture.md#回補資料儲存)

### Jev 紀錄

需要含 `jev_telemetry.py` 的 Jev client. 以下操作修改 `CODEX_HOME/monitoring/jev-monitor.json`, Jev CLI 與 MCP 共用此設定:

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

- [使用說明](docs/usage.md): 圖表、表格、明細與設定
- [程式架構](docs/architecture.md): 資料流、模組、API 與擴充
- [設定檔格式](docs/settings-format.md): 欄位、驗證與相容性
- [資料盤點](docs/data-inventory.md)與[錯誤觀察](docs/error-observation.md): 來源、範圍與資料意義
- [卡片庫](docs/card-library.md): 可用圖表形式
- [維護與驗收](docs/maintenance.md)、[測試與建置](docs/validation.md)與[效能與資料上限](docs/performance-report.md): 開發流程及資源管理

## 授權

LAM 原始碼與文件採 [MIT License](LICENSE). 內附 Workbench UI 與原生套件元件的授權條件見 [NOTICE](NOTICE), WBUI 資產依持有人對 LAM 的授權提供
