# 驗證紀錄

## 2026-10-07 歷史與建置交付整理

已在隔離副本逐一整理 25 筆來源提交, 移除 11 個桌面封裝入口、相依清單、執行環境、圖示與專屬測試路徑, 並清理相應封裝程式及文件段落. 原始 recovery bundle 與 commit 對應表保留於私人備份, 不加入公開來源

本輪核對 25 筆提交與 384 個歷史 blob, 未保留已移除的封裝或私人 agent 路徑及封裝關鍵字. 清理後以隔離暫存資料重跑前述五組最小檢查, 50/50 通過. workflow 的 0.x / 1.0.0 與發布旗標四種組合通過, 本機未執行封裝測試

每個 minor tag 須核對 `pyproject.toml` 與 `src/local_activity_monitor/__init__.py` 的實際版本, 不把舊版 tag 指向最新程式. 本輪不重建已略過的 patch tags, GitHub Releases 在 1.0.0 前維持空白

歷史驗證紀錄對應清理前的來源與當時流程, 不自動作為改寫後 commit / tag 的驗收. 最新 `src/` 執行程式與固定 WB revision 保留, 新版 workflow 預設只保存原生 / 原始碼與 checksum artifacts, 明確發布旗標在 0.x 會拒絕建立 Release

可達 refs 的清理不證明 GitHub 已清除舊 SHA 快取、其他人的 clone 或既有 Actions 紀錄. 本輪不刪除他人的本機副本

## 0.6.0 發布準備

2026-10-07, `test_mcp_observation`、`test_content_details`、`test_content_pages`、`test_ui_assets` 與 release-mode guard 合計 50/50 通過, 使用 Python 3.13.15 與隔離合成暫存資料. 先前 sandbox 暫存目錄權限失敗及錯誤測試模組名稱已修正重跑, 不列為產品測試通過. 版本 / MIT metadata / locale UTF-8 解析通過, native archive mock 測試未在本機執行, 留待 release CI

README、授權範圍、1.0.0 與效能報告經 textlint / 人工複核, 不做本機封裝. 下列瀏覽器紀錄仍為先前實際流程, 不以版本 / 文件調整宣告新的 CPU / 記憶體數據

## 2026-10-07, 0.5.1

Windows, Python 3.12.14 隔離測試與 Python 3.14.7 本機服務, Node.js 22.19.0, Playwright CLI 0.1.22 與 Edge

- 完整 unittest 共 366 項, 364 項通過, 2 項因環境略過, 耗時 39.491 秒. 涵蓋按需 Git / 驗證 / 工具 / 錯誤內容, 安全物件與 YAML 子集解析, Plugins metadata、工作樹、持續寫入時的歷史回補預算及來源啟動流程
- 真實本機資料檢查 8 個主頁與 18 個子頁, 共 26 組. 已有資料的表格表頭與本文欄位位置差為 0 px, 沒有卡片重疊或頁面橫向溢出, 時間範圍只保留一個全域控制項. 空表格未作欄位對齊判定
- 三種語言與 1280 / 820 / 390 / 320 px 及 844 x 390 短視窗共 15 組設定檢查通過, 18 px 字級的窄螢幕設定沒有溢出. 右上資訊區維持兩行, 第一行連線狀態與額度百分比, 第二行重設與更新時間, 三種語言、四種寬度共 12 組檢查沒有標題或列間重疊, 缺值、0% 與 100% 保留正確意義. 窄螢幕的更新間隔與啟動時間保留於 tooltip
- Git diff、驗證指令、SQL 原文與 Log 內容實際可讀. textlint lintFile 在歷史回補完成後能取得輸入與外層回傳, 不以外層回傳推定個別 MCP 結果. 長紀錄分批回補, 明細保留來源與範圍, 不保存至 snapshot
- 本機工作樹查詢使用開啟 stdin 的監看服務情境重現, 修正後同一檢查由 5.037 秒逾時變成 0.073 秒取得結果, 8787 實際顯示 5 個工作樹. 沒有改寫 Git 設定
- Workbench UI 樹狀元件以實際技能檔案驗證展開、鍵盤導覽、內容載入、返回保留狀態與關閉釋放. 背景資料更新保留明細 DOM、捲動、收合與編輯草稿. 編輯框依文字寬高擴張, 390 px 下換行, 儲存與取消按鈕維持橫排
- 1280 px 三欄 Log 圖表的 220 px 高度中, 實際繪圖高度約 151 px, 單位與頂端刻度同列對齊且間距約 9.8 px. 校對使用本機 textlint 台灣用語規則, en / ja 新增說明另人工複核, 新增日文文字都是短標籤或名詞片語, 未以空的完整句子檢查宣稱日文文法通過
- CLI 缺少 UI 時自動使用既有 Git 認證取得固定版本, 18 項隔離測試涵蓋離線重用、固定來源、認證失敗、逾時、異常資產保留及 Windows junction / 快取路徑邊界. 真實隔離專案成功取得指定 commit 並產生四個前端資產、載入器與 manifest, 第二次直接重用離線資產, 臨時來源與快取已清理
- 首次跨平台 CI 發現暫存目錄別名造成 fixture 路徑不一致, 已改用解析後路徑. 第二次 CI 的 source、Linux x64 / ARM64 與 Windows x64 通過, macOS 啟動逾時由 traceback 確認停在 HTTP 綁定時的 getfqdn. 本機位址改用不依賴反向 DNS 的綁定流程, 以實際 loopback socket 與失敗的 DNS mock 驗證, 沿用原 port / 來源檢查
- v0.5.1 發布於 commit `449a0bb`, 共 12 個下載項目. 11 個套件的 GitHub 上傳 digest 與 SHA256SUMS 一致, 另核對校驗清單本身, 實際下載 source.zip、wheel 與 sdist 並檢查檔案 hash、內附 UI revision 與資產完整性. 未在本機重新下載或執行各平台原生包

