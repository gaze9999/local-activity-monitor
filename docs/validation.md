# 測試與建置

## 原始碼檢查

需要 Python 3.10+ 與已準備的 WBUI 資產. 建置頁面後以 checkout 的 `src` 執行測試

Windows PowerShell:

```powershell
python tools/build_frontend.py
$env:PYTHONPATH = "src"
python -m unittest discover -s tests -v
```

macOS / Linux:

```sh
python3 tools/build_frontend.py
PYTHONPATH=src python3 -m unittest discover -s tests -v
```

測試使用暫存 metadata、SQLite 與來源 fixture, 涵蓋資料投影、增量讀取、checkpoint、內容遮蔽、來源快取、HTTP 邊界、啟動及建置回復

`test_history_store.py` 檢查分區共用、舊資料匯入、版本升級備份、遷移失敗回復、寫入鎖定及不支援格式. `test_process_lifecycle.py` 檢查終止訊號、父程序控制串流關閉、Windows 隱藏子程序清理與資料保留

JavaScript 檢查需要 Node.js 22+:

```sh
node --check frontend/app.js
node tests/test_runtime_ui.mjs
node tests/test_page_metrics.mjs
node tests/test_usage_projection.mjs
```

## 瀏覽器流程

`tests/serve_loading_fixture.py` 建立隔離的合成 Codex home、session、SQLite 及工具紀錄, 啟動後輸出 loopback URL

```sh
python tests/serve_loading_fixture.py
```

各 `.cjs` 檔匯出接受 Playwright `page` 的流程, 對上述 fixture 執行:

| 流程 | 內容 |
| --- | --- |
| `loading-layout-flow.cjs` | 初次載入、延後更新、503、重試、缺值、零值、帳戶及卡片布局 |
| `appearance-layout-flow.cjs` | 主題、強調色、字型及偏好保存 |
| `font-resize-flow.cjs` | 字級、字型與視窗縮放 |
| `distribution-feedback-flow.cjs` | 分布圖、讀值及更新狀態 |
| `output-detail-flow.cjs` | 原文、內容分頁、取消與明細清理 |
| `parser-summary-motion-flow.cjs` | 結構解析、摘要、減少動畫及設定匯入 |

## 套件建置

建置相依列在 `tools/requirements-build.txt`, 專案 package metadata 由 `pyproject.toml` 管理

```sh
python -m pip install -r tools/requirements-build.txt
python tools/build_frontend.py
python -m build
python tools/build_release.py
```

Wheel 與 sdist 輸出至 `dist/`, 原生測試包位於 `.local/package-tests/`. 原生包由對應作業系統與處理器建置, `tools/smoke_release.py` 以暫存 home 及隨機 loopback 連接埠核對頁面、CSP、語系、版本、snapshot 與檔案存取邊界

## 平台 CI

[Build CLI packages](../.github/workflows/release.yml) 由 workflow_dispatch 啟動, 建置 Windows x64、macOS x64 / ARM64、Linux x64 / ARM64

每個平台先取得 `workbench-ui.json` 的完整 SHA, 準備 `_workbench/` 並建置 `_web/`, 再執行 unittest、原生建置與服務 smoke check. Source job 另產生 wheel、sdist 與含離線資產的原始碼 zip

所有套件通過後產生 `SHA256SUMS.txt`, 檔案保存在 Actions artifacts. 0.x 使用 minor tag, 1.0.0 起可透過 `publish` 附加至草稿 Release. 版本與下載紀錄可從 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml)查詢
