# 測試與建置

依[維護與驗收](maintenance.md)選擇受影響的檢查, 使用獨立的合成資料與測試設定

| 檢查範圍 | 執行入口 |
| --- | --- |
| Python 行為 | 在儲存庫根目錄執行 `python -m unittest discover -s tests` |
| 前端互動 | 依修改範圍選擇 `tests/*-flow.cjs` 的對應流程, 核對載入、更新、失敗恢復與實際介面 |
| 離線資產 | 執行 `python tools/build_frontend.py --verify-offline`, 依 [workbench-ui.json](../workbench-ui.json) 核對固定的 WBUI revision |
| 套件與平台 | 依[發行工作流程](../.github/workflows/release.yml)核對建置、隔離啟動及產物 checksum |

公開驗證結果記錄受測 revision、平台、相依版本、合成資料條件、執行方式、觀察結果與涵蓋範圍

## 2026-10-24 固定版本資產校正

重建時比對封裝資產與固定 WBUI revision 的 Git blob, 避免工作目錄的 CRLF / LF 轉換改變封裝內容, 核對方式見[維護與驗收](maintenance.md#共用-ui-與按需內容)