## 2026-10-06, 0.5.0

Windows, Python 3.14.7, Node.js 22.19.0, Playwright CLI 0.1.22 與安裝的 Edge. 版面及設定使用 85 個合成對話的隔離服務, 真實本機資料只做唯讀來源與連線檢查

- 完整 unittest 共 282 項, 281 項通過, 1 項因標準函式庫 HTTP 控制伺服器逾時而略過, 耗時 18.128 秒. 略過項目不列為通過
- 1366 / 640 / 390 px, 三種語言與 14 / 18 px 字級, 共 432 組主頁與子頁, 2250 組設定分類檢查, 無頁面及設定控制項橫向溢出. 270 組圖表單位與頂端刻度檢查通過, 單位位於左側且不重疊. 切換主頁中位數 150 ms, P95 為 273.6 ms, 僅代表此合成資料與本機環境
- 另有 53 項互動檢查通過, 涵蓋八個主頁與子頁保留捲動, 1100 / 900 / 640 / 390 px 的設定說明分類, Switch 44x24 / 20px 圓角與標籤不切換, 官方帳戶來源啟用, 重載保存與關閉. 刻意注入畫面錯誤時顯示「畫面更新失敗」, 恢復後可再次更新
- 真實 8787 頁面重複更新三次均顯示已連線, monitor-environment 與 mcp-observations 容器存在. curl 取得 gzip 快照並完整解壓與解析, 951573 bytes, 183 個已載入對話, 版本 0.5.0. CSP 仍有 AdGuard 指令, 此結果未證明其造成標準函式庫連線問題
- 本機唯讀來源補齊七個歷史子代理程式, 均有已完成事件. 七筆保存的 textlint lintText metadata 中, 三個呼叫 ID 可取得輸入來源及外層回傳. 動態參數顯示記錄程式碼, 不宣稱已取得當時執行值. 截圖指定的 lintFile Call ID 不在目前來源中, 以合成同格式呼叫驗證解析與頁面呈現
- 合成 SQL INSERT 原文可按需讀取, 不執行 SQL, 未保存至 snapshot. 不具來源本文的 SQL 診斷保留無法取得狀態. Plugins 區分設定與快取, 真實本機取得 30 項 metadata, 不以快取存在判定已載入
- 17 MiB 同一來源 fixture 的冷啟動與回補均遵守每輪 8 MiB 預算, 耗時依序約 107 / 23 / 9 ms, 完成後無新增資料的增量更新為 6 ms / 0 bytes. 回傳約 4.4 KiB, 僅代表該 fixture
- 官方 Codex CLI 0.160.0 的三個帳戶唯讀方法實際取得回應, 確認額度與帳戶統計來源可用, 未顯示帳戶 ID 或憑證. API 設定流程與失敗隔離由合成回應驗證, 未連接雲端 Work / My dots / 排程清單
- 本機 textlint 台灣用語規則及 prh 檢查文件與 33 組新增文案, 日文完整句子經技術寫作規則檢查, 均無提示. 英文與日文短標籤另人工複核, 沒有送至外部校對服務. JavaScript 語法與 diff 空白檢查通過, LAM 附帶的 Workbench UI JS/CSS 與獨立來源逐位元組一致

共用介面參考 [Steam 商店](https://store.steampowered.com/), [SteamDB](https://steamdb.info/) 與 [Maximilian Lock 的社群設計案例](https://www.maximilianlock.co.uk/post/steam-redesign). 社群案例為作者研究, 本次檢查不等同完整可存取性或正式使用者研究

五平台建置與原生啟動結果以本版本 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml) 為準. 本機缺少 build 模組, 未宣稱本機套件建置通過

## 2026-10-05 至 2026-10-06, 0.4.0

Windows, Python 3.14.7, Edge, 使用獨立測試服務與瀏覽器. 延續 85 個合成對話, 加入三個本機排程, 兩個額度視窗與 26 種程式碼辨識工具. 正式服務與來源設定未改寫, 未執行真實安裝或付費 API

