# 測試與建置

目前 LAM 為 0.20.4, 固定 WBUI 0.20.0 / `f79ca1c`, 最新證據見 [2026-10-24 資產與隔離操作](#2026-10-24-固定版本資產校正)
以下各節保留當時的版本、環境與結果, 完整平台及使用端驗收依 [功能清單](features.md#待驗收) 追蹤

## 0.20.0 來源交付檢查

0.20.0 來源候選, 固定 WBUI 0.15.0 / `87257e7291e56aadb0ce6786346ff761158e4985`, 本次包含 LAM 程序使用量、歷史表格、Context 載入與顯示設定, 私人 Codex 工作規範、角色及環境設定由 Setup 保存

- Python 3.12.14 完整 unittest 執行 533 項, 528 項通過、2 項失敗、3 項略過. 啟動記錄測試因 PowerShell 長路徑訊息換行而失敗, 使用短暫存路徑複查 8 項通過
- Watcher 的 Python HTTP 讀取仍出現連線重設, 同一隔離服務的 curl 請求成功. 以 curl 的實際 HTTP 讀取執行相同生命週期斷言通過, 涵蓋重複啟動、來源更新、更新失敗恢復、建置取消及程序清理. Python HTTP 測試失敗仍保留, 未列為通過
- Edge 155.0.4283.45 的 11 組合成資料流程通過, 涵蓋歷史、Context、卡片、桌面中繼資料、表格欄位對齊、設定保存、Tooltip 及觸控手把, 未觀察到 pageerror, 故障回覆由測試 fixture 提供
- 固定離線資產建置與 checksum 核對、JavaScript 語法、runtime UI、頁面指標、使用量投影及本次文件 textlint 通過, 本輪未建置五平台原生包或驗收正式服務

## 2026-10-14 02:22 更新失敗恢復與內容判斷

Asia/Taipei, HEAD 0964f46 後的 0.18.0 候選, WBUI pin cd59922 / 0.14.0 保留

- 來源與資產載入成功後才暫停舊服務, 替代服務監聽成功後才結束舊程序. 載入失敗保留執行中服務, 啟動失敗恢復同一舊程序、連接埠及已載入狀態, 修復來源後可再次更新. 內部控制訊息不增加 HTTP 或公開 CLI 介面
- signature 依來源內容 SHA-256 判斷變更, 相同內容重新存檔不重啟, 相同大小及修改時間但不同內容仍可辨識, 內附產物繼續排除
- Python 3.14.7 的實際 watcher 隔離流程通過來源載入失敗、啟動失敗恢復、修復後更新、相同內容修改時間、重複啟動、建置失敗與建置中取消, 正常退出 0 並釋放連接埠. Watcher 單元 14 項及啟動器 12 項通過, 固定離線驗證通過
- 既有 ctypes 回歸 fixture 改用指標型別建立呼叫檢查, 暖機後連續取樣 100 次不建立新的指標型別, 8 項效能診斷測試通過. 原私有快取計數在 Python 3.14.7 不支援 len, 監測功能未修改
- 同一 Python 3.14.7 的完整來源回歸共 524 項, 521 項通過、3 項略過, 其中符號連結權限與標準 HTTP 大型回應基準的環境缺口保留, 略過不計為通過. 固定 pin 與資產 checksum / revision 未變
- 原 Python 持續新增回報仍未重現為 LAM 所屬程序增長, 此結果只涵蓋有界來源建置與服務更新生命週期. 未啟動正式 8787、查詢帳戶或建立原生可攜包
- 本批 8 份修改文件的本機 textlint 與人工語意複核通過, diff 空白檢查通過. 測試建置採合成封裝工具, 不計為原生包驗收

## 2026-10-14 02:02 後續錯誤重現

Asia/Taipei, HEAD 0964f46 與 01:42 未提交候選, 本輪僅查核與記錄, 未追加實作

- 隔離副本使用 Python 3.14.7、空 home 與動態連接埠, 原服務已監聽後令 server.py 在載入時失敗. 前端建置成功, watcher 先停止舊服務再啟動替代程序, 最後因替代程序退出而結束為 1, 原連接埠已關閉. 此重現屬來源更新失敗, 與原「無限新增 Python」回報分開
- 不改來源位元組、只更新 mtime, signature 仍改變. 來源確認目前以 mtime / size 觸發重建及服務重啟, 相同內容重新存檔缺少抑制
- 前者優先補來源載入檢查與替代服務啟動失敗的恢復, 後者以內容判斷實際變更. 未啟動正式 8787, 自建程序與隔離副本已清理
- 本節新增文案採本機格式檢查與人工語意複核

## 2026-10-14 01:42 啟動與 watcher 控制管線修正

Asia/Taipei, main HEAD 0964f46 後的未提交來源, LAM 0.17.0, WBUI pin cd59922 / 0.14.0 保留

- Python 3.14.7 的隔離重現中, 父程序讀取控制輸入時, 繼承同一輸入的子程序在 3 秒內未完成, 改用獨立輸入後正常完成. 來源建置改用 DEVNULL、180 秒上限及取消檢查, Windows 建置程序沿用 Job Object 保護所屬子程序
- watcher 排除 _workbench / _web 內附 Python 產物, 固定 pin 與實際來源仍觸發更新. 建置失敗保留執行中服務, 不因相同失敗狀態反覆啟動, 控制輸入關閉可取消建置並正常停止
- 原啟動器 Python 的 53 項針對性檢查通過: watcher 10、啟動器 12、實際 watcher 生命週期 1、程序生命週期 4、前端建置 18、啟動 Log 8. 實際生命週期使用複製來源與空 home, 核對來源更新換成新服務、重複啟動重用同一服務、失敗保留、修復及建置中取消退出 0, 最後測試連接埠已釋放
- 固定離線建置通過, 未變更使用者設定或 WBUI pin. 本輪沒有啟動正式 8787、原生封裝或 Git 交付, Codex 管理的 MCP 程序未受清理影響, 原「無限新增」回報未單獨重現為 LAM 所屬程序持續增長

## 2026-10-13 21:09 前景後端更新

使用原 launch-cli.cmd 與原 codex home 啟動可見前景程式, 後端 snapshot 讀回 0.17.0, 頁面使用 WBUI 0.14.0

- 原 home 識別一致, 重啟前後核對的 5 份既有檔案 hash 未變, 原設定沿用
- Edge 正式頁面七個可見主要頁籤切換通過, 390 / 320 px 無整頁橫向溢出, 無 pageerror 或設定寫入請求, 測試瀏覽器關閉後前景服務持續執行
- 此結果取代先前「正式後端仍為 0.16.0」的目前狀態, 先前受測版本與待查程序增長問題保留各自紀錄

## 2026-10-13 19:47 WBUI 0.14.0 本機同步

LAM 0.17.0 來源 c9598a5 沿用既有 Store 串接, pin 與離線資產更新至 WBUI 0.14.0 / `cd599225d16715f8d6b7069802db85874b8e15a4`

- Python 3.12.10 的 18 項前端建置 / 保留既有資產 / 防降版測試與固定離線相容性檢查通過, 兩組 manifest 與來源 bytes 一致
- Edge 合成資料的 files-navigation、output-detail、loading-scope 三個流程通過. 1,000 筆資料保留完整來源, 每頁掛載 10 列, 核對 Enter / Space、搜尋、換頁、列 DOM / 焦點 / 選取保留及 900 / 390 / 320 px. 先前列 DOM 保留失敗已由新版來源修正, 本次重驗通過
- 工具、MCP、Git、錯誤與 Markdown 輸出、取消請求及 modal 資源清理通過, 載入範圍保留內容與焦點, 無 pageerror
- 正式 8787 頁面讀回 WBUI 0.14.0, snapshot 的 monitor version 仍為 0.16.0, 來源為 0.17.0. 使用者指定的前景程序保留, 本次未手動重新啟動, 不將頁面資產更新當成後端版本載入

## 2026-10-13 01:25 0.17.0 來源交付檢查

Asia/Taipei, 0.17.0 來源候選, 固定 WBUI 0.13.0 / `9c6b2898c5f959d948e2ed5ac5465227158b28e7`. 此 minor 記錄 Store 訂閱遷移、最低共用庫版本及離線封裝相容性要求, 既有 v0.16.0 保留原提交

- Python 3.12.14 的來源套件與 runtime 版本同為 0.17.0, 固定離線建置驗證通過. 完整 unittest 共 515 項, 512 項通過、3 項略過, 兩項因本機無法建立 symlink, 一項因標準函式庫的大型 HTTP 基準逾時, 該環境下的大型應用本文傳輸仍未驗收
- JavaScript 語法、runtime UI、頁面效能與 usage projection 檢查通過. 本次版本調整未改動已驗收的 app / WBUI 資產, 沿用上述固定來源的四個 Edge 流程, 不將前一節 0.16.0 原生候選當成 0.17.0 封裝證據
- 前景服務仍回報 0.16.0, source watcher 所屬的資產建置程序停滯, 以相同 Python 3.14.7 獨立執行固定離線建置成功, 原因尚未確認. 此現況與 Python 持續增加的回報分開追蹤, 來源 / Tag 交付不代表正式服務已載入 0.17.0

## 2026-10-13 01:12 Windows 可攜包與正式前景服務

Asia/Taipei, LAM 0.16.0 / main HEAD cef4072 後的未提交來源, 固定 WBUI 0.13.0 / `9c6b2898c5f959d948e2ed5ac5465227158b28e7`. 使用者授權隔離安裝建置工具、Windows 可攜包驗收及正式服務重啟, 並指定最後以前景開啟 launch-cli.cmd

- 隔離環境使用 Python 3.12.14、專案鎖定的 PyInstaller 6.22.3 與 build 1.6.1. 本機候選透過既有 build_release.py 建置, 未使用正式發布模式, ZIP 含 121 個項目, SHA-256 為 `378bb23864dc99030f975549ab7aa18fcc3fcdd8b23505d2545fe48c5435d1e4`, 不代表正式 Release
- 原生 smoke 檢查通過 HTTP、內附資產、0.16.0 版本、空 home、CSP、圖示及來源檔案存取邊界. 搬移至含繁體中文及空白的路徑後, 移除開發來源與 Python 路徑覆寫, 核對 WBUI / 頁面 manifest、checksum 與必要授權檔, 選定的使用者設定、憑證及資料庫檔名未包含於候選 ZIP
- 搬移後以隔離空 home 執行內附 launch-cli.exe, Edge 155.0.4283.45 確認 LAM 0.16.0 / WBUI 0.13.0、七個可見主要頁面及 390 / 320 px 無整頁橫向溢出, 無 pageerror. 關閉控制輸入後執行檔正常退出 0, 所屬程序及測試連接埠已回收
- 正式 8787 服務沿用同一 home 識別及監測設定, 以前景 launch-cli.cmd 載入 LAM 0.16.0 / WBUI 0.13.0. 唯讀 Edge 驗收讀到 856 個中繼資料對話, 七個主要頁面及 390 / 320 px 通過, served app / WBUI bytes 與已驗收來源相同, 無設定寫入、pageerror 或設定變更
- Python 持續增加的回報尚未重現. 第一組原啟動流程從 01:01 至 01:09 維持三個所屬 Python 程序, 另有一分鐘連續六次盤點一致, 重複開啟辨識 already_running 並退出 0. 關閉本輪啟動器後三個程序及 8787 均回收, 再從使用者確認的 launch-cli.cmd 前景開啟仍為三個, 新增的 MCP 程序歸屬 Codex 主程式. 本次未修改啟動器或關閉 Codex 管理的 MCP, 這項回報仍保留待重現狀態
- 自建封裝環境、搬移目錄、建置 staging 與暫存驗證程式已清理, 候選 ZIP、記錄檔及本機驗收報告保留. 指定的前景服務維持執行, macOS / Linux 原生驗收及 LAM Git / Release 交付仍未完成

## 2026-10-13 00:08 固定 WBUI 0.13.0 與離線資產同步

Asia/Taipei, 使用者授權交付 WBUI 來源及更新 LAM 固定版本. WBUI main 已提交並推送 `9c6b2898c5f959d948e2ed5ac5465227158b28e7`, 遠端完整 SHA 已核對, 本次未建立 Tag / Release. LAM main HEAD 仍為 cef4072, 本輪及既有修改保留未提交

- 透過共用建置 helper 指定上述完整 revision, 先驗證既有資產, 再原子同步 workbench-ui.json、WBUI 離線資產及 LAM 頁面. 四份共用資產與來源位元組一致, JS / CSS 的 SHA-256 前 16 字元為 2b169d04a0831404 / 4d0f6ed20add226f, app.js 仍為 f69e3ecd11699438
- 離線相容性、checksum、pin 與頁面 revision 檢查通過, 開發選擇器在完整同步後移除. 清除測試程序的 WORKBENCH_UI_PATH 後, 預設 loader 使用 LAM 內附 _workbench 目錄, 不需要相鄰 checkout
- Python 3.12.14 的前端建置 18/18、UI 資產 7/7、模擬原生封裝流程 3/3 通過. 第二次固定來源建置確認無須重建, 未執行原生發布包建置
- Edge 155.0.4283.45 的 files-navigation、presentation-settings、output-detail、loading-scope 四組隔離流程通過, 實際 runtime 為 0.13.0. 包含三語系、320 / 390 / 1366 px、列 / 焦點 / 選取保留、設定重載、1,000 筆虛擬資料與明細取消 / 清理, 未觀察到 pageerror
- AI 入口、使用、架構、功能、維護及效能文件同步目前固定來源, 歷史量測保留原受測版本. 本次未手動重啟正式服務或修改 Universe Idle / Sites, 正式執行與原生包驗收仍屬後續工作

## 2026-10-12 22:47 Store 遷移的載入與封裝檢查

Asia/Taipei, main HEAD cef4072, 未提交候選, 前端與 WBUI 來源沿用下節已驗收的 hash

- `ui_assets.py` 在開發及離線載入時確認 WBUI 0.13.0 以上, 舊版、預覽版及無效版本資訊明確拒絕. `build_frontend.py --verify-offline` 在替換資產前檢查相容性, `archive_source.py` 另核對資產 / 頁面 / pin revision, 原生封裝及兩個發布工作流程皆採用此檢查
- Python 3.12.14 的前端建置 18/18、UI 資產 7/7、封裝流程 3/3 通過. 使用隔離資料驗證正常建置、相容候選、舊版拒絕、修改後的失敗保留、revision 不符、checksum 損壞與來源封裝保留, 原生封裝測試模擬建置工具, 不代表跨平台執行檔已建置
- 實際本機開發載入 0.13.0, 對目前 0.12.0 離線資產執行封裝檢查時按預期失敗, pin 與兩份資產 manifest 的 SHA-256 維持一致, 未產生正式封裝
- Edge 155.0.4283.45 的 files-navigation 與 loading-scope 合成流程通過, 核對 320 / 390 px、熱度開關、列 / 焦點 / 選取保留、容器 / 卡片載入及統計缺值. WBUI reactive-motion 補驗通過插入 / 移除、焦點 / 選取、快速更新、動畫停用、表格欄對齊與清理
- 修改包含 `src/local_activity_monitor/ui_assets.py`、三份建置 / 封裝工具、發布 workflow、三份回歸測試及 AI / 維護文件, workflow YAML、兩個封裝檢查入口、兩庫 diff 空白及四份文件的本機 textlint / 人工語意複核通過. 本次未執行 Git 交付、手動重啟正式服務或修改 Sites / Universe Idle, 固定 pin 及離線發布仍待 WBUI 來源交付

## 2026-10-12 22:13 本機表格修正與 WBUI Store 遷移

Asia/Taipei, LAM HEAD cef4072 後的未提交來源, app.js SHA-256 前 16 字元為 f69e3ecd11699438. WBUI 固定 pin 保留 0.12.0 / 03c10244, 既有開發選擇器直接提供本機 0.13.0 候選, JavaScript SHA-256 前 16 字元為 2b169d04a0831404. 開發選擇器的 revision 是 pin 基準, 不是未提交資產的固定修訂

- 列比對原先把 tooltip 處理後移除的 title 視為資料改變, 造成相同紀錄重建. 現在依實際提示語意比對並排除衍生的 heat-cell 樣式, 保留資料變更判定與操作回呼更新
- snapshotStore 及 tableRowStores 遷移為 subscribe(options, listener), 由共用 Python Assets 載入本機 WBUI src, 沒有手工複製候選到正式離線資產
- presentation-settings-flow 改用可見檔案排行的合成資料, 不再點擊隱藏的 Platform API 入口. 1,000 筆資料初始掛載 17 列, 捲動、排序與停用後還原完整列通過
- 原生 Python 的前端建置 16/16、UI 資產 6/6 通過, JavaScript 語法及 runtime UI / page metrics / usage projection 通過. 正式 pin 的取得、失敗保留與防降版檢查維持原規則
- Edge 155.0.4283.45 通過 files-navigation、presentation-settings、table-stream-interaction、partial-refresh、output-detail、loading-scope 六組合成流程. 包含三語系、320 / 390 / 1366 px、熱度開關下的列 / 焦點 / 文字選取、設定重載、明細關閉 / 取消要求、載入範圍及統計零值 / 缺值
- 瀏覽器核對實際 WBUI runtime 為 0.13.0, gallery 數量及 LAM 合成檔案表格的 390 px 截圖已人工檢視, 沒有 pageerror. 本次沒有驗證真實帳戶、其他平台或發布包
- 文件同步目前 pin 與開發候選邊界, 固定 pin 更新及離線封裝需等 WBUI 來源提交後完成. 本次未執行 Git 交付或手動重啟正式 8787

## 2026-10-12 01:40 版本交付檢查

Asia/Taipei, 0.16.0 來源, WBUI 0.11.0 pin `7c77ffea621d65db422f164552f7750f96ee94dc`

- 固定 pin 建置、JavaScript 語法、差異空白與修改文件的 textlint 檢查通過, 同步修正 AI 入口及維護說明的舊版本敘述
- Python 來源測試共 511 項, 3 項依平台條件略過, 其餘通過, 帳戶投影測試改由現行 `ApiQueries` mock 結果驗證, 不呼叫真實帳戶
- Edge 合成資料的 loading-scope、hidden-api、retained-error、loading-layout、startup-preload、chart-copy、partial-refresh、idle-interaction 與 query-status 九個流程通過, 包含三語系及窄螢幕. 保留查詢流程僅在測試攔截頁面開啟入口, 該替換頁面的 CSP hash 不再適用, 正式入口與 CSP 保留
- 公開來源與可達歷史的指定憑證格式檢查未發現符合模式的內容, 測試、截圖及暫存資料使用隔離目錄, 正式服務未重新啟動, 本機測試不代表 macOS / Linux 原生執行檔已驗收

## 2026-10-11 21:51 共用載入範圍與統計規則

Asia/Taipei, HEAD `32ee725` / 0.15.0 之後的未提交修改, `frontend/app.js` SHA-256 前 16 字元為 `3d8623dfa7155a2c`. 沿用既有 WBUI 0.11.0 pin `7c77ffea621d65db422f164552f7750f96ee94dc`, 本輪未修改 WBUI 或 pin

- `frontend/app.js` 以 WBUI `createDataState` 共用容器 / 卡片載入呈現, 完整快照到達後自動切換. `frontend/style.css` 保留頁面標題及單一整區提示, 容器模式停用卡片內效果, 卡片內容、焦點與偏好保留, 已預載目錄可先查看
- 統計來源相容 `.wb-statistic-label` / `.wb-statistic-value` 與舊名稱, 百分比及不適合加總的表格欄位不提供總和. 四張監測 / 總覽原始效能圖及副本不顯示總數, 原來源 ID 保留, 不再提供的來源維持 `--`
- `tests/test_frontend_build.py` 使用受控的已安裝版本與有效 hash 測試穩定 Tag 數字排序 / annotated Tag, 防降版實作未變, 最後來源的 Python 建置測試 16/16 通過, 固定 pin 建置、JavaScript 語法及差異空白檢查通過
- 新增 `tests/loading-scope-flow.cjs` / `tests/hidden-api-flow.cjs`, Edge 隔離頁面核對一個整區提示、初次失敗 / 重試、三種寬度的切換 / 焦點 / 節點身分、表格有效樣本 / 最新缺值 / 執行耗時總和、三語系 API 隱藏及舊雲端分頁恢復. 每組 API 設定與間隔保留, 無七個 API 來源請求或 pageerror
- 合成整理樣本 1000 / 2000 / 3000 ms 的平均值為 2000 ms, P99 為 2980 ms, CPU 樣本 100 / 200 / 300 ms 的平均值為 200 ms, P99 為 298 ms, 四張圖皆為三個樣本 / 三個欄位. 真正執行耗時 1000 / 2000 ms 保留總和 3000 ms, 百分比來源序號仍為 0 / 1 / 3
- 既有 startup-preload / chart-copy / partial-refresh / retained-error / idle-interaction 流程通過. `tests/loading-layout-flow.cjs` 改為檢查新的整區提示位置後通過, 保留零值 / 更新高度 / 三種寬度版面檢查, 共八個瀏覽器流程通過. 整區及卡片載入截圖已人工檢視
- 正式 `8787` 由既有 PID `49928` 提供, 本輪未啟動或重新啟動. 唯讀瀏覽器核對 served app bytes 與本輪來源相同, WBUI 0.11.0、API 隱藏 / 保存設定 / 舊分頁恢復、360 筆實際樣本的三欄效能統計通過. 瀏覽器內模擬更新失敗 / 恢復保留圖表節點及卡片高度, 不代表外部服務故障已重現

## 2026-10-11 20:07 API 入口隱藏與時間統計

Asia/Taipei, HEAD `32ee725` / 0.15.0 之後的未提交修改, `frontend/app.js` SHA-256 前 16 字元為 `0fd78b4cce4f3b3b`. 目前工作目錄另有 WBUI pin `7c77ffea621d65db422f164552f7750f96ee94dc` / 0.11.0 的未提交更新, 本輪沿用現況, 未修改 pin 或 WBUI 原始碼

- `frontend/app.js` 暫時隱藏七個 API 來源的設定、官方帳戶卡、雲端對話分頁、公開 API 與 Platform 卡片. 保存的雲端分頁回到對話列表, 本機紀錄、Dots 與 MCP 本機查詢保留. 既有 API 啟用狀態、間隔與後端實作不改寫
- 資料整理耗時與整理 CPU 時間省略總數, 保留平均值、P99 與統計範圍. 工具與 SQL 等實際執行耗時保留加總. 總覽副本沿用來源規則, 自訂來源移除不再顯示的總數, 原有平均 / P99 / 樣本數來源 ID 不變, 舊總數選擇保留為缺值
- `docs/usage.md` / `docs/card-library.md` 說明時間統計, `docs/settings-format.md` 說明 API 欄位相容性與暫時隱藏, 本節與 `.local/task-context.md` 記錄驗證及現況

Edge 隔離頁面通過三語系、1366 px 深色及 390 px 淺色的六組 API 隱藏檢查, 未產生七個來源的查詢請求, 無 pageerror. 驗證已啟用的七個設定與 600 秒間隔未被改寫, 既有雲端分頁選擇安全回到對話列表, 監測設定與 Dots 仍可使用. 人工檢視設定與時間統計截圖, 未見 API 導覽或總數欄位

合成資料整理樣本 1,000 / 2,000 / 3,000 ms 顯示平均 2,000 ms、P99 2,980 ms 與 3 個樣本, CPU 樣本 100 / 200 / 300 ms 顯示平均 200 ms、P99 298 ms. 總覽副本與自訂樣本數來源 ID 驗證通過, 工具執行耗時保留總數 6,000 ms. 同一建置另通過 `chart-copy-flow` 與 `retained-error-flow`, 後者涵蓋三語系、明暗模式與 1366 / 390 / 320 px 的原有失敗效果

建置與 JavaScript 語法檢查通過. Python 前端建置測試 15/16 通過, `test_stable_tags_sort_numerically_and_peel_annotated_tags` 的假資料只提供至 0.10.0, 現行 pin 已為 0.11.0, 防降版規則保留現行 pin, 與測試的較舊 Tag 預期不同. 本輪未修改這項既有測試或版本選擇流程, 測試資料的版本假設仍待更新

20:09 核對隔離測試 61401 已無監聽, 測試瀏覽器與暫存檢查程式已清理. 正式 8787 另由 19:48 啟動的程序 PID 49928 提供, 上層控制程序 PID 46732, 不屬於本輪建立的程序, 保留該組程序, 取代本輪開始時沿用的服務關閉假設. 新行為只在隔離頁面驗證, 未宣稱正式服務已載入. 本輪未啟動或重新啟動正式服務, 未提交、推送或建立 Tag, 保留先前查詢修正與失敗外框的未提交成果

## 2026-10-11 19:44 保留資料的更新失敗效果

Asia/Taipei, HEAD `32ee725` / 0.15.0 之後的未提交修改, `frontend/app.js` SHA-256 前 16 字元為 `2f648f7707b37bb6`, `frontend/style.css` 為 `c508973a49c7baaa`. WBUI pin 維持 `cf13a9417d752001ec388ab74a0f0fddab238066` / 0.9.0

- `frontend/app.js` 由 LAM 保存卡片的更新失敗狀態, 已有資料時保留內容並隱藏可見錯誤文字, 另提供螢幕閱讀器狀態. 重試期間保留效果, 成功更新後清除, 初次失敗仍提供可見錯誤說明
- `frontend/style.css` 使用主題警示色、虛線與內側外框效果, 不增加卡片尺寸、不遮住圖表或降低資料對比, 不依賴動畫
- `tests/retained-error-flow.cjs` 檢查三語系、明暗模式、1366 / 390 / 320 px 與 24 px 字級的 18 組設定, 每組涵蓋 SVG 與長條圖, 比對卡片、標題及圖表尺寸與位置. 另檢查表格列保留、焦點、重試、恢復、初次失敗與減少動態效果
- `docs/usage.md` 補充顯示行為, 本節與 `.local/task-context.md` 記錄驗證及背景服務關閉的決策. 先前查詢修正與未提交成果保留

固定 pin 的 Edge 隔離頁面通過 `retained-error-flow`、`partial-refresh-flow` 與 `idle-interaction-flow`, 無 pageerror. 人工檢視 SVG 與長條圖的失敗外框畫面, 內容清楚且沒有失敗文字重疊. 既有兩個流程確認局部更新、焦點及下一次來源事件恢復. Python 前端建置測試 16/16、建置、JavaScript 語法與 diff 檢查通過

使用者已於 19:31 要求關閉背景執行, 正式 8787 維持停止, 本節是隔離頁面與原始碼驗證, 尚未在正式服務驗證新效果. 19:45 核對正式 8787 與自建測試 54814 均無監聽, 測試瀏覽器已關閉. 本輪未修改 WBUI、pin 或其他使用端, 未提交、推送或建立 Tag

## 2026-10-11 19:20 未查詢顯示與正式查詢修正

Asia/Taipei, HEAD `32ee725` / 0.15.0 之後的未提交修改, `frontend/app.js` SHA-256 前 16 字元為 `0296cde4ab00f0a2`, 正式 8787 提供的資產與來源 hash 相同. WBUI pin 維持 `cf13a9417d752001ec388ab74a0f0fddab238066` / 0.9.0

- `frontend/app.js` 隱藏尚未查詢的狀態, 查詢期間的背景更新保留 loading, HTTP 404 明確提示重新啟動 LAM. 官方帳戶結果與時間移入查詢工具列, 空白狀態不另顯示未取得文案
- `frontend/locales.json` 補齊服務未更新提示的英文與日文
- `tests/query-status-flow.cjs` 補齊初始隱藏、查詢期間背景更新、舊服務 404、恢復與帳戶工具列檢查
- 本節與 `.local/task-context.md` 記錄新的驗收與服務現況. 建置同步 `_web/app.js`、`_web/locales.json` 與 `_web/manifest.json`, 其餘頁面資產與 WBUI pin 沿用原版本

最新固定 pin 的 Edge 隔離頁面通過 `query-status-flow`、`api-query-flow` 與 `cloud-activity-flow`, 檢查初始隱藏、pending、成功 / 失敗、404 / 恢復、雲端分頁、MCP、Dots、三語系與 1366 / 390 / 320 px. 人工檢視未查詢畫面, 查詢按鈕與篩選保留, 未顯示尚未查詢. Python 前端建置 / 查詢排程 20/20 通過, JavaScript 語法與 diff 檢查通過

正式 8787 原先七個新查詢端點都回傳 HTTP 404. 核對發現舊啟動器的建置子程序自 2026-10-10 未結束, 單獨恢復後仍無法完成更新. 本次只重新啟動已核對的 LAM 控制程序與專屬子程序, 使用原有 Python、home、設定、連接埠與固定 pin. 新控制程序 PID 43820 的自動建置 / 服務更新已實測, 正式版本為 0.15.0

正式 API 的七項查詢均回傳 HTTP 200, OpenAI 狀態、Codex 版本、帳戶及雲端對話為 `health=ok`, 三個 Platform 來源為 `missing_credentials`. 同一正式頁面的七個查詢按鈕完成驗證, 雲端對話顯示空資料的成功結果, 帳戶查詢成功, Codex 為 0.162.1, OpenAI 的部分服務異常是來源結果, Platform 明確顯示未設定金鑰, 無 pageerror. 未取得金鑰的 Platform 資料與權限仍待驗收, 未將缺少設定當成查詢失敗

合成服務與測試瀏覽器均已關閉, 保留正式 8787 常駐. 本輪未提交、推送或建立 Tag, 未修改 WBUI 或其他使用端

## 2026-10-11 15:10 v0.15.0 版本核對

Asia/Taipei, `pyproject.toml` 與 `src/local_activity_monitor/__init__.py` 更新至 0.15.0. 版本更新後重新通過 84/84 Python 建置 / API / 本機 metadata / MCP 測試與 JavaScript Client Hints runtime 檢查, 四個建置前端檔案與來源 bytes 一致

`workbench-ui.json` 與離線 manifest 維持 cf13a9417d752001ec388ab74a0f0fddab238066 / WBUI 0.9.0, manifest hash 核對通過. 前端行為與下節 14:46 的受測來源相同, 本節未重跑全部瀏覽器流程

本次另修改 `pyproject.toml` 與 `src/local_activity_monitor/__init__.py`, 維護套件與執行版本. Cpt 交付範圍為提交、推送與 v0.15.0 Tag, 不包含 GitHub Release、原生套件或正式 8787 重啟. 跨平台套件與真實帳戶權限仍依下節列出的條件驗收

## 2026-10-11 14:46 查詢、顯示與明細驗收

Asia/Taipei, 本機 0.14.0 未提交來源, HEAD 1dbf722, workbench-ui.json 維持 cf13a9417d752001ec388ab74a0f0fddab238066 / 0.9.0. 共用候選以既有 WORKBENCH_UI_PATH 串接本機 WBUI 工作目錄, 另用固定 pin 隔離頁面核對備用方式, 不將候選寫入 pin

| 受測來源 | SHA-256 前 16 字元 |
| --- | --- |
| `frontend/app.js` | `bce4ad4f2477f2d2` |
| `frontend/style.css` | `43aeb30882453d35` |
| `frontend/index.html` | `14d02fe9c0ddc389` |
| `frontend/locales.json` | `b734a9eefa13d93c` |
| `src/local_activity_monitor/server.py` | `1ae280460fb2492c` |

| 檢查 | 實際結果與範圍 |
| --- | --- |
| 建置 / API / 本機讀取 | 最新來源 84/84 Python 通過, 包含前端建置 16、七個選用 API、間隔與快取、公開 API / Platform / cloud、桌面 metadata、MCP 文件與觀察. Dots window 過濾與原來源讀取上限通過, 零 / false / 未知保留. Client Hints、UA 備用與省略欄位的 JavaScript runtime 檢查通過 |
| 提示與明細 | 最新 tooltip-semantics-flow、instructions-layout-flow、detail-tabs-flow 通過. 純文字 / 無 HTML、入口焦點、原 aria-describedby 復原、hover 移入閱讀、manual Top Layer、巢狀 Escape、文件與概要合併、入口 16 px 間距、動態明細清空 / 捲動歸零、default read-only / 明確 Edit / Cancel / Save、草稿與 SHA 防護 |
| 查詢工具列 | 最新 query-status-flow、api-query-flow、cloud-activity-flow 通過, off 手動按鈕 / on 間隔、七個來源工具列、成功 / 失敗與時間相依、資料保留與恢復、cloud cursor / 空資料 / 分頁、Dots 固定本機快取 / 查詢期間切換範圍忽略過時結果 / 背景重繪保持 pending、MCP 固定本機文件. 390 px 的簡短失敗訊息維持水平, 三語系 x 三種寬度 / 字級 24 無卡片溢位 |
| 顯示設定 | 最新 appearance-controls-flow、presentation-settings-flow 通過, 主題 / 自訂色 / 最小字級、介面 / 程式碼設定、數值顯示、Tag / badge 群組與外框、九方位 / 鈴鐺 / 歷史、binding 匯入 / 無效資料 / 重載. 1,000 筆模型完整來源保留, 虛擬掛載初始 18 列, 深層捲動、排序與停用後完整還原 |
| 固定 pin 相容 | WBUI 0.9.0 的隔離頁面通過缺少 API 的設定停用、Intl 數值備用、middle-center 通知、公開查詢工具列與無溢位 / pageerror. 相同 tooltip-semantics-flow / query-status-flow 通過, 不要求使用端提前採用新共用元件 |
| 其他 focused 回歸 | table-column-flow、card-presentation-flow、settings-sources-flow、diagnostic-settings-flow、theme-api-flow 在最新建置來源通過, 包含活動排序 / 缺值備用、自訂摘要 / 表格總和平均最新值、卡片 / 群組排序、診斷偏好及主題色彩 |

隔離 fixture 啟動時核對四個前端檔案與來源 bytes 一致, 不一致時明確要求重新建置, 沒有改動正式服務的 runtime 載入方式

上述瀏覽器為 Edge 155.0.4283.45, 使用 serve_loading_fixture.py 的隔離 SQLite、合成對話 / MCP 文件 / API 回應與桌面空快取. 本機合成資料不使用真實 credential, fixture 不啟動 MCP server, 實際帳戶 / cloud / Platform API Key 權限尚未驗證. Dots 手動查詢讀桌面快取, 不是遠端即時 API, 有關聯的自訂電腦名稱在目前有界投影未取得

2026-10-11 13:55, Asia/Taipei, 原生直接查詢的公開 OpenAI 服務狀態與 Codex release 都回傳 health=ok, 該次資料為 minor / 0.162.1. 同時正式 8787 的公開查詢路由回傳 HTTP 404, 這是舊後端尚未載入的證據, 不是合成 fixture 的成功證據. 本輪不重啟正式服務、不更新 pin、不提交 / 推送 / 建立 Tag, 所有來源與未提交成果保留

Firefox / WebKit、實體手把 / 觸控、其他 OS、輔助技術、完整 WCAG、GC 後保留量與正式服務驗收仍為交付環境待驗證. 共用虛擬捲動的配對量測見[效能與資料上限](performance-report.md), 不把共用 fixture 效能當成 LAM 所有頁面的改善比例

### 修改與新增檔案

- 修改 `AI.md` - 依任務查找來源與驗證入口
- 修改 `README.md` - 目前功能 / API / Dots 操作入口
- 修改 `docs/architecture.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/card-library.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/data-inventory.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/features.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/maintenance.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/performance-report.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/settings-format.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/usage.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `docs/validation.md` - 用法 / 架構 / 驗收與規劃文件
- 修改 `frontend/app.js` - 頁面呈現、查詢工具列、設定、提示與明細
- 修改 `frontend/index.html` - 頁面及設定控制項
- 修改 `frontend/locales.json` - 三語系文案
- 修改 `frontend/style.css` - 布局、字級、配色與浮動樣式
- 修改 `src/local_activity_monitor/codex_metadata.py` - 沿用共用 Dots 投影
- 修改 `src/local_activity_monitor/codex_schedules.py` - 已觀察排程活動
- 修改 `src/local_activity_monitor/desktop_activity.py` - Dots / 電腦投影與有界快取讀取
- 修改 `src/local_activity_monitor/server.py` - 固定查詢路由 / 設定驗證
- 修改 `tests/context-agent-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 修改 `tests/detail-tabs-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 修改 `tests/serve_loading_fixture.py` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 修改 `tests/settings-cards-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 修改 `tests/test_desktop_activity.py` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 修改 `tests/test_runtime_ui.mjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `src/local_activity_monitor/api_queries.py` - 七個查詢來源、間隔與生命週期
- 新增 `src/local_activity_monitor/codex_cloud.py` - 雲端對話唯讀查詢
- 新增 `src/local_activity_monitor/openai_platform.py` - OpenAI Platform 唯讀查詢
- 新增 `src/local_activity_monitor/public_api.py` - 固定公開狀態 / Codex 版本來源
- 新增 `tests/api-query-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/appearance-controls-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/card-presentation-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/cloud-activity-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/instructions-layout-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/presentation-settings-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/query-status-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/settings-sources-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/table-column-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/test_api_queries.py` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/test_codex_cloud.py` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/test_openai_platform.py` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/test_public_api.py` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/theme-api-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 新增 `tests/tooltip-semantics-flow.cjs` - focused 邏輯 / 型別 / UI 回歸與隔離 fixture
- 更新本機 `.local/task-context.md`, 保存本批有效目標與當前驗收

## 最近活動時間預設

2026-10-11 11:00, Asia/Taipei, `frontend/app.js` SHA-256 前 16 字元為 `3d1e1ca8183bc1e5`, 本機修改保留未提交, WBUI 固定版本維持 0.9.0

`table-column-flow.cjs` 在 Edge 155 合成資料通過最近活動時間欄位置首及新到舊排序, 同一表格的建立時間與最近活動時間採相反順序, 確認有活動紀錄時使用最近活動, 活動紀錄全為缺值時改用原本建立時間, 沒有活動欄位時保留原本排序與欄位順序, 自訂欄序 / 排序、重載與列 ID 保留

原有拖曳、Escape / 關閉 / 擷取中斷取消、四種寬度與缺值處理同時通過, 固定版本建置、JavaScript 語法、runtime 與新增文件文案檢查通過, 修改 `frontend/app.js`、`tests/table-column-flow.cjs`、`docs/usage.md`、本驗證紀錄及 `.local/task-context.md`, 其他未提交成果保留, 隔離 fixture 已結束

## 欄位排序與指示文件

2026-10-11 02:14, Asia/Taipei, 受測來源為 `1dbf722` 之後的本機未提交修改, WBUI 固定版本仍為 `cf13a9417d752001ec388ab74a0f0fddab238066` / 0.9.0

| 來源 | SHA-256 前 16 字元 |
| --- | --- |
| `frontend/app.js` | `017daafcb7f95146` |
| `frontend/style.css` | `9416bf58ed0452f4` |
| `frontend/index.html` | `b9859ffe40e1da3c` |

| 檢查 | 結果與範圍 |
| --- | --- |
| 欄位排序 | Edge 155.0.4283.45 的 `table-column-flow.cjs` 核對 20 欄清單, 通過設定列拖曳、方向鍵 / Home / End、焦點保留、隱藏欄位與重載保存, 設定排序不需開啟主頁拖曳開關 |
| 表頭與取消 | 整個拖曳不觸發資料排序, 下一次點擊可正常排序, Alt + End 依可見欄位移動, Escape、關閉設定視窗及滑鼠擷取中斷皆取消順序變更並清除預覽, 排序動畫完成後核對列 ID 保存 |
| 文件與間距 | `instructions-layout-flow.cjs` 通過全域 AGENTS.md 入口、文件與概要合併、預覽 / 原文、零值、空資料、503 及再次開啟後恢復, 1366 / 900 / 390 / 320 px 的入口與下方卡片保留 16 px 間距, 欄位清單無重疊或頁面水平溢位 |
| 明細回歸 | `detail-tabs-flow.cjs` 通過其他明細的概要首頁、單一標題列操作、內容快取 / 取消與草稿保留, 三語系共 12 組布局, 瀏覽器沒有 pageerror |
| 建置與靜態檢查 | 固定 WBUI 前端建置、4 份 JavaScript 語法、`test_runtime_ui.mjs` 與 `test_frontend_build.py` 16/16 通過, Python 測試先遇沙箱 Temp 權限錯誤, 原生隔離暫存環境重驗通過 |

以上流程使用 `serve_loading_fixture.py` 的 demo threads、隔離 SQLite 與合成指示文件, 桌面及直向截圖已人工檢視, 新增文件文案通過本機 textlint 與人工語意複核, 正式 8787 服務未手動重新啟動, 觸控及其他瀏覽器未納入本次驗證

本次修改 `frontend/app.js`、`frontend/style.css`、`frontend/index.html`、`tests/detail-tabs-flow.cjs`、`AI.md`、`docs/usage.md` 與本驗證紀錄, 新增 `tests/table-column-flow.cjs`、`tests/instructions-layout-flow.cjs`, 本機 `.local/task-context.md` 保存目前驗收與未提交狀態

## 0.14.0 歷史分頁、代理關聯與 Context 更新

2026-10-11 00:02, Asia/Taipei, 受測來源為 `b0275e5` 之後的本機 0.14.0 工作目錄, `workbench-ui.json` 維持 `cf13a9417d752001ec388ab74a0f0fddab238066` / WBUI 0.9.0, 下方各節保留先前受測來源

| 來源 | SHA-256 前 16 字元 |
| --- | --- |
| `frontend/app.js` | `97a61bee47b153eb` |
| `frontend/style.css` | `e4e61c328b7edb5d` |
| `frontend/index.html` | `5281ed100f40ee9c` |
| `frontend/locales.json` | `4ce46e1098074cae` |
| `src/local_activity_monitor/collectors.py` | `7897e7fa6727974e` |

| 檢查 | 結果與範圍 |
| --- | --- |
| 歷史紀錄 | Edge 155.0.4283.45 的 `history-flow` 通過 SQL / SQLite、網路、MCP、錯誤、技能各自的頁內入口及分頁, 175 筆 MCP 紀錄以 cursor 逐頁讀取皆保留唯一 ID, 首末 / 前後頁、503 恢復、無效 cursor 400 與空資料隱藏表頭通過 |
| Context 顯示更新 | `context-refresh-flow` 核對修改段落與追加紀錄, 未變動紀錄保留 DOM、捲動區、展開狀態與捲動位置, 讀取失敗保留內容, 恢復及明細重建後的捲動位置通過 |
| 代理與用途 | 同一流程核對父對話 / 子代理入口及實際 thread ID, Goal / Plan 歷史標籤移至獨立「已觀察用途」欄位, 20 欄表頭與儲存格一致, 1366 / 390 / 320 px 無頁面水平溢位 |
| 明細回歸 | `context-agent-flow`、`context-layout-flow` 核對來源空 / 尚未記錄、遮蔽與請求取消、父子來源分開及三語系窄畫面 |
| 表格與更新 | `files-navigation-flow`、`partial-refresh-flow`、`tab-consistency-flow` 通過, 1,000 筆檔案來源只掛載 10 列, ID / Enter / Space / 零值 / 分頁 / 搜尋保留, 本次冷開啟合成樣本 567.4 ms |
| Python 與建置 | 前端建置 16/16 通過, Context / history store / monitor state / performance debug 合計 38 筆, 37 通過、1 因 Windows 未允許建立 symlink 而略過, Context byte offset 在追加及同長度修改後保留原紀錄身分 |

Context 來源讀取仍使用既有有界、按需端點, 本次增量處理在顯示層, 不新增常駐內容保存或 SSE 變更, 代理關聯入口使用已觀察的 parent_thread_id, 不將子對話訊息冒充父對話訊息, 換頁動畫以目前 pin 的 CSS 相容規則限制捲動範圍, 共用修正另由 WBUI 0.10.0 提供

測試使用自行建立的 `serve_loading_fixture.py`、隔離 SQLite 與合成訊息, 不讀取正式帳戶內容, 正式 8787 服務未重新啟動, 使用端 pin 未更換, 本輪未執行五平台包建置或 Firefox / WebKit

本次調整 `frontend/app.js`、`frontend/index.html`、`frontend/style.css`、`frontend/locales.json`、`src/local_activity_monitor/collectors.py`、`tests/history-flow.cjs`、`tests/test_context_detail.py`, 新增 `tests/context-refresh-flow.cjs`, 同步 `AI.md`、`docs/features.md`、`docs/usage.md`、`docs/maintenance.md` 與本驗證紀錄, 版本來源 `pyproject.toml`、`src/local_activity_monitor/__init__.py` 由已發布的 0.13.0 遞增至 0.14.0

### 本批交付檔案

- 文件、教學與 AI 入口: `AI.md`、`README.md`、`docs/README.md`、`docs/architecture.md`、`docs/card-library.md`、`docs/features.md`、`docs/maintenance.md`、`docs/performance-report.md`、`docs/settings-format.md`、`docs/usage.md`、`docs/validation.md`
- LAM 畫面、語系與樣式: `frontend/app.js`、`frontend/index.html`、`frontend/locales.json`、`frontend/style.css`
- 版本、來源清單與索引: `pyproject.toml`
- LAM 資料與診斷: `src/local_activity_monitor/__init__.py`、`src/local_activity_monitor/collectors.py`、`src/local_activity_monitor/monitor_state.py`、`src/local_activity_monitor/performance_debug.py`
- 測試與合成驗證: `tests/chart-copy-flow.cjs`、`tests/context-refresh-flow.cjs`、`tests/cpu-theme-flow.cjs`、`tests/debug-flow.cjs`、`tests/diagnostic-settings-flow.cjs`、`tests/files-navigation-flow.cjs`、`tests/history-flow.cjs`、`tests/partial-refresh-flow.cjs`、`tests/serve_loading_fixture.py`、`tests/settings-cards-flow.cjs`、`tests/test_context_detail.py`、`tests/test_monitor_state.py`、`tests/test_performance_debug.py`

## AI 文件入口

[AI.md](../AI.md) 按資料來源、畫面 / 更新、卡片 / 設定、診斷與固定 WBUI 版本提供實作及 focused 測試路徑. README、功能、卡片庫、設定格式與架構同步目前候選, 分清 LAM 已採用的 0.9.0 與相鄰 WBUI 未交付的新能力

本次只更新文件與本機指引, 不改 LAM runtime、資料庫或 SSE. 文件核對參照實際來源 / 型別 / pin、相對連結 / 程式碼區塊及本機 textlint / 人工語意複核, 下節保留先前行為測試的受測來源與證據

2026-10-10 文件整理另加入[文件索引](README.md), README 保留已實作功能與操作入口, 待辦及遠端設計仍留 docs. 14 份公開 Markdown 文件的連結 / 標題 / 程式碼區塊、本機 textlint 與人工複核, 不重跑 LAM 行為測試, 元件教學由 WBUI 的文件與合成流程驗證

本輪新增 `docs/README.md`, 調整 `README.md`、`AI.md` 與 `docs/validation.md`. 既有未提交的其他來源變更保留

## 2026-10-10 工作目錄驗證

受測來源為 `b0275e5` 之後的本機 `0.13.0` 未提交修改, Python 3.12.14、Node.js 22.19.0、Microsoft Edge 155.0.4283.45 headless. LAM 保持 `workbench-ui.json` 指定的 WBUI `cf13a9417d752001ec388ab74a0f0fddab238066` / 0.9.0, 未採用相鄰 WBUI 0.10.0 候選. 本輪未修改 SSE, 未重新啟動正式服務

受測檔案 SHA-256 前 16 字元:

| 檔案 | 來源 hash |
| --- | --- |
| `frontend/app.js` | `34fe264eb1a3aa07` |
| `frontend/style.css` | `685b5a78823c3c1b` |
| `frontend/index.html` | `25801082f9dcc830` |
| `frontend/locales.json` | `7d23da216121f392` |
| `monitor_state.py` | `d8214f61dec39654` |
| `performance_debug.py` | `4ccac60ef774341a` |

| 檢查 | 結果與範圍 |
| --- | --- |
| 檔案頁 | `files-navigation-flow.cjs` 通過隱藏 / 可見頁首次只掛載 10 列、完整 1,000 筆來源、排行、分頁、Enter / Space 對應目前 ID、metadata 零值、搜尋換頁及保留 DOM. 900 px 直向、390 / 320 px 無頁面溢位 |
| 設定與卡片 | `settings-cards-flow.cjs` 通過來源 / 自訂卡片與群組、排序及取消、穩定 ID、Goal / Plan 實際觀察、回補完成 / 未完成文案、120 px 載入及五種寬度 |
| 更新 | `stable-refresh-flow.cjs`、`partial-refresh-flow.cjs`、`table-stream-interaction-flow.cjs` 通過 DOM / 焦點 / 頁碼保留、hover 數值局部更新、保留列順序、焦點保護、離開套用最新值、背景暫停動畫、系統減少動畫及來源更新後恢復 |
| 資料與布局 | `chart-copy-flow.cjs`、`loading-layout-flow.cjs` 通過合成錯誤類型 / Log 等級、空資料文案、帳戶零值 / false、卡片不重疊、頁尾位置及三種寬度 |
| CPU / 主題 | `cpu-theme-flow.cjs` 通過 CPU 零值與缺值分開、棕色主題 / 共用 tokens 及直向布局 |
| 診斷設定副本 | `diagnostic-settings-flow.cjs` 通過自訂名稱 / 路徑 / 數值 ID 去識別化、gzip 往返、JSON 備用方式, 本機設定完整保留. 合成樣本 18,121 bytes 壓縮為 662 bytes |
| Debug | `debug-flow.cjs` 通過主設定開關、路徑、21 筆隔離紀錄、32 MiB 上限、無收集 heartbeat、關閉停止記錄及隱藏面板. 三語系各 1366 / 390 / 320 px 無溢位 |
| 來源與建置 | 前端建置與 `test_frontend_build.py` 16/16、`test_performance_debug.py` 8/8、`test_monitor_state.py` 18/18 通過, JavaScript runtime / 頁面指標 / 用量投影 focused checks 通過 |

瀏覽器使用 `tests/serve_loading_fixture.py` 的 demo threads 與隔離 SQLite, 不讀取正式帳戶或實際來源內容. 同一瀏覽器流程可將各 `tests/*-flow.cjs` 匯出的函式交給 Playwright Page 執行, fixture 服務及瀏覽器由測試建立者清理. 具體效能樣本見[效能紀錄](performance-report.md)

上述結果涵蓋合成資料及指定操作, 實機來源、Firefox / WebKit、輔助技術、完整 WCAG 與長時間 GC 後記憶體尚未驗證. 不把相鄰 WBUI 候選的檢查當成此固定版本已採用的新能力

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

測試使用暫存 metadata、SQLite 與來源 fixture, 涵蓋資料投影、差異讀取、checkpoint、內容遮蔽、來源快取、HTTP 邊界、啟動及建置回復

Windows 的來源更新與閒置 I/O 比較使用 `tools/benchmark_source_updates.py`, 先完成啟動整理再量測. 舊版從指定 Git 提交擷取, 新版凍結目前原始碼, 使用相同 12 檔合成資料, 每組獨立 home, 不開啟 HTTP、帳戶 API、硬體查詢或 Debug. 舊版預設 10 秒輪詢, 另以已滿 5 分鐘的 fixture 狀態檢查閒置模式. 兩版交換順序執行, 程序 I/O 計數先以 256 KiB 讀寫校準, 結果保存於 `.local/source-update-io-performance.json`

```powershell
python -X utf8 -B tools/benchmark_source_updates.py --baseline a1cc7fe --duration 65 --runs 2
python -X utf8 -B tools/benchmark_source_updates.py --baseline a1cc7fe --case active --duration 35 --runs 2
```

`active` 模式由獨立程序每秒追加一筆資料, 最後保留 15 秒讓兩版讀完相同內容, 核對最新 Token 值. 產生資料的程序 I/O 與 CPU 排除於量測, 結果保存於 `.local/source-update-io-active-performance.json`

比較未提交的原始碼時, 先保存修改前的來源, 再以 `--baseline-source` 選取該副本. 副本與輸出均放在 checkout 的 `.local`, 每組結果保存兩版 SHA-256, 並另記首次整理的 CPU、耗時與 I/O

```powershell
python -X utf8 -B -c "from pathlib import Path; from tools.benchmark_source_updates import freeze_sources; print(freeze_sources(Path('.local/before-source')))"
# 修改原始碼後執行
python -X utf8 -B tools/benchmark_source_updates.py --baseline-source .local/before-source --case active --duration 35 --runs 2 --output .local/optimized-active.json
python -X utf8 -B tools/benchmark_source_updates.py --baseline-source .local/before-source --duration 65 --runs 2 --output .local/optimized-idle.json
```

`aggregate_many` 核對同一唯讀交易的時間範圍、來源篩選、空結果與獨立副本, 並模擬讀取期間另一個 writer 新增資料. 錯誤與對話狀態檢查相同輸入略過、完整保存、offset 變更及失敗重試

`v0.11.0` 交付前固定 WBUI `v0.7.0` 的提交 `19a81a7`, 兩組資產 manifest 與完整 SHA 相符. 本機原生環境執行歷史、時間範圍、保存、HTTP、前端建置與資產檢查, 67 項中 65 項通過, 2 項因無法建立 symlink 與大型 HTTP 標準函式庫基準逾時略過. 三個 Node 使用端檢查通過. 正式固定資產方式的合成服務另通過四類表格換頁與 23 種明細流程, 包含三語、四種寬度、連點、取消、焦點與長網址提示

`v0.12.0` 交付前在 Windows、Python 3.12.14 執行完整 474 項測試, 471 項通過, 2 項因無法建立 symlink、1 項因大型 HTTP 標準函式庫基準逾時略過. 固定 WBUI `19a81a7` 的前端建置、資產檢查、JavaScript 語法及三個 Node 使用端檢查通過. Edge 154.0.4258.62 的隔離合成服務通過 12 類流程, 涵蓋 SSE、重連、還原設定、載入、啟動預載、互動期間保留表格、Debug、歷史圖表、換頁、摘要與直螢幕. 分頁 SVG 的水平與垂直中心偏移均為 0, 各按鈕高度一致, 未出現頁面程式錯誤. 正式資料的長時間效能及五平台原生包仍依後續驗收流程核對

`test_activity_history.py` 核對超過 1 MiB 的未變更輸入不重寫、完整 SQLite 保存與有界畫面資料、失敗後重試、快照獨立複製及即時保存期限篩選

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
| `detail-tabs-flow.cjs` | 23 種明細的概要首分頁與單一頂端導覽, 保留歷史資料查詢分類, 檔案執行內容延後讀取、遮蔽、快取、重試、取消與快速返回, 編輯草稿、三語四種畫面尺寸及背景更新後的長網址圖例提示 |
| `parser-summary-motion-flow.cjs` | 結構解析、摘要、減少動畫及設定匯入 |
| `console-sql-flow.cjs` | console 呈現、SQL 外層工具回覆、複製與捲軸 |
| `context-agent-flow.cjs` | 代理訊息、公開 Context 摘要、遮蔽切換、舊設定相容性及三語空紀錄 / 整理中提示 |
| `context-layout-flow.cjs` | 父子 Context、對話明細 Context 延後讀取與快取、列點選、欄位重排後的入口、展開中斷、長程式碼、窄螢幕明細與三語兩行狀態列 |
| `tab-consistency-flow.cjs` | 八個主頁及各子頁的卡片、表格、欄位、設定相容與四種寬度 |
| `model-api-flow.cjs` | 模型 API 事件分類、明細、缺值與來源 |
| `reactive-motion-adapter-flow.cjs` | 卡片、資料列與欄位插入 / 移出, 保留節點與目標計數 |
| `history-flow.cjs` | SQLite 歷史總數、游標分頁、錯誤後重試 |
| `pagination-motion-flow.cjs` | 對話、一般表格、歷史與說明分頁的左右滑動、快速連點、SVG 按鈕置中與一致高度、減少動畫及四種畫面寬度的溢出 / 按鈕重疊 |
| `history-chart-flow.cjs` | 完整保存範圍統計與總覽副本獨立範圍 |
| `reconnect-flow.cjs` | 中斷後自動重連與保留畫面 |
| `restart-reset-flow.cjs` | 後端更新及還原預設不重新載入整頁 |
| `event-stream-flow.cjs` | SSE 快照與 Log、重複及晚到資料、後端重啟與重連、連線關閉 |
| `debug-flow.cjs` | Debug 開關、監測程式 Tab 與明細、心跳不觸發來源收集、停止記錄及三語排版 |
| `stream-burst-flow.cjs` | 接受 page 與 CDP session, 1200 筆快照與獨立 / 混合 Debug 事件合併渲染、最新資料與回收後 heap / DOM 保留量 |
| `idle-interaction-flow.cjs` | 明確暫停與最新快照合併、hover / 焦點 / 選取 / touch / 失焦保護、診斷回應、無效網址、來源更新後恢復、狀態到期、背景切換及檔案資訊檢查頻率 |
| `idle-cadence-flow.cjs` | 隔離瀏覽器時鐘核對 60 秒的計時回呼、手動暫停後停止計時、裝置取樣時間與四種寬度 / 三語系排版, 計時次數不代表實機 CPU 改善幅度 |
| `partial-refresh-flow.cjs` | hover 更新數值並保留列身分、順序、篩選結果及分頁, 焦點保留內容, 新增 / 移除延後套用, 失焦與系統減少動畫 |
| `mcp-refresh-flow.cjs` | MCP 摘要更新保留卡片、欄位與標題、柔和光條、連續更新逐影格的可見性與高度、單一明細入口及三種寬度 / 三語系布局 |
| `stable-refresh-flow.cjs` | 總覽捷徑、額度、已展開來源、裝置欄位的焦點與文字選取、官方帳戶欄位、MCP 來源卡與表格、互動期間的欄位變動、文件列表及圖表保留 / 更新 |
| `table-stream-interaction-flow.cjs` | SSE 更新期間保留滑鼠與鍵盤目標, 離開後套用最新排序, 對話及 SQL 表格 |
| `followup-layout-flow.cjs` | 空圖表恢復、精簡欄位、Context 順序、摘要、列明細、空資料提示及首尾頁 |
| `startup-preload-flow.cjs` | 首輪活動整理前的對話目錄與完整結果切換, 使用隔離回應 |
| `viewport-detail-flow.cjs` | 截圖比例、不同字級與語系、兩行狀態、摘要與子頁籤分區、Dots 明細 |

`test_startup_preload.py` 核對 state 目錄查詢的 2,000 筆上限、session 讀取量為 0、未選取 prompt / credential 欄位、停用後不讀取及完整更新時間尚未產生. 瀏覽器模擬回應驗證呈現, 原生 fixture 可加 `--startup-delay 35`, 核對 HTTP 在整理前提供目錄, 整理後提供完整快照

## 更新後的正式資料驗收

更新後端後重新啟動服務, 再重新載入頁面. `/api/activity` 的 `code_revision` 與快照的 `backend_revision` 識別後端工作階段, `frontend_revision` 識別前端資產, 兩者分別核對. 使用 `tools/watch.py` 時, 確認其所屬後端已完成重新啟動

| 項目 | 驗收內容 |
| --- | --- |
| 來源計數 | 頁首可讀取 / 啟用數與來源 Log 共用讀取器清單, 停用列另計, 檢查時間與讀取結果相符 |
| 驗證與 MCP | 從已觀察操作核對分類、工具回傳、時間及耗時, 空資料時追查來源、篩選範圍與回補進度 |
| 父子 Context | Context 列開啟目前對話內容, 對話列表列開啟明細後切入 Context 才讀取, 切換分頁保留內容, 代理訊息與父對話入口維持對應, 核對更新、欄位重排與取消要求 |
| Dots 與排程 | 核對桌面快取中的活動及電腦, 即時連線與完整遠端排程仍列待實作 |
| 裝置資訊 | 依實體顯示卡識別核對數量, 在對應平台驗證字型、取樣時間與使用量 |
| 效能 | 固定資料、設定及取樣條件, 比較冷啟動、回補、增量與長時間更新, 分開記錄 CPU、耗時、讀取量及快照大小 |

`test_http_snapshot.py` 的大型 HTTP 本文案例會先檢查標準函式庫 HTTP 基準, 基準失敗時跳過該案例. `test_history_store.py` 與 `test_plugins.py` 的符號連結案例在無法建立連結時跳過. 驗收結果需列出實際跳過原因, 大型本文另以目標瀏覽器檢查 gzip 與未壓縮回應

macOS / Linux 實機啟動與字型、Firefox / WebKit、觸控、輔助技術及平台下載包的待驗收項目見[功能清單](features.md)

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

本批自建隔離 fixture 與測試瀏覽器均已關閉, 正式 8787 未由本輪重新啟動

## 2026-10-12 WBUI pin 更新

- 使用既有穩定 Tag 選擇器與建置器, 將 WBUI 從 0.11.0 / 7c77ffea 更新至 0.12.0 / 03c10244, 完整 SHA 保存於 workbench-ui.json. 兩組離線 manifest、逐檔 hash 與 LAM 前端來源核對一致, 正式 8787 頁面已由既有 watcher 自動載入 0.12.0, 本輪未手動重新啟動
- 原生 Windows 的 16 項前端建置、更新失敗保留與防降版測試通過, JavaScript 語法檢查通過. 沙箱暫存目錄的權限失敗已改在原生環境重驗
- Edge 合成資料驗證通過載入範圍、三語系 / 三寬度的可見顯示設定、九方位通知與保存、工具 / MCP / Git / 錯誤 / Markdown 輸出及 modal 關閉 / 取消請求. output-detail-flow 的資源斷言改為核對 modal 所有權, 保留主頁控制器, 並拒絕未釋放的 modal 或 detached 資源
- 完整 presentation-settings-flow 仍引用已隱藏的 API 入口, 本次僅驗證其可見設定區段. files-navigation-flow 的「未變記錄保留列 DOM」斷言, 在相同合成資料、Edge 與 CSP bypass 條件下比較 0.11.0 / 0.12.0 都失敗, 保留為既有待查問題, 不宣稱完整表格流程通過

## 2026-10-19 WBUI 0.18.0 串接

固定正式 Tag v0.18.0 / `add6565`, 以既有 `build_frontend.py --revision` 建置 pin 與兩組離線資產, 四份共用資產核對確切 Git 來源, 頁面 manifest 與 pin 一致. 本機 Windows Python 3.12.14 的 18 項前端建置與 7 項資產載入測試通過

Edge 155.0.4283.45 合成資料驗證通過 `partial-refresh-flow.cjs` 與 `monitor-ui-revision-flow.cjs`: hover 數值更新保留列 DOM / 成員 / 分頁, 焦點保護、恢復最新資料、失焦與減少動畫, 以及 1366 / 820 / 390 / 320 px 的表格與監測資訊. 390 px 截圖已檢視, 自建 fixture 與瀏覽器均已關閉. 正式服務與使用者資料不作為本次合成測試輸入, 既有平台及長跑驗收沿原範圍保留

## 2026-10-20 WBUI 0.19.0 串接

固定正式 Tag v0.19.0 / `9593397`, 以既有建置工具同步 pin、共用離線資產及頁面 manifest, 四份資產逐 byte 與確切提交來源相符, LF loader 保持跨使用端相同 checksum, 未變更 LAM runtime 版本或前端資料流程

- Windows Python 3.13.15 的 18 項前端建置與 7 項資產測試通過, 固定離線建置驗證通過
- Edge 155.0.4283.45 隔離合成資料的 table-stream-interaction、loading-scope、monitor-ui-revision 三組流程通過, 涵蓋列身分 / 點擊 / 焦點保留、最新值套用、載入 / 錯誤恢復及 1366 / 820 / 390 / 320 px 表格布局
- 最終資產實際載入 WBUI 0.19.0, 無 pageerror, 390 px 截圖已檢視, 自建服務與瀏覽器已關閉

本輪驗證使用隔離來源, 未手動重啟正式監測服務或建立原生發布包, 使用端資料與偏好仍由 LAM 維護

## 2026-10-21 WBUI 0.20.0 串接

採用正式 Tag v0.20.0 / `f79ca1c`, 取代 0.19.0 pin, 使用既有建置工具更新離線資產與頁面 manifest, LAM 版本更新為 0.20.4. 四份共用資產與確切提交來源逐 byte 相符, 保留現有 classic shared loader 與使用端資料流程

- 18 項前端建置、7 項資產及 3 項發布組裝測試通過, 固定離線建置驗證通過
- Edge 155.0.4283.45 隔離合成來源的 table-stream-interaction、loading-scope、monitor-ui-revision 三組流程通過, 涵蓋列身分 / 點擊 / 焦點保留、最新值套用、載入 / 錯誤恢復及 1366 / 820 / 390 / 320 px 表格布局
- 實際載入 LAM 0.20.4 / WBUI 0.20.0, 無 pageerror, 390 px 截圖已檢視, 自建服務與瀏覽器已關閉

本輪未手動重啟正式監測服務或建立原生發行包, macOS 實機與常駐服務採用沿各自驗收

## 2026-10-24 固定版本資產校正

WBUI pin 保持 f79ca1c / 0.20.0, 四份內嵌資產中只有 workbench-loader.js 的 CRLF 位元組與固定 Git 提交不同, 自身 manifest 驗證原本仍通過
先保留原離線包, 再從固定提交校正資產與 manifest, 四份資產已與 Git bytes 相符, pin 與 LAM 程式碼未修改, 詳見 .local/fix-10-assets.json

verify_bundle、7 項資產測試與 18 項前端建置測試通過
內建瀏覽器在合成資料 fixture 載入 LAM 0.20.4 / WBUI 0.20.0, 設定視窗開關、Escape 與焦點返回通過, 正式監測服務未重啟
WBUI 資產準備工具的根因修正與 8 項測試記錄於共用庫 validation 的 FIX-10, 本輪沒有採用 0.20.1 或變更資料來源
