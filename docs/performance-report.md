# 效能與資料上限

## 分批收集

服務先提供 loopback 頁面與等待狀態, 單一 worker 依序整理 session、統計、工作樹、帳戶、診斷、歷史、MCP、Jev 與硬體資訊. 快照 API 使用最近完整結果, 收集取消或失敗時保留上次快照

Session 清單每 15 秒更新, 預設追蹤 20 個近期檔案, 上限 5000. 初次讀取頭行與最多 1 MiB 尾端, 後續增量讀取. 每輪共用 8 MiB 預算, 使用輪替游標處理多檔及歷史回補

完整輸入輸出、SQL、Log、Git、技能文件與檔案 metadata 透過明細 API 按需取得. 同一內容以 revision 核對後續分頁

## 畫面更新

前端更新目前頁面與已選總覽卡片. 圖表接近可視範圍後逐張繪製, 新快照及 Tab 切換取代舊排程. 圖表設定在第一次開啟齒輪時建立

WBUI 共用卡片控制器負責布局, LAM 更新可見項目並在移除容器或離開頁面時釋放. 背景更新保留已開啟明細、捲動位置及草稿

瀏覽器隱藏時暫停畫面輪詢. Codex 閒置預設五分鐘後保留輕量活動檢查, 新活動出現時恢復完整整理

## 保存上限

| 資料 | 上限 |
| --- | --- |
| 工具 Call ID | 每檔 8192 筆, 全部 50000 筆 |
| 未完成行 | 每檔 1 MiB, 合計 8 MiB |
| Lifecycle checkpoint | 1000 項 |
| Skills checkpoint | 500 筆 |
| Thread checkpoint | 合計 512 KiB |
| SQL 活動 | 500 筆 |
| 網路 / MCP 活動 | 各 1000 筆 |
| 活動快取 | 合計 1 MiB, 預設 7 天, 可調整 1 - 365 天 |
| 效能樣本 | 360 筆 |
| 執行事件 | 200 筆 |
| 程式 Log | 1000 筆 |
| 錯誤 checkpoint | 1000 筆及 512 KiB |
| 圖表樣本區間 | 240 個 |

回補資料使用 SQLite, 各分區的大小上限以投影資料計算, 不包含 SQLite 頁面與索引. 寫入以交易完成, 未變更的資料略過重寫. 來源資料庫仍使用唯讀連線

來源讀取及 API 上限見 [資料盤點](data-inventory.md)

## 頁面指標

| 指標 | 意義 |
| --- | --- |
| TTFB | Navigation Timing 的 responseStart |
| FCP | 首次進入背景前的 first-contentful-paint |
| LCP | 首次進入背景前的最後 largest-contentful-paint 樣本 |
| CLS | 排除近期輸入後, 間隔小於 1 秒且總長小於 5 秒的最大位移群組 |
| 首次資料呈現 | 首批資料更新 DOM 後的下一個畫面回呼時間 |
| 長任務 | PerformanceObserver 的 longtask 數量與最大耗時 |
| 最慢互動 | Event Timing 中 interactionId 大於零的最大 duration |

指標保留於目前頁面, 重新載入後重算. 缺少樣本或瀏覽器 API 時顯示缺值. 後端另記錄整理耗時、程序 CPU 時間、讀取量與回應大小