- 完整 unittest 共 260 項, 259 項通過, 1 項依環境略過, 耗時 30.787 秒. 啟動流程使用假安裝器與隔離環境, 排程覆蓋唯讀資料庫, 缺少來源, 舊欄位, 時間單位, 500 / 1,000 筆讀取上限, 隱私欄位與快取
- 1440 x 900 / 14 px 與 390 x 900 / 18 px 檢查八個主頁與所有子頁, 另切換三種語言的主設定與工具卡片庫, 共 52 組頁面檢查. 主設定及 Tab 設定的各分類維持固定寬高, 沒有頁面或設定內容橫向溢出, 資料來源區塊仍置底. 子頁沒有摘要卡或摘要設定, 瀏覽器沒有 JavaScript 頁面錯誤與失敗請求
- Steam 預設主題實際使用背景 #1b2838 與強調色 #66c0f4. 主頁七天範圍可被網路子頁一小時覆寫, SQL 子頁沿用七天, 回到網路仍保留一小時. 舊 MCP / Jev / Skills / 檔案 / SQL / Git / 驗證入口均定位到新的主子頁
- 摘要自訂名稱與三張顯示數量通過設定匯出 / 解析及重新載入檢查. 原圖表與資料範圍保留, 新導覽選擇納入匯出欄位. 已封存對話子頁依最新需求移除
- 工具預覽實際保留五行, 完整明細有 26 種工具. 面積圖實際產生填色區域, 耗時分布預設直條圖, 五個數值區間按大小排列, 138 個有效耗時樣本均納入計數. SQL 空資料顯示說明並移除空圖高度
- 額度卡的資料狀態與時間合併, 每個視窗只顯示一次. 方案沿用來源名稱, 未加入手動名稱. 對話明細各頁切換後內容捲動位置回到零, 排程時間置於標題, 全域 AGENTS.md 入口保留在專案頁
- 後續檢查三種語言的圖表與 70 張統計卡標題, 均不附加視圖名稱. 八個主頁與 15 個子頁切換時不主動捲動頁面, 另確認從監測頁切到用量頁保留 60 px 的既有捲動位置, 瀏覽器沒有 JavaScript 頁面錯誤
- 重複更新後 monitor-environment 與 mcp-observations 仍是原有 DOM 容器, 監測圖表與環境卡可共用較短欄位. 沒有重做大型效能比較, 此結果不宣稱速度或帳戶額度節省
- 本機 textlint 台灣用語規則檢查新增介面文字, 英文及文件段落, 日文技術寫作規則檢查完整句子, 均無提示. 日文短標籤與各語言語意另人工複核, 未送至外部校對服務. JavaScript 語法與 diff 空白檢查通過

發布套件由既有 [release workflow](../.github/workflows/release.yml) 在五個原生平台執行測試, 建置與隔離啟動檢查, 實際結果見 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml)

## 2026-10-05, 0.2.2 最終驗收

Windows, Python 3.14.7, Codex in-app browser. 延續隔離 fixture, 擴充至 86 個 session, 11 個 MCP 來源, 加入父子對話, 封存對話, 專案 roots, Global / Project AGENTS.md 與 Git 結構化回傳. 測試資料未使用正式對話內容或改寫正式來源設定

- 本機共 169 項 unittest, 168 項通過, 1 項大型 HTTP 對照測試因標準服務傳輸逾時跳過, 耗時 6.277 秒. 新增回歸覆蓋範圍篩選先於明細上限, Skills / Dot 全部保留資料的計數, 未知 MCP 的零值及多個紀錄旗標, 工具獨立統計, 專案名稱來源優先順序, 封存及父子 metadata, Git / Skills 按需內容, 指示文件的路徑 / 讀取 / 遮蔽邊界
- 來源範圍由全域預設及各主 / 子頁覆寫決定. 實際 API 比較 1 小時與全部, 工具, MCP, Skills, Dot, Log 與監測樣本使用範圍內資料. 最新對話狀態, 累計 Token, 額度, runtime 與累計 HTTP 計數保持最近來源語意. 測試確認時間未知事件只在全部顯示, 快照投影不增加來源讀取
- 1280 x 900 桌面與 390 x 844 窄螢幕檢查 12 個主頁. 主標題, 單行說明, 右側範圍與設定入口使用共用配置. en / ja 的 18 px 窄螢幕檢查亦涵蓋全部主頁, 沒有頁面橫向溢出, 範圍控制位置一致. 表格不使用浮動表頭
- 實際確認用量頁位於總覽之後, 摘要包含最後觀測與各視窗剩餘比例, 下方額度卡片只保留一組. 說明與官方連結合併置底. MCP 來源固定展開並換行, 沒有來源搜尋, 選取項目標題或水平捲軸
- 字級輸入 18 時保留已套用的 14, 按套用才改成 18. 桌面最終畫面回到 14. 排行預設 5, 表格預設 10, Skills 與工具統計分別提供 9 / 10 個欄位. 設定匯出, 預覽匯入及重新載入後保留全域 7 天和各頁全部範圍的覆寫
- Skills 按鈕實際開啟並切換 README.md / SKILL.md. 專案頁取得資料夾與相關 86 個已載入對話, Subagents 與封存頁各取得 1 筆, Global / Project AGENTS.md 入口按需載入對應文件. 名稱使用資料庫的新值, 不被舊 app state 覆蓋
- 明細的最近活動時間置於標題, 修正按需載入時的重複時間. 對話明細實際捲至 1,087 px 後切換子 Tab, 內容回到 0. 子頁及明細 Tab 的用途改為 tooltip. Jev 摘要與查看明細可點開, MCP 來源動作位於四張摘要卡之後
- Git 明細實際顯示已觀察的指令與預設收合回覆. JSON 及 JSON 相容的 Python literal 資料以縮排和彩色 key / string / number / punctuation 顯示, 巢狀 output 能解出獨立結構. 解析不使用 eval, 不執行紀錄內容. 最後瀏覽器沒有 console error / warn
- Node 語法, 18 個 Python 檔的 3.10 AST, README 相對連結及 diff 檢查通過. en / ja 各 1397 個 key 一致, 目前介面 source catalog 1255 個 key 無缺漏. 新英文文案及日文完整 tooltip 的 textlint MCP 檢查無提示, 短標籤另人工核對. 最後文件校對使用已核對資料流的本機 textlint CLI, 不傳送文件至遠端

