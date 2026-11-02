# 公開內容檢查

需要 Python 3.10 以上與 Git, 每份 clone 都要設定 hooks, 先確認既有 `core.hooksPath` 與 Git hooks, 有自訂內容時保留並串接, 不直接覆蓋

```sh
git config --local --get core.hooksPath
git config --local core.hooksPath .githooks
python tools/check-public-content.py --index
python tools/check-public-content.py --history
python -m unittest discover -s tests -p test_public_content.py
```

`pre-commit` 檢查暫存區的實際位元組, 工作目錄後續修改不會取代已暫存內容, `pre-push` 檢查每個待推送分支與 Tag 的完整可達歷史及註記, detached HEAD 也會納入, shallow clone 須先取得完整歷史, 檢查失敗會停止 commit 或 push

檢查涵蓋 PEM 私鑰標記、RSA / EC / PKCS#8 DER 私鑰結構、私鑰檔名、CRX 封裝與 Windows 個人 / 工作區絕對路徑, 包含二進位內容及常見 UTF-16 / UTF-32、JSON、URL、HTML 編碼, 診斷只輸出物件 ID 與類別, 公開憑證、公鑰、相對路徑及 `C:/path/to/project` 通用範例可保留

經確認的合成測試誤判可記於已暫存的 `.public-content-fixtures.json`, 每筆須包含完整 Git blob ID、來源路徑與審查原因, 僅該 blob 的工作區路徑誤判使用例外, 私鑰內容、檔名與 Tag 註記仍檢查, 檔案內容變更後的新 blob 須重新審查, 不使用路徑或規則類別的廣泛排除

私鑰與簽署檔放在儲存庫外, 已外洩金鑰須撤銷並禁止重用, 產生的 ZIP 與安裝包另外檢查, 此檢查涵蓋上述明確類別, 其他機敏資訊仍須依來源與發布內容審查

## CI 與伺服器保護

自動 CI 僅在推送大版本 Tag `vX.0.0` 或 1.x 以上中版本 Tag `vX.Y.0` 時執行, 一般 push、PR、0.x 中版本、patch 與預發布 Tag 不自動執行, 手動入口保留

`public-content` 不設為必要分支檢查, 預設分支與既有 `dev` 分支保留禁止刪除及強制推送保護, 已發布 Tag 禁止更新與刪除, GitHub rulesets 是伺服器設定, 不會由 clone 自動還原, 搬移儲存庫時須重新核對

本機 hooks、GitHub CI、伺服器 rulesets 與 GitHub secret push protection 各有自己的設定與作用範圍, 本次不變更 secret push protection

## 維護來源

檢查器與隔離案例由 [Codex Toolkit](https://github.com/gaze9999/codex-toolkit/blob/main/tools/check-public-content.py) 維護, 各儲存庫保留相同的獨立副本, 不依賴其他 checkout 或網路下載, 更新時比對來源與 hash 並執行既有隔離案例

檢查器 SHA-256: `7035ffeca4c0eb62d46b8db52447e38bd12b65824a7452854135cd2775e9b5d6`
