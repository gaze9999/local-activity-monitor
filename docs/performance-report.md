# LAM 效能與回收測試報告

日期 2026-10-07, 整理本輪已執行的隔離合成資料測試, 功能 source 為 `64269e3` 之後的工作目錄, 發布版本 0.6.0. 不讀真實使用者資料, 不重新執行本機封裝或停止其他對話的自動化

## 本輪直接證據

| 項目 | 實際檢查 | 能證明的範圍 |
| --- | --- | --- |
| 初次 / 更新 / 恢復 | `tests/loading-layout-flow.cjs`, cold load、503、unknown / 0 / false、重試 | 保留既有內容, skeleton / 光帶不重複, 不以 0 代替缺值 |
| 緊湊布局 | 同 flow, 9 張卡、1600 / 820 / 390 px | 無重疊, 來源尾端保持全寬, 非全部頁面長時間布局證明 |
| 分布圖 / 光帶 | `tests/distribution-feedback-flow.cjs` | 可點 / 唯讀樣式一致、原始節點保留、遮罩字級與動畫相位一致、導頁 tooltip 關閉 |
| 字級 / 字型 | `tests/font-resize-flow.cjs`, 12–18 px, 3 字型 × 4 視窗 | 12 組排列及實際設定保存, 不代表 Safari / iPad |
| 內容量與清理 | `tests/output-detail-flow.cjs`, 702 行 diff / 完整原文、分頁 / 中止 / 關閉 | bounded 載入及回應取消, 沒有取得完整應用的 GC / heap 長期樣本 |
| 資料解析 | 歷史 Python focused 38/38 與 SQL / YAML / Git / Markdown 操作, 發布前最小檢查 50/50 | 回應 metadata、內容路由與格式 / 隱私邊界, 不含本機封裝測試, 不代表 collector 或 API 耗時改善 |

共用元件另有 10 萬筆 table、25 萬筆 pivot、5000 選項、日曆 / 圖表、CPU / heap 與重複 destroy 的直接樣本, 見 [Workbench UI 效能報告](https://github.com/gaze9999/workbench-ui/blob/v0.3.0/docs/performance-report.md). 該報告需要共用儲存庫存取權, 元件數據不能當成 LAM 全站效能

## 回收責任

Workbench UI 清理自己建立的 DOM、事件、observer、frame / timer 與 Worker. LAM 必須在 Tab / modal / 明細生命週期結束時 destroy 控制器並中止未完成要求, 舊回應不得覆寫目前畫面. 使用端保存的資料陣列、控制器、訂閱與快取另有 owner, 關閉 DOM 不等於釋放所有參照

README 已列目前工具紀錄、未完成片段、效能樣本及執行事件的資料上限. 1.0.0 前需逐一驗證裁切 / 保留天數、游標輪替、刪除 / 移動來源、HTTP 內容快取、延後 renderer 及明細快取, 核對最大容量、TTL、取消與例外清理

## 尚無可比樣本

本輪沒有 LAM 同條件的初次載入 / 更新 / 歷史回補 p50 / p95 / p99、程序 CPU / RSS、GC 後 heap、detached DOM、完整 paint / GPU 或正式 CLS. 沒有這些基準時不宣告 peak 已降低, 不從其他網頁自動化的 CPU / 記憶體扣除後估計真正效能

後續以固定來源大小、API 更新頻率、WB revision、視窗、字級 / 字型與動畫設定進行配對量測, 先記 cold / warm / update / cancel / close 各階段, 再比較結果. Shadow DOM 與跨引擎仍是獨立驗收, 本輪未測

一般使用者不需 Playwright. 可在既有 Edge / Chrome 的 Performance / Memory 面板依序記錄開頁、更新、切 Tab、開關明細及字級變更, 註記資料規模、瀏覽器版本與同時執行的工作負載, 保存 trace / heap snapshot. 公開分享只使用合成資料, 真實活動 trace 留在本機

已有 Playwright CLI 的維護者可用各 flow 的隔離伺服器與命令, 見 [驗證紀錄](validation.md). 本報告不代表 1.0.0 已完成, 發布產物的原生 CI 啟動檢查也不取代真實平台效能驗收