效能使用相同 85 個 session fixture 與 ae8bdc4 的程式, 分別以兩種先後順序量測, 每組各 20 次 refresh + snapshot. 修改前增量中位數為 44.857 / 38.275 ms, 修改後為 59.267 / 47.681 ms, CPU 中位數各為 46.875 / 39.062 ms 與 62.5 / 46.875 ms. 新程式增加四個時間範圍與 metadata 投影, 這兩組量測增加約 9.4 至 14.4 ms, session 增量讀取仍皆為 0 bytes. 每組冷啟動只有單次樣本, 不用來判定加速. 最近一次 snapshot gzip 由 34,526 減為 26,574 bytes, 差異包含時間範圍語意與樣本數, 不代表壓縮效率改善. 未重跑 5,000 個 session 效能測試

首次原生 CI 在 macOS 的兩項指示文件測試發現暫存 fixture 使用 `/var` 系統 symlink, 被保護規則拒絕. 測試改以解析後的實際 fixture 路徑建立資料, 保留原本拒絕 symlink 的程式與驗證. 本機重新執行 169 項測試, 168 項通過, 原有大型 HTTP 對照測試 1 項跳過, 耗時 6.087 秒

發布前的原生平台建置與隔離啟動由既有 release workflow 驗證, 實際結果見 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml). 以下保留先前階段的驗證資料, 筆數與頁面配置以本節最終驗收為準

## 2026-10-04 至 2026-10-05, 0.2.2

Windows, Python 3.14.7 與 Codex in-app browser. 使用隔離的 85 個 session, 11 個 MCP 來源, Jev 與 SQL 示範資料. 沒有使用正式對話內容或改寫正式來源設定

- 125 項 unittest 通過, 1 項大型 HTTP debug 測試因標準函式庫對照服務的傳輸逾時而跳過. 新增 7 項來源測試, 覆蓋實際 schema / 路徑 / 讀取量, 缺少及不支援格式, 未知 telemetry 來源, 0 值, 停用狀態, MCP 暫停讀取不使快照失敗, 來源資訊不額外讀取檔案內容與不包含私人欄位. 重現並修正低頻 MCP 回應 / 工具 / 紀錄狀態被 1,000 筆表格上限遮掉, automation 查詢未限定範圍與未報告讀取量, checkpoint 0 筆無法區分讀取失敗
- Node 語法, Python 3.10 AST 與文件相對連結通過. en / ja 的 1321 個 key 一致, 新增來源描述保留日文標點
- 依最新決定, 實際確認新偏好為 14 px, 有效的 16 px 自訂偏好保留, 還原設定後實際回到 14 px, 排行 5 項, 表格每頁 10 筆. 所有主頁表格在資料充足時顯示 10 筆. MCP 排行的 5 / 10 / 20 / 全部位於齒輪內, 原先卡片上的數量選單已移除. 實際切換全部及重整後保留選擇
- 各主頁的來源說明初始收合且未建立詳細 DOM, 標題持續顯示最近更新時間. 各來源全寬排列且獨立展開, 更新後保留各自展開狀態. Catalog 實際顯示 85 列主查詢, automation_runs / automations 各 1 列與 85 個選取 Thread IDs, checkpoint 分列載入 / 保存狀態. 展開後顯示實際位置, 欄位 / 觀察項目, 程式上限, 讀取結果與保留量. 單一 MCP 頁不列其他來源的 telemetry. Jev 範圍從 24 小時切換為 1 小時後, 來源區塊顯示實際範圍及 0 筆明細 / 趨勢點
- 實際切換 zh-TW / en / ja, 檢查設定與詳細來源文案. 1280 px 桌面與 390 px 窄螢幕沒有頁面橫向溢出. 390 px 使用 18 px 字級時, 圖表設定採單欄且 modal 沒有橫向溢出, 長來源路徑可換行. 保留原有表格容器內捲動
- 工具頁新增統計表格, 直接呼叫與程式碼辨識分列, 實際確認每頁 10 筆, 數值排序與篩選為程式碼辨識後的 2 筆紀錄. 1280 / 390 px 逐一檢查 11 個主頁完成渲染後的表格筆數與 SVG 文字邊界, 沒有頁面橫向溢出或 SVG 文字越界
- Tag 使用共用分類色與狀態色, 已完成 / 已設定 / 正常使用成功色, 缺值使用中性色, 保留文字. 修改後的測試頁面沒有新增 console error

textlint MCP 兩次檢查 8 份 Markdown, 台灣規則提示從 20 項降至 8 項, 剩下是 inline code 邊界的斷句誤判, 已逐項核對. 新增資料盤點文件回傳 0 項提示. 繁中 UI literals 與英文語系檔抽取文字的通用規則回傳 0 項提示, 不代表英文完整文法檢查. 日文全部字串抽成段落時產生大量標籤句尾誤報, 保留短標籤, 人工核對非句尾規則並修正冗詞與標點. 針對來源, 額度, 傳輸, 未分類, 平均及 P1 等 6 段完整 tooltip 再使用日文規則檢查, 回傳 0 項提示

大型回應 debug 使用大於 64 KiB 的合成內容, 比較標準 HTTP 服務與實際 handler 的未壓縮 / gzip 完整性. 此環境的標準服務先發生 TimeoutError, 因此 handler 大型回應驗證跳過, 不能記為通過. 既有本機服務的 /api/instance 正常, /api/snapshot 回應標頭可取得, 內容接收逾時, 完整即時資料盤點受此限制. 沒有據此修改壓縮策略或停止既有服務

重跑 debug: Windows 設定 `$env:PYTHONPATH="src"` 後, 執行 `python -m unittest discover -s tests -p test_http_snapshot.py -v`. 其他系統設定 `PYTHONPATH=src` 再執行相同測試. 此測試只啟動與關閉自己的 loopback 合成服務

效能比較使用相同 85 個 session fixture, 修改前程式取自 ae8bdc4, 每次初始化後量測 20 次 refresh + snapshot. 最終程式的兩組量測改變先後順序, 更新耗時中位數為修改前 66.8 / 66.9 ms, 修改後 97.4 / 60.2 ms, CPU 中位數為修改前 62.5 ms, 修改後 62.5 / 46.9 ms. 最後一組冷啟動各為單次樣本, 修改前 209 ms / 125 ms CPU, 修改後 199 ms / 156 ms CPU. 波動不足以判定加速或退步. session 增量讀取皆為 0 bytes, 最後一次 snapshot gzip 大小從 34,596 增至 36,495 bytes, 增量約 1.9 KiB, 來自來源與摘要 metadata. 未重跑 5000 個 session 的效能測試

設計依據與後續驗收規則見 [維護與驗收](maintenance.md). 版本與套件驗證依既有 release workflow 執行, 各平台結果見 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml), 實際發佈產物見 [v0.2.2](https://github.com/gaze9999/local-activity-monitor/releases/tag/v0.2.2). v0.2.1 的已發佈驗證保留如下

## 2026-10-04, 0.2.1

Windows, Python 3.14.7 與 Codex in-app browser. 沿用隔離 session / Jev fixture, 新增可提供 credits 的未知 MCP 與 SQL 診斷紀錄. 沒有讀取正式對話內容或改寫來源設定

- 118 項 unittest 通過, 2.528 秒. 包含按需 SQL 身份 / 路徑 / HTTP 邊界, literal 解析, 多筆 INSERT 憑證遮蔽, 內容上限與 snapshot 不保存 SQL
- Python 3.10 AST, en / ja 1264 個 key, README 與治理文件連結通過. 三個角色 TOML 符合必要欄位及 sandbox 設定, 保留 model / effort 繼承. 本機角色檔解析不代表目前對話已重新載入
- 實際頁面確認 SQL 內容按需顯示 SELECT 1, MCP 使用明細涵蓋 11 個來源, 舊排行設定更新為前 5 名, 5 / 10 / 20 / 全部可直接選擇並保存
- 觀察來源位於四張摘要卡下方, 預設收合, 展開選擇可在重整後保留. 卡片, 排行及 modal 的內部連結定位來源頁首. 未觀察到連線狀態時不顯示尚未確認 tag
- 動態模型篩選實際只保留選定模型, 重整後保留. error / info 與 xhigh 使用英文及不同顏色. 其餘等級的顏色以共用 CSS 規則檢查
- 主頁排序關閉時不顯示拖曳提示, 開啟後顯示, 關閉後隱藏. modal 開關維持獨立. 實際切換 en / ja, 檢查數量單位, 設定分類與分頁控制, 修正保留舊語言及錯誤單位
- 1280 px / 390 px 與來回縮放檢查 MCP 卡片, 數量選單, 來源摘要及監測卡片, 沒有頁面橫向溢出或卡片重疊. 多欄配置固定欄位順序, 不因卡片高度變更交換左右. 桌面表格保留容器內橫捲
- 測試頁面沒有新增 console error / warn. 這次沒有重跑 5000 個 session 效能測試或實際指標拖曳手勢

## 2026-10-04, 0.2.0

本機使用 Windows, Python 3.14.7 與 Codex in-app browser. UI 使用隔離的 85 個 session fixture 與 4 筆 Jev telemetry, 預設載入 20 個近期檔案. 範例圖片只包含示範資料. 未變更正式 MCP 或 Jev 設定

| 檢查 | 結果 |
| --- | --- |
| Python unittest | 114 項通過, 3.941 秒. 涵蓋 collector metadata, lifecycle 分輪回補與 checkpoint, Skills 去重與重啟, 錯誤保留 / 遮蔽 / 上限, SQL literal 辨識, 用量快照, 未知 MCP 指標與 tags, 並行呼叫狀態, Jev 未知操作與損壞欄位隔離, HTTP / CSP / 來源設定 |
| JavaScript | Node 語法檢查通過, 已載入的測試頁面沒有新增 console error |
| Python 相容語法 | 16 個後端與建置入口通過 Python 3.10 AST 語法檢查, 本機實際執行版本為 3.14.7 |
| 設定 | 數量四捨五入, 更新最小 1 秒, MCP 自訂標籤匯入 / 匯出 / 還原, 舊設定的 recording=false 不覆寫來源, 全部還原保留來源紀錄狀態 |
| 導覽與排序 | Skills 獨立主 Tab, MCP 動態來源子頁, Log 子頁, 單列捲動導覽. Modal 與主頁排序開關分開, Alt + 方向鍵可移動並保存 |
| 表格 | 主頁與子頁依資料來源呈現, 預設每頁最多 20 筆, 篩選預設收合. 對話搜尋 / 清除與分頁保留, 名稱入口與整列入口不重複 |
| 圖表 | 直條 / 堆疊有刻度與單位, 標籤按寬度整組傾斜與省略, tooltip 保留完整文字. 圓餅 / 環圈資訊另列. 額度 / 分類沒有平均 / P99 / P1, 耗時 / 趨勢保留有效統計 |
| MCP | 來源按設定與工具紀錄發現, 未知來源有共用操作與指標區塊. 本機呼叫 / 回傳 / 傳輸失敗提供被動連線狀態, 不送出 MCP 探測呼叫 |
| 明細 | 錯誤的 HTTP code, Request / Trace / Call ID 與關聯對話. MCP HTTP 嘗試, 已取得的 credits / tokens / 大小. 缺值為 --, 已確認沒有錯誤為無, 已確認的零計數為 0 |
| 排版 | 1265 px 桌面與 390 px viewport, 14 / 18 px 字體. 所檢查圖表, 監測卡片與 modal 沒有橫向溢出. 單位避開峰值, 日期 / tooltip / modal 排序開關保留在範圍內 |

圖表與卡片拖曳的開關, 區域邊界與指標處理以程式碼檢查, 排序與保存以鍵盤操作驗證. 本輪沒有實際操作指標拖曳手勢

## 需求回顧

- 總覽依重要性預設顯示 12 個區塊, 帳戶用量與剩餘額度分開, 相同資料預設只保留一種圖表. 活動摘要獨立排序
- 設定改為直接分類, MCP 來源可搜尋與編輯用途 / 標籤. 來源紀錄狀態由來源決定, 沒有重複的 Jev 開關
- MCP 依目前取得的欄位顯示專有指標與本機紀錄, 未知操作 / 狀態保留. 只有全部來源頁顯示完整來源卡片, 資料來源說明只留在各頁
- 文字精簡, 以台灣用語與半形標點呈現. 必要解釋置於 tooltip, modal 關閉不重新彈出原提示. 成功通知短暫浮動, 還原預設只在有自訂值時顯示
- 對話狀態與 Skills 有上限的保存與回補. SQL 只計可辨識的工具操作與診斷, 不把來源未記錄的 Codex 內部 SQLite 讀寫當成零次使用

## Fixture 效能

85 個 session, 170 筆檔案操作, 4 筆 Jev 紀錄. 初次整理 513.53 ms. 後續無新增資料時, 整理與 snapshot 深複製共 20 輪, 平均 41.65 ms, P99 46.05 ms, P1 39.12 ms, 中位數 41.54 ms, 最大 46.09 ms. 額外 session 讀取量為 0 bytes

同批增量程序 CPU 時間平均 40.62 ms, P99 46.88 ms. 另行啟用 tracemalloc 的 Python 配置峰值為 2.4 MiB, 此值不包含程序 RSS. Snapshot JSON 412183 bytes, gzip 35187 bytes. 這些結果來自指定 fixture, 尚未驗證 5000 個 session 的常駐效能

## 可攜套件驗證

[release workflow](../.github/workflows/release.yml) 在 Windows x64, macOS x64 / ARM64, Linux x64 / ARM64 建置. 每個原生 job 使用 Python 3.12 執行 unittest, 包含 Python runtime 與第三方授權文件, 再以空白暫存 home 啟動成品, 核對 HTTP 頁面, CSP, 語系資源, 版本, snapshot 與檔案存取邊界. 通過後產生原生壓縮檔, wheel, sdist, source archive 與 SHA256SUMS

建置只從該 workflow 的 Git commit 取檔, 不包含本機 .local, 狀態資料庫或私人活動紀錄. 所有 job 通過才附加到 draft release. 原生建置的實際結果見 [Actions](https://github.com/gaze9999/local-activity-monitor/actions/workflows/release.yml)

本機獨立 HTTP smoke check 遇到 AdGuard 修改回應後讀取逾時, 因此本機結果不能確認原生套件啟動. 本機 UI 已在瀏覽器驗證, 原生套件啟動由各平台的隔離 CI 檢查

## 其他驗證範圍

本輪沒有呼叫 live provider 或付費 API, MCP 連線狀態由本機紀錄取得. macOS / Linux 桌面雙擊與簽章提示未在本機操作. 設定 JSON 內容與匯入已驗證, IAB download 等待未取得實體下載檔, 頁面仍提供可複製 JSON



本次 checked state 為 `64269e3` 之後的未提交工作目錄, Python 3.12.14、Node.js v26.9.0、Playwright CLI 0.1.22 與 Microsoft Edge 154 headless

- Python focused checks 共 43 項通過: `test_release.py` 2、`test_account_projection.py` 4、`test_ui_assets.py` 5、`test_launch.py` 7、`test_bootstrap.py` 25. 封裝測試 mock PyInstaller 與 smoke 程序, 檢查 Windows、macOS、Linux 的 CLI 入口與 archive 組裝, 沒有建立原生二進位檔
- `tests/test_runtime_ui.mjs` 與 `web/app.js` Node 語法檢查通過. CMD / PowerShell 啟動、參數與 Unicode 由 bootstrap tests 檢查, 沒有安裝 Python 或相依套件
- `tests/loading-layout-flow.cjs` 通過: 初次等待、503 斷線、重新取得資料、空帳戶資料及再更新, 初次總覽不必先開用量分頁. 零餘額與 false 保留, model Token 圖可按需呈現, 舊 Steam 設定名稱轉為 Workbench
- 同一 browser flow 在 1600、820、390 px 檢查 9 張監測卡片, 無重疊或頁面水平溢位, 框線保留, 頁尾資料來源仍全寬且在最底下. 桌面截圖另經人工複核
- 本機 textlint 台灣用語 / prh 與人工複核通過. 瀏覽器檢查使用新建的合成來源, 未讀取真實 Codex 活動、帳戶或專案資料
- Fixture 啟動與中斷清理另經 focused check 通過. SQLite transaction context 不會自動關閉 connection, 本輪改為明確關閉後再清理暫存目錄, 解決 Windows 關閉時的檔案鎖定

可重現 UI fixture 由 `tests/serve_loading_fixture.py` 在暫存目錄建立, 只綁定 loopback, 不啟動帳戶 / 工作樹 / 裝置探測. 關閉時釋放服務並清理該暫存目錄. 使用現有工具, 不自動安裝:

```sh
python -X utf8 tests/serve_loading_fixture.py
```

另一個終端機使用程式印出的 URL:

```sh
playwright-cli -s=lam-loading open <fixture-url> --browser=msedge
playwright-cli -s=lam-loading run-code --filename=tests/loading-layout-flow.cjs
playwright-cli -s=lam-loading close
```

UI 流程沒有 pageerror, 注入的 503 會在 console 留下預期的 HTTP 錯誤. 測試重跑會重設該隔離 origin 的頁面偏好, 不可對正式監測服務使用. 受限程序無法存取 loopback / 暫存檔時, 以本次隔離的本機測試程序完成, 未安裝新工具

本輪未執行完整 unittest、原生封裝、平台雙擊驗收、發布或遠端更新. Release workflow 經 source 複核, 本機沒有可用的 YAML parser, 未重跑遠端 CI. Workbench UI 的布局實作與缺項文件在上游工作目錄, LAM 原始碼可讀取相鄰 source, 既有 `workbench-ui.json` 固定版本未更新, 舊封裝仍以內附 UI 為準

## 2026-10-07 共用解析與主題串接

Checked state 為 `64269e3` 之後的未提交工作目錄, Python 3.12.14、Node.js v26.9.0、Playwright CLI 0.1.22 與 Microsoft Edge 154 headless

- `PYTHONPATH=src python -B -m unittest tests.test_content_pages tests.test_ui_assets`: 12/12 通過, 涵蓋按需內容分頁、來源版本、敏感資料遮蔽與共用資產 / CSP
- `tests/test_runtime_ui.mjs` 與 `web/app.js` 語法檢查通過, 此結果不代表所有 collector 或原生封裝已驗證
- `tests/output-detail-flow.cjs` 通過: 工具、MCP、Git 與錯誤明細使用相同解析器, 外層工具來源說明保留, 摺疊時不啟動解析、分頁只讀取所需下一頁、702 個新增行與尾端內容完整、原文保留、關閉取消未完成請求, 共用 Markdown 與 SQL 顏色可顯示. 摺疊狀態以區段名稱恢復, 不將先前的執行資訊狀態套給新輸入區段, 已移除的摺疊區不延遲建立解析器
- Workbench、工作台與簡約主題的深色 / 淺色共 6 個狀態, 解析背景與既有 payload 背景一致, 框線及模式按鈕配色通過實際 computed style 比較. 共用 UI 的 CSS 主題變數由 LAM 自有主題提供, 元件樣式仍由 Workbench UI 管理
- 本機 textlint 台灣用語 / prh 與人工複核完成, 測試來源及畫面全部使用隔離合成資料

重現時先啟動 `tests/serve_loading_fixture.py`, 用其印出的 loopback URL 開啟命名瀏覽器 session, 再執行:

```sh
playwright-cli -s=lam-output run-code --filename=tests/output-detail-flow.cjs
```

瀏覽器沒有 pageerror, 關閉中的分頁要求回報預期的 `net::ERR_ABORTED`. 未測試真實活動內容、所有來源 API、Safari / iPad / WebView、完整 WCAG、CPU / 記憶體峰值或原生封裝. 上游 source 與 LAM 串接已修改, `workbench-ui.json` 固定版本保持原值, 舊封裝沒有自動取得本輪共用 parser. 沒有 commit、push 或 release

Python focused checks 首次在沙箱暫存目錄遭遇 WinError 5, 改以同一組合成測試的隔離本機程序完成, 沒有因該限制修改應用程式或安裝套件

## 2026-10-07 摘要、外觀、更新回饋與結果 metadata

Checked state 為 `64269e3` 之後的未提交工作目錄, Python 3.12.14、Node.js v26.9.0、Playwright CLI 0.1.22 與 Microsoft Edge 154 headless. 使用相鄰 Workbench UI 的未提交 source, `workbench-ui.json` 維持原固定版本, 本節不代表已封裝使用端已更新

- `PYTHONPATH=src python -B -m unittest tests.test_mcp_observation tests.test_content_pages tests.test_ui_assets`: 38/38 通過. MCP 結果可從物件 / JSON 字串的 input_text 區塊取出 status、來源耗時與 Token, 多個結果不合併, 外層 error 保留, 未將 answers / probabilities 本文加入 metadata
- `tests/parser-summary-motion-flow.cjs`: 子分頁預設沒有摘要, 全域與個別 0 / 3 張設定、保存 / 重載 / 匯出 / 匯入及主頁拒絕 0 通過. 54,864 字元 SQL 分頁、排版、完整原文、YAML 結構與編輯後預覽、關閉控制器清理通過
- 同 flow 檢查三語減少動畫設定, 手動及系統 reduced-motion 都停用轉場、進入動畫、光帶與 pseudo 動畫, 靜態狀態保留. 深淺模式與 1280 / 820 / 390 / 320 px 共 8 組原文布局沒有水平溢位
- `tests/appearance-layout-flow.cjs`: 三語主題名稱、保留目前配色、移除重複工作台、自訂強調色 / 本機字型保存與匯入拒絕通過. number 上下箭頭隱藏, 原生 ArrowUp 保留. 延後統計卡四種寬度無重疊, SVG 更新保留節點且不重播進入動畫
- `tests/loading-layout-flow.cjs`: 冷啟動、503、恢復、未知 / 零帳戶值及重新更新通過. 9 張監測卡在 1600 / 820 / 390 px 無重疊 / 溢位, 框線及全寬頁尾保留. 初次使用橫條 skeleton, 已有內容使用柔和光帶, 沒有背景覆蓋或 spinner, 帳戶卡更新前後高度一致
- `tests/distribution-feedback-flow.cjs`: 可操作與唯讀分布的字級 / 高度 / 對齊相同, SVG 遮罩字體與原圖一致、原始節點位置不變、更新不改卡片尺寸. 光帶僅在文字 / 圖形, 底軌與框線不參與. Tooltip 點擊導頁即關閉, 合成截圖另經人工複核
- `tests/font-resize-flow.cjs`: 設定中的 12–18 px 字級全部實際套用並保存, 12 / 14 / 18 px 與 1600 / 820 / 390 / 320 px 共 12 組布局無重疊及水平溢位, 最後回復 fixture 字級 14
- `tests/output-detail-flow.cjs` 依 sticky 複製列的新內容容器更新 selector 後通過, 702 個新增行及尾端 / 原文完整, tool / MCP / Git / error 共用解析、按需分頁、關閉取消與 Markdown 保留. 目前 Workbench / 簡約的深淺模式共 4 個狀態通過, 前述 6 狀態是移除重複主題前的歷史證據
- JavaScript 語法、locale JSON 與 Git diff whitespace 檢查通過. 文件使用本機 textlint 台灣用語 / prh 與人工複核, 畫面和 browser flow 均為隔離合成資料

重現時啟動 `python -X utf8 tests/serve_loading_fixture.py`, 使用印出的 loopback URL 開啟命名 session, 依序執行:

```sh
playwright-cli -s=lam-final run-code --filename=tests/parser-summary-motion-flow.cjs
playwright-cli -s=lam-final run-code --filename=tests/appearance-layout-flow.cjs
playwright-cli -s=lam-final run-code --filename=tests/loading-layout-flow.cjs
playwright-cli -s=lam-final run-code --filename=tests/distribution-feedback-flow.cjs
playwright-cli -s=lam-final run-code --filename=tests/font-resize-flow.cjs
playwright-cli -s=lam-final run-code --filename=tests/output-detail-flow.cjs
playwright-cli -s=lam-final close
```

本次流程沒有 pageerror, 503 與 `net::ERR_ABORTED` 是失敗 / 取消注入的預期結果, 既有 Permissions-Policy 警告保留. 共用元件壓力測試由 Workbench UI 的 `tests/stress-font-flow.cjs` 驗證, 不代表真實 LAM 收集與回補 peak 已改善, 各案例耗時與自有 renderer thread CPU 分開記錄

本輪未執行完整 unittest、真實 Codex / provider API、Safari / iPad / WebView、完整 WCAG、原生封裝或平台雙擊, 分別屬資料來源 / 平台 / 發布驗收. 沒有安裝、改存檔 migration、更新 pin、commit、push 或 release. LAM 的 1.0.0 門檻見 [獨立規劃](release-1.0.md)
