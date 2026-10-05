"use strict";
const preferenceKey="local-activity-monitor.preferences.v1";
let dragEnabled=false;
let preferences={};try{preferences=JSON.parse(localStorage.getItem(preferenceKey)||"{}");if(!preferences||typeof preferences!=="object"||Array.isArray(preferences))preferences={};}catch{}
function editableCopy(value){return typeof value==="string"&&value===value.trim()&&value.length>=2&&value.length<=400&&/[A-Za-z\u3400-\u9fff]/.test(value)&&!/^(?:ms|px|bytes|tokens|分鐘|小時|秒鐘|\d+\s*(?:筆|項|秒|分鐘|小時|天))$/i.test(value);}
preferences.copy=Object.fromEntries(Object.entries(preferences.copy||{}).filter(([key,value])=>editableCopy(key)&&typeof value==="string"&&value.length<=400).slice(0,500));
const readableCopyKeys={"程式版本":"LAM 版本","程序位元數":"執行架構"," · 觀察來源":" · 資料來源"," 觀察開關":" 檢查開關","Git 觀察已關閉":"Git 檢查已關閉","Log 觀察已停用":"Log 檢查已停用","Log 觀察與程式事件保存":"Log 檢查與程式事件保存","SQL 操作觀察已停用":"SQL 操作檢查已停用","技能文件觀察已關閉":"技能文件檢查已關閉","介面偏好保存在此瀏覽器, 工具用途由觀察設定管理":"介面偏好保存在此瀏覽器, 工具用途由檢查設定管理","保存外觀, 觀察開關, 顯示數量, 圖表, 表格, Tab 順序與介面文字":"保存外觀, 檢查開關, 顯示數量, 圖表, 表格, Tab 順序與介面文字","啟用或停用此觀察項目, 不會修改來源紀錄":"啟用或停用此檢查項目, 不會修改來源紀錄","啟用的觀察項目":"啟用的檢查項目","將套用外觀, 觀察開關, 顯示數量, 圖表, 表格與介面文字. Jev 紀錄將設為 ":"將套用外觀, 檢查開關, 顯示數量, 圖表, 表格與介面文字. Jev 紀錄將設為 ","對話與工具錯誤觀察已停用":"對話與工具錯誤檢查已停用","工作觀察台":"工作檢查台","工具紀錄超過每個 session 或整體保留上限時, 移除舊資料的累計筆數. 重新建立 Codex 觀察後重新累積":"工具紀錄超過每個 session 或整體保留上限時, 移除舊資料的累計筆數. 重新建立 Codex 檢查後重新累積","已觀察到開始與結束的工作時間加總. 尚在執行的工作持續計時, 缺少起始紀錄時顯示 --":"已取得開始與結束的工作時間加總. 尚在執行的工作持續計時, 缺少起始紀錄時顯示 --","本機觀察統計":"本機活動統計","查看診斷與觀察程式事件":"查看診斷與監測程式事件","此頁使用目前已載入的觀測資料":"此頁使用目前已載入的檢查資料","此項觀察已關閉":"此項檢查已關閉","目前 Codex 觀察累計讀取的 session 位元組數, 包含初次尾端, 增量與狀態回查. 重新建立 Codex 觀察後重新累積":"目前 Codex 檢查累計讀取的 session 位元組數, 包含初次尾端, 增量與狀態回查. 重新建立 Codex 檢查後重新累積","統計觀察已關閉":"統計檢查已關閉","觀察來源":"資料來源","觀察來源與讀取狀態":"資料來源與讀取狀態","觀察分類":"檢查分類","觀察已關閉":"檢查已關閉","觀察程式":"監測程式","觀察設定已套用":"檢查設定已套用","觀察設定或介面文字格式無效":"檢查設定或介面文字格式無效","觀察開關":"檢查開關","觀察項目":"檢查項目","MCP 觀察":"MCP 檢查","資料觀察":"資料檢查","啟用需要的觀察項目, 停用後會停止對應資料的讀取與辨識":"啟用需要的檢查項目, 停用後會停止對應資料的讀取與辨識","這裡控制來源是否產生紀錄. 與本程式的資料觀察開關分開設定":"這裡控制來源是否產生紀錄. 與本程式的資料檢查開關分開設定","已觀測":"已檢查","最後觀測":"最後檢查","來源回報的額度使用百分比, 保留原始視窗長度與觀測時間":"來源回報的額度使用百分比, 保留原始視窗長度與最近檢查時間","已觀察到開始與結束的工作時間加總. 尚在執行的工作持續計時":"已取得開始與結束的工作時間加總. 尚在執行的工作持續計時","只觀察可辨識的工具 SQL 操作與來源提供的 SQL 診斷. 0 表示目前載入範圍沒有紀錄":"只檢查可辨識的工具 SQL 操作與來源提供的 SQL 診斷. 0 表示目前載入範圍沒有紀錄","外觀, 語言, 顯示數量, 所有排序, 圖表, 表格, 介面文字與觀察設定都會還原. Jev 紀錄會停用, 已有觀察紀錄保留":"外觀, 語言, 顯示數量, 所有排序, 圖表, 表格, 介面文字與檢查設定都會還原. Jev 紀錄會停用, 已有檢查紀錄保留","依來源名稱與已觀察到的工具名稱分類":"依來源名稱與已取得的工具名稱分類","第 1 百分位數, 約有 1% 樣本不超過此值. 使用線性內插, 搭配平均值與 P99 觀察分布":"第 1 百分位數, 約有 1% 樣本不超過此值. 使用線性內插, 搭配平均值與 P99 檢查分布","外觀, 語言, 顯示數量, 所有排序, 圖表, 表格, 介面文字與觀察設定都會還原. Jev 紀錄會啟用, 已有觀察紀錄保留":"外觀, 語言, 顯示數量, 所有排序, 圖表, 表格, 介面文字與檢查設定都會還原. Jev 紀錄會啟用, 已有檢查紀錄保留","外觀, 語言, 顯示數量, 排序, 圖表, 表格, 介面文字與觀察設定都會還原":"外觀, 語言, 顯示數量, 排序, 圖表, 表格, 介面文字與檢查設定都會還原","將套用外觀, 觀察開關, 顯示數量, 圖表, 表格與介面文字":"將套用外觀, 檢查開關, 顯示數量, 圖表, 表格與介面文字","已載入的觀測資料":"已載入的檢查資料","觀察停用":"檢查停用","觀察摘要":"活動摘要","從目前 MCP 設定取得啟用的來源名稱與用途, 再合併已觀察到的呼叫來源. 只選取顯示需要的欄位, 不顯示啟動參數與憑證":"從目前 MCP 設定取得啟用的來源名稱與用途, 再合併已取得的呼叫來源. 只選取顯示需要的欄位, 不顯示啟動參數與憑證","來源已載入的觀測資料. 下方列出可取得的欄位, 讀取結果與上限":"來源已載入的檢查資料. 下方列出可取得的欄位, 讀取結果與上限","本機觀察":"本機檢查","觀察已暫停":"檢查已暫停","依本機紀錄觀察 Codex 連線, 最近 5 分鐘有模型回報時顯示最近有回應":"依本機紀錄檢查 Codex 連線, 最近 5 分鐘有模型回報時顯示最近有回應","觀測時間":"最近檢查時間"};
function currentCopyKey(value){if(typeof value==="string")value=readableCopyKeys[value]||value;return typeof value==="string"?value.replace(/\b(?:Subagents?|Skills?|Provider|Sandbox)\b/g,term=>({Subagent:"子代理程式",Subagents:"子代理程式",Skill:"技能",Skills:"技能",Provider:"供應商",Sandbox:"沙盒"})[term]).replace(/([\u3400-\u9fff]) 技能/g,"$1技能").replace(/技能 ([\u3400-\u9fff])/g,"技能$1").replace("沙盒 模式","沙盒模式").replace("Cache write input","快取寫入 Token").replace("查看 Global AGENTS.md","查看全域 AGENTS.md").replace("查看 Project AGENTS.md","查看專案 AGENTS.md").replace("Global AGENTS.md","全域 AGENTS.md"):value;}
for(const [key,value]of Object.entries(preferences.copy)){const current=currentCopyKey(key);if(current!==key){if(!Object.hasOwn(preferences.copy,current))preferences.copy[current]=value;delete preferences.copy[key];}}
function validDisplay(value){return value&&["mainSummary","subSummary"].every(key=>value[key]==null||Number.isInteger(value[key])&&value[key]>=1&&value[key]<=8)&&(value.lines==null||[3,5,10].includes(value.lines))&&(value.heatmap==null||typeof value.heatmap==="boolean")&&Array.isArray(value.options)&&value.options.length>=1&&value.options.length<=8&&value.options.every(n=>Number.isInteger(n)&&n>=1&&n<=200)&&new Set(value.options).size===value.options.length&&[...value.options,"all"].includes(value.ranking)&&[...value.options,"all"].includes(value.table);}
const displayDefaults={options:[5,10,20],ranking:5,table:10,lines:3,mainSummary:4,subSummary:3};
const summaryLibrary=new Map();
let display=validDisplay(preferences.display)?preferences.display:{...displayDefaults,options:[...displayDefaults.options]};
const copyCatalog=new Set(),copyTargets=[];
let locale=["zh-TW","en","ja"].includes(preferences.locale)?preferences.locale:"zh-TW",translations={},translatedValues=new Set(),tagIdentities=new Map();
const defaultCopy={"資料保留與讀取":"監測資料與保存狀況","工作檢查台":"工作監測台","對話, 工具與專案操作":"對話與工具活動監測","程式狀態":"監測程式","Log 紀錄":"來源 Log","工具使用情況":"工具活動","Log 檢查與程式事件保存":"來源 Log 讀取與歷史回補","整理狀態":"資料更新狀態","本輪整理耗時":"本次資料更新耗時","每輪讀取上限":"每次讀取上限","保留效能樣本":"效能樣本數","保留狀態事件":"執行事件數","待完成的紀錄片段 (bytes)":"待解析資料 (bytes)","片段保留上限 (bytes)":"資料暫存上限 (bytes)","累計移除的舊工具紀錄":"已清理工具紀錄","累計 session 讀取量 (bytes)":"累計讀取量 (bytes)","損壞紀錄 (行)":"無法解析的紀錄 (行)","上次資料大小 (bytes)":"最近回應大小 (bytes)","上次傳輸大小 (bytes)":"最近傳輸大小 (bytes)","Model":"模型","Model 設定更新時間":"模型設定時間","Model 分布":"模型分布","Model Token 分布":"模型 Token 分布","Model 用量分析":"模型用量分析","Reasoning 等級":"推理等級","Input":"輸入 Token","Cached input":"快取輸入 Token","Output":"輸出 Token","Reasoning output":"推理輸出 Token","Total":"Token 合計","手動對話":"一般對話","exec 辨識":"程式碼辨識","exec 內辨識到的工具":"程式碼中辨識到的工具","原始文案":"預設文字","顯示文字":"自訂文字","用途說明":"工具用途","位置":"執行位置","過長紀錄 (行)":"超過長度上限 (行)","工具回傳狀態":"紀錄狀態","Session 讀取量 (bytes)":"檔案讀取量 (bytes)","Session 讀取量趨勢":"檔案讀取量趨勢"};
function preset(value){value=readableCopyKeys[value]||value;const base=defaultCopy[value]||value;return translations[locale]?.[base]||translations[locale]?.[value]||base;}
function ui(value){if(editableCopy(value)&&!translatedValues.has(value))copyCatalog.add(value);return Object.hasOwn(preferences.copy,value)?preferences.copy[value]:preset(value);}
const fieldDescriptions={"快取命中率 (%)":"對話最新累計快取輸入除以輸入 Token","參考次數":"每次工具操作對同一網址計一次, 同次操作的重複網址合併","讀取量 (bytes)":"讀取量以來源提供的 bytes_read 計算","寫入量 (bytes)":"寫入量以來源提供的 bytes_written 計算","送出文字大小 (UTF-8 bytes)":"送出文字以 UTF-8 計算大小","目前檔案大小 (bytes)":"讀取檔案頁面或點擊檢查時取得目前檔案容量","最後更新時間":"本機檔案目前的最後修改時間, 依作業系統資料顯示","資料狀態":"額度取自最後讀取的來源紀錄, 不會隨時間推估用量. 到達重設時間後等待新的來源回報","Low 1% (P1)":"第 1 百分位數, 約有 1% 樣本不超過此值. 使用線性內插, 搭配平均值與 P99 檢查分布","統計範圍":"平均值, P99 與 Low 1% 使用的資料範圍, 同時列出實際參與統計的樣本數","Credits 餘額":"來源最近回報的 credits 餘額","已使用":"來源回報的額度使用百分比, 保留原始視窗長度與最近檢查時間","剩餘比例":"以 100 減去來源回報的使用百分比, 快照超過重設時間後需等來源更新","重設時間":"來源提供的額度視窗重設時間, 依目前語系與本機時區顯示","快取命中比例 (%)":"已載入對話的快取輸入除以輸入 Token","來源檔案":"可用來對照原始紀錄的檔名, 搭配時間與紀錄 ID 追查","Request ID":"來源提供的請求識別碼, 用來對照相同請求的診斷事件","Trace ID":"來源提供的追蹤或關聯識別碼, 用來查找同一操作的事件","Model":"目前來源提供的模型名稱","Reasoning 等級":"來源提供的推理強度設定","Input":"對話最新累計輸入 Token, 包含來源計入的快取輸入","Cached input":"已包含在輸入 Token 中的快取部分, 不再額外加總","Output":"來源提供的累計輸出 Token","Reasoning output":"來源提供的推理輸出 Token, 是否包含在輸出量中依來源定義","Total":"來源提供的最新累計 Token, 不加總歷次更新數值","狀態":"本機紀錄提供的最新對話狀態","工作總耗時 (秒)":"已取得開始與結束的工作時間加總. 尚在執行的工作持續計時","SQL 耗時 (ms)":"只顯示 SQL 診斷來源提供的個別操作耗時. 不以外層工具時間代替","影響列數":"SQL 診斷來源提供的 rows_affected. 不從查詢內容推算","回傳列數":"SQL 診斷來源提供的 rows_returned","工具整次耗時 (ms)":"整次工具呼叫的執行時間","工具回傳狀態":"工具紀錄顯示整次呼叫的回傳狀態, 診斷紀錄顯示來源回報的錯誤","紀錄方式":"顯示直接工具呼叫或程式碼中的操作位置","CPU 時間":"此監測程式更新資料時使用的 CPU 時間","平均值":"目前統計範圍內有效樣本的算術平均. 活動次數以每個時間格計算, 包含零活動格","P99":"第 99 百分位數, 約有 99% 樣本不超過此值. 使用線性內插, 少量樣本時需搭配樣本數判讀","樣本數":"目前統計區間內有效的資料筆數或時間格數","資料庫位置":"紀錄中能確認的 SQLite 檔案位置","SQL 操作":"從工具參數或 literal 程式碼辨識的 SQL 指令, 點開操作明細可查看來源記錄的內容","Log 等級":"沿用來源提供的 error, warn, info, debug, trace 等等級. 未知的新等級保留原值","已讀取 (bytes)":"此來源自啟動至今累計讀取的位元組數, 包含歷史回補","未辨識 (行)":"來源格式與目前解析方式不相符的行數, 可用來判斷是否需要更新解析器","歷史回補":"分批讀取程式啟動前的來源紀錄, 補上最近 24 小時可取得的錯誤摘要","資料來源與讀取範圍":"列出此頁使用的實際來源位置, 讀取方式與涵蓋範圍","每頁筆數":"每頁顯示的資料列數. 修改後立即套用並保存在此瀏覽器","檢查項目":"啟用或停用指定資料的讀取與辨識, 不會修改來源紀錄"};
for(const [key,text]of Object.entries(fieldDescriptions)){ui(key);ui(text);}
function fieldDescription(label){const entry=Object.entries(fieldDescriptions).find(([key])=>key===label||ui(key)===label);return entry?ui(entry[1]):null;}
function editableLabels(values){return Object.defineProperties({},Object.fromEntries(Object.entries(values).map(([key,text])=>{ui(text);return [key,{enumerable:true,get:()=>ui(text)}];})));}
function discoverCopy(){
  const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);let text;
  while(text=walker.nextNode()){if(text.parentElement.closest("script,style,[data-copy=ignore]"))continue;const original=text.textContent,base=original.trim();if(!editableCopy(base)&&!/^[\u3400-\u9fff]$/.test(base))continue;const start=original.indexOf(base);copyTargets.push({text,base,before:original.slice(0,start),after:original.slice(start+base.length)});ui(base);if(/^(?:H[1-4]|TH)$/.test(text.parentElement.tagName))text.parentElement.dataset.copyBase=base;}
  for(const el of document.querySelectorAll("[placeholder],[aria-label],[title]"))for(const attribute of ["placeholder","aria-label","title"]){const base=el.getAttribute(attribute);if(!el.closest("[data-copy=ignore]")&&editableCopy(base)){copyTargets.push({el,attribute,base});ui(base);}}
}
function applyCopy(){for(const target of copyTargets)if(target.text)target.text.textContent=target.before+ui(target.base)+target.after;else target.el.setAttribute(target.attribute,ui(target.base));}
async function loadLocales(){try{const response=await request("/locales.json",{cache:"no-store"});if(!response.ok||!response.data?.en||!response.data?.ja)throw new Error();translations=response.data;translatedValues=new Set(Object.values(translations).flatMap(Object.values));}catch{locale="zh-TW";$("action-message").textContent="語言檔無法讀取, 使用繁體中文";}}
function applyLanguage(){tagIdentities=new Map(Object.entries(translations[locale]||{}).map(([key,value])=>[value,defaultCopy[key]||key]));for(const [key,value]of Object.entries(preferences.copy||{}))tagIdentities.set(value,defaultCopy[key]||key);preferences.locale=locale;document.documentElement.lang=locale;numberFormat=new Intl.NumberFormat(locale);dateFormat=new Intl.DateTimeFormat(locale,{year:"numeric",month:"numeric",day:"numeric",hour:"2-digit",minute:"2-digit",second:"2-digit",hour12:false});applyCopy();applyAppearance();document.querySelector(".settings-layout>.modal-tabs")?.setAttribute("aria-label",ui("設定分類"));$("language-select").value=locale;for(const input of document.querySelectorAll(".pagination input"))input.setAttribute("aria-label",ui(" 頁碼").trim());for(const tab of document.querySelectorAll(".modal-tabs button[data-copy-base]"))tab.textContent=ui(tab.dataset.copyBase);if(data)render(data);syncSourceWindow();windowOptions($("source-window-global"));$("source-window-global").value=sourceWindows.global;const draft=$("tab-source-window").value;windowOptions($("tab-source-window"),true);$("tab-source-window").value=draft;setupTabDescriptions();for(const el of document.querySelectorAll("[data-help-key]"))el.dataset.help=ui(el.dataset.helpKey);syncHelp();}
const $ = id => document.getElementById(id);
let numberFormat=new Intl.NumberFormat(locale),dateFormat=new Intl.DateTimeFormat(locale,{year:"numeric",month:"numeric",day:"numeric",hour:"2-digit",minute:"2-digit",second:"2-digit",hour12:false});
const fmt = value => value == null ? "--" : numberFormat.format(value);
function when(value){if(!value)return "--";const date=new Date(value);return Number.isFinite(date.getTime())?dateFormat.format(date):ui("未知");}
const labels = {
  status:editableLabels({ok:"完成",success:"完成",ready:"就緒",passed:"通過",failed:"失敗",error:"錯誤",timeout:"逾時",cancelled:"已取消",canceled:"已取消",partial:"部分完成",extracted:"已取得內容",written:"已寫入",preview:"預覽",unchanged:"未變更",fallback:"備用結果",skipped:"略過",dry_run:"試跑",running:"執行中",completed:"已完成",observed:"未知",pending:"等待中",queued:"排隊中",missing_dependencies:"缺少相依",used:"已執行",not_needed:"無需執行",disabled:"已停用"}),
  type:editableLabels({codex:"Codex",work:"ChatGPT Work",chat:"ChatGPT 對話",unknown:"未知"}),
  environment:editableLabels({local:"本機",cloud:"雲端",remote:"遠端",unknown:"未知"}),
  trigger:editableLabels({user:"手動對話",dot:"Dot",orbit:"Dot",schedule:"排程",automation:"排程",heartbeat:"Heartbeat",subagent:"子代理程式",guardian_review:"自動審查",unknown:"未知"})
};
const tokenLabels = editableLabels({input_tokens:"Input",cached_input_tokens:"Cached input",output_tokens:"Output",reasoning_output_tokens:"Reasoning output",total_tokens:"Total"});
const observationLabels = editableLabels({codex:"對話紀錄",jev:"Jev 用量",metadata:"對話名稱與分類",usage:"用量與額度快照",git:"Git 操作",jev_calls:"Jev 送出與回傳內容",skills:"技能讀取",checks:"test / build / lint 操作",tool_events:"工具呼叫明細",mcp:"MCP 操作與結果",web:"網路工具與參考網址",files:"檔案讀寫紀錄",errors:"對話與工具錯誤",logs:"Log 檢查與程式事件保存",sqlite:"SQL / SQLite 操作"});
const actionLabels=editableLabels({read:"讀取",write:"寫入",change:"變更",deployment:"部署",operation:"操作"}),fileLabels=editableLabels({added:"新增",modified:"修改",deleted:"刪除",moved:"移動",write:"寫入",read:"讀取"});
const activityWindows=["1h","24h","7d","all"];
const windowPreference=preferences.sourceWindows||{};
const sourceWindows={global:activityWindows.includes(windowPreference.global)?windowPreference.global:activityWindows.includes(preferences.inputs?.window)?preferences.inputs.window:"24h",tabs:Object.fromEntries(Object.entries(windowPreference.tabs||{}).filter(([key,value])=>/^[a-z][a-z:-]{0,79}$/.test(key)&&activityWindows.includes(value)).slice(0,64))};
preferences.sourceWindows=sourceWindows;
function sourceWindowKey(name){return name==="codex"?"codex:"+conversationSource:name==="errors"?"errors:"+diagnosticSource:name==="workflow"?"workflow:"+activitySource:name;}
function activeSourceWindow(){const name=document.querySelector('[data-tab][aria-selected="true"]')?.dataset.tab||"overview";return sourceWindows.tabs[sourceWindowKey(name)]||sourceWindows.global;}
function windowOptions(select,inherit=false){select.replaceChildren(...(inherit?["inherit",...activityWindows]:activityWindows).map(value=>{const option=node("option",value==="inherit"?ui("沿用全域設定"):sourceWindowLabels[value]);option.value=value;return option;}));}
function syncSourceWindow(){
  for(const tab of document.querySelectorAll("[data-tab]")){const name=tab.dataset.tab,head=$("view-"+name).querySelector(".section-head"),actions=head.querySelector(".section-actions")||node("div",null,"section-actions");if(!actions.parentElement)head.append(actions);head.querySelector(".source-window-badge")?.remove();let select=name==="mcp"?$("window"):$("source-window-"+name);if(!select){select=node("select",null,"source-range-select");select.id="source-window-"+name;select.addEventListener("change",()=>{const key=sourceWindowKey(name);if(select.value==="inherit")delete sourceWindows.tabs[key];else sourceWindows.tabs[key]=select.value;applySourceWindow();});actions.prepend(select);}windowOptions(select,true);select.options[0].textContent=sourceWindowLabels[sourceWindows.global]+" ("+ui("全域")+")";select.value=sourceWindows.tabs[sourceWindowKey(name)]||"inherit";select.setAttribute("aria-label",ui("來源紀錄範圍"));bindHelp(select,ui("可覆寫全域範圍, 圖表範圍與表格篩選會在此範圍內套用"));const gear=head.querySelector(".tab-settings-button");if(gear){gear.setAttribute("aria-label",tab.textContent+ui(" Tab 設定"));gear.title=ui("Tab 設定");}}
}
function applySourceWindow(){version++;syncSourceWindow();logData=null;saveView();refresh();if(data&&!$("view-logs").hidden)loadLogs();}
function setupSourceWindowSettings(){
  const global=node("label",null,"setting-row"),globalSelect=node("select");globalSelect.id="source-window-global";windowOptions(globalSelect);globalSelect.value=sourceWindows.global;global.append(node("span",ui("來源紀錄範圍")),globalSelect);$("refresh-interval").closest(".setting-row").after(global,node("p",ui("列表與操作統計一起套用. 最新對話狀態, 累計 Token 與帳戶額度保留來源回報值"),"muted"));globalSelect.addEventListener("change",()=>{sourceWindows.global=globalSelect.value;applySourceWindow();});
  const section=node("section"),label=node("label",null,"setting-row"),select=node("select");select.id="tab-source-window";windowOptions(select,true);label.append(node("span",ui("來源紀錄範圍")),select);section.append(node("h4",ui("此 Tab 的來源紀錄")),label,node("p",ui("可覆寫全域範圍, 圖表範圍與表格篩選會在此範圍內套用"),"muted"));$("tab-chart-controls").before(section);select.addEventListener("change",()=>{const key=sourceWindowKey(currentTabSettings);if(select.value==="inherit")delete sourceWindows.tabs[key];else sourceWindows.tabs[key]=select.value;applySourceWindow();feedback("tab-settings-message",ui("來源紀錄範圍已套用"));});syncSourceWindow();
}
const viewInputs=["thread-search","filter-type","filter-environment","filter-project","filter-trigger","filter-status","filter-reasoning","filter-model","page-size","filter-git","window","filter-mcp-category","filter-mcp-server","filter-mcp-result","file-search","filter-file-operation","filter-file-method","filter-file-project","filter-file-tool"];
let pendingStatus=preferences.inputs?.["filter-status"];
let pendingFileProject=preferences.inputs?.["filter-file-project"],pendingFileTool=preferences.inputs?.["filter-file-tool"];
let loadedRevision=null,pendingRevision=null,preferencesApplied=false,pendingProject=preferences.inputs?.["filter-project"],pendingGit=preferences.inputs?.["filter-git"],pendingReasoning=preferences.inputs?.["filter-reasoning"],pendingModel=preferences.inputs?.["filter-model"];
let threadIndex=new Map();
let logData=null,logBusy=false,logQueued=false;
let data = null, page = 1, pageCount = 1, refreshTimer = null, refreshSeconds = 10;
let idlePaused=false,activityBusy=false,scheduledSeconds=0;
const fileSnapshots=new Map(),fileSnapshotRequests=new Set();let fileSummaryBusy=false;
let busy = false, refreshQueued = false, settingsBusy = false, version = 0, detail = null;
let lazyLimit=50,mcpPage=1,docsPage=1,docTools=[],chartSelection=null;
const docDrafts=new Map();
const chartViews=new Map(),systemDark=matchMedia("(prefers-color-scheme: dark)");
let appearance={mode:"dark",theme:"slate",accent:"green",font:14,...preferences.appearance};

function node(tag,text,cls){const el=document.createElement(tag);if(text!=null)el.textContent=text;if(cls)el.className=cls;return el;}
function button(text,action,cls){const el=node("button",text,cls);el.type="button";el.addEventListener("click",action);return el;}
function cell(row,text,cls){const el=node("td",text==null?null:text===ui("未知")||text===ui("--")?"--":text,cls);el.dataset.columnIndex=row.children.length;row.append(el);return el;}
function valueCell(row,value,cls){const el=cell(row,value==null?"--":fmt(value),cls);if(Number.isFinite(value))el.dataset.heatValue=String(value);el.dataset.sortValue=value==null?"":String(value);return el;}
function cacheHit(tokens){const input=tokens?.input_tokens,cached=tokens?.cached_input_tokens;return Number.isFinite(input)&&input>0&&Number.isFinite(cached)&&cached>=0&&cached<=input?Math.round(cached/input*10000)/100:null;}
function tag(text,cls=""){
  const value=text===ui("未知")?"--":text,el=node("span",value,"badge "+cls),key=tagIdentities.get(value)||value||"--";
  const success=["完成","已完成","通過","已設定","已啟用","就緒","有紀錄","正常","最近有回應"],failure=["錯誤","失敗","逾時","連線錯誤","無法存取","設定無效","讀取失敗"],pending=["等待中","排隊中","回補中","部分完成","等待讀取","未找到","尚未設定","格式未支援","超過讀取上限","部分無法存取"],inactive=["--","已停用","已暫停","已關閉","未提供結果"];
  let hash=0;for(const char of String(key))hash=(Math.imul(hash,31)+char.codePointAt(0))>>>0;
  el.dataset.tagTone=success.includes(key)?"success":failure.includes(key)?"error":pending.includes(key)?"warning":inactive.includes(key)?"neutral":"category";
  el.style.setProperty("--badge-hue",String(({success:155,error:0,warning:38})[el.dataset.tagTone]??[210,275,165,35,325,135][hash%6]));return el;
}
function clickableRow(row,action,label){row.classList.add("clickable-row");row.title=label||ui("開啟明細");row.tabIndex=0;row.addEventListener("click",event=>{if(!event.target.closest("button,a,input,select"))action();});row.addEventListener("keydown",event=>{if(event.target===row&&["Enter"," "].includes(event.key)){event.preventDefault();action();}});return row;}
const notice=node("div",null,"notification"),noticeText=node("span"),noticeClose=button("×",()=>hideNotice(),"notice-close");notice.id="setting-notification";notice.setAttribute("popover","manual");notice.setAttribute("role","status");notice.setAttribute("aria-live","polite");notice.setAttribute("aria-atomic","true");noticeClose.setAttribute("aria-label",ui("關閉通知"));notice.append(noticeText,noticeClose);notice.hidden=true;document.body.append(notice);let noticeTimer,noticeOpen=false;
function hideNotice(){clearTimeout(noticeTimer);if(noticeOpen){notice.hidePopover();noticeOpen=false;}notice.hidden=true;}
function feedback(id,text,state="success"){const field=$(id);if(field){field.textContent="";field.dataset.state=state;}if(!text){hideNotice();return;}clearTimeout(noticeTimer);noticeText.textContent=text;notice.dataset.state=state;notice.hidden=false;if(typeof notice.showPopover==="function"){if(!noticeOpen){notice.showPopover();noticeOpen=true;}}else (document.querySelector("dialog[open]")||document.body).append(notice);noticeTimer=setTimeout(hideNotice,state==="error"?8000:state==="pending"?12000:3600);}
function roundNumber(input){if(input.type!=="number"||input.step==="any"||!input.value.trim())return false;const value=Number(input.value);if(!Number.isFinite(value))return false;const rounded=Math.max(input.min===""?-Infinity:Number(input.min),Math.min(Math.round(value),input.max===""?Infinity:Number(input.max)));input.value=String(rounded);return true;}
document.addEventListener("change",event=>{if(event.target instanceof HTMLInputElement)roundNumber(event.target);},true);
document.addEventListener("focusout",event=>{const input=event.target;if(!(input instanceof HTMLInputElement)||input.type!=="number")return;const before=input.value;if(roundNumber(input)&&input.value!==before)input.dispatchEvent(new Event("change",{bubbles:true}));});
function pageInput(input,change){
  const apply=()=>{const value=Number(input.value);input.value=Math.max(1,Math.min(Number.isFinite(value)?Math.round(value):1,Number(input.max)||1));change();};
  input.addEventListener("change",apply);input.addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();apply();}});input.title=ui("輸入頁碼後按 Enter 套用");
}
function syncPageInput(input,value,max){input.max=max;if(document.activeElement!==input)input.value=value;}
let overviewFrame=0;
function arrangeOverview(){
  if(overviewFrame)return;overviewFrame=requestAnimationFrame(()=>{overviewFrame=0;const grid=document.querySelector(".overview-panels"),panels=[...grid.children].filter(panel=>!panel.hidden&&!panel.classList.contains("layout-hidden")),heights=panels.map(panel=>panel.getBoundingClientRect().height);if(!heights.some(height=>height>0))return;grid.classList.add("masonry");const style=getComputedStyle(grid),gap=parseFloat(style.rowGap),step=parseFloat(style.gridAutoRows)+gap;for(let i=0;i<panels.length;i++)if(heights[i]>0)panels[i].style.gridRowEnd="span "+Math.ceil((heights[i]+gap)/step);});
}
let contentPanelFrame=0;
function arrangeContentPanels(){
  if(contentPanelFrame)return;contentPanelFrame=requestAnimationFrame(()=>{contentPanelFrame=0;
    for(const grid of document.querySelectorAll("main .panels:not(.overview-panels),#mcp-dashboard-panels")){
      if(grid.closest("dialog")||getComputedStyle(grid).display!=="grid")continue;
      const panels=[...grid.querySelectorAll(":scope>.panel,:scope>.panels>.panel")].filter(panel=>!panel.hidden&&!panel.classList.contains("layout-hidden")&&!panel.classList.contains("empty-statistics"));grid.style.setProperty("--panel-columns",Math.min(3,Math.max(1,panels.filter(panel=>panel.dataset.fullWidth!=="true").length)));for(const panel of panels){panel.style.removeProperty("grid-column");panel.style.removeProperty("grid-row");}const heights=panels.map(panel=>panel.getBoundingClientRect().height);if(!heights.some(height=>height>0))continue;
      grid.classList.add("masonry-panel-grid");const style=getComputedStyle(grid),columns=style.gridTemplateColumns.split(" ").length,gap=parseFloat(style.rowGap),step=parseFloat(style.gridAutoRows)+gap,rows=Array(columns).fill(1);
      for(let i=0;i<panels.length;i++){const panel=panels[i];if(!heights[i])continue;const span=Math.ceil((heights[i]+gap)/step);if(panel.dataset.fullWidth==="true"){const start=Math.max(...rows);panel.style.gridColumn="1 / -1";panel.style.gridRow=start+" / span "+span;rows.fill(start+span);}else{const column=rows.indexOf(Math.min(...rows));panel.style.gridColumn=String(column+1);panel.style.gridRow=rows[column]+" / span "+span;rows[column]+=span;}}
    }
  });
}
function cards(id,items){const root=typeof id==="string"?$(id):id;if(root.id&&!root.closest("dialog")){summaryLibrary.set(root.id,{root,items,sub:root.dataset.summaryLevel==="sub"||!!root.closest("#view-logs,#projects-content,#subagents-content,#dots-content")});const selected=preferences.summaries?.[root.id]||{},order=[...new Set((selected.order||[]).filter(index=>Number.isInteger(index)&&index>=0&&index<items.length).concat(items.map((_,index)=>index)))],count=selected.count??display[summaryLibrary.get(root.id).sub?"subSummary":"mainSummary"]??(summaryLibrary.get(root.id).sub?3:4);root.style.setProperty("--summary-columns",Math.max(1,Math.min(count,order.filter(index=>!selected.hidden?.includes(index)).length)));items=order.filter(index=>!selected.hidden?.includes(index)).slice(0,count).map(index=>[selected.titles?.[index]||items[index][0],...items[index].slice(1)]);}root.replaceChildren(...items.map(([label,value,info])=>{const el=node("div",null,"card"),amount=node("div",value,"value"),caption=node("div",label,"label"),help=fieldDescription(label),parts=typeof value==="string"?value.match(/^([\d,.]+)\s+(.+)$/):null;if(parts)amount.replaceChildren(document.createTextNode(parts[1]+" "),node("span",parts[2],"value-unit"));if(help){caption.title=help;caption.tabIndex=0;caption.append(node("span"," ⓘ","help-mark"));}el.append(caption,amount);if(info)el.append(node("div",info,"detail"));return el;}));if(root.id&&!root.closest("dialog"))renderChildSummaries(root);}

function renderChildSummaries(root){
  const targets={"codex-cards":["conversation-content"],"git-cards":["view-git"],"check-cards":["view-checks"],"error-cards":["error-content"],"mcp-cards":["mcp-source-content"]},source=summaryLibrary.get(root.id);if(!source)return;
  function summary(panelId,items){const panel=$(panelId);if(!panel)return;const id=panelId+"-summary";let group=$(id);if(!group){group=node("div",null,"cards sub-summary-cards");group.id=id;group.dataset.summaryLevel="sub";panel.prepend(group);}cards(group,items);}
  for(const id of targets[root.id]||[])summary(id,source.items);
  if(root.id==="codex-cards"&&data){const threads=data.codex.threads||[],project=threads.filter(thread=>thread.project_id||thread.project_name),agents=threads.filter(thread=>thread.execution?.parent_thread_id),known=items=>{const counts=items.map(thread=>thread.tool_calls).filter(Number.isFinite);return counts.length?counts.reduce((total,value)=>total+value,0):null;};
    summary("projects-content",[[ui("不同專案"),fmt(new Set(project.map(thread=>thread.project_id||thread.project_name)).size),""],[ui("有專案資訊的對話"),fmt(project.length),""],[ui("工具呼叫"),fmt(known(project)),""],[ui("最近活動"),when(project.map(thread=>thread.updated_at).filter(Boolean).sort().at(-1)),""]]);
    summary("subagents-content",[[ui("子代理程式"),fmt(agents.length),""],[ui("執行中"),fmt(agents.filter(thread=>thread.status==="running").length),""],[ui("工具呼叫"),fmt(known(agents)),""],[ui("已完成"),fmt(agents.filter(thread=>thread.status==="completed").length),""]]);
  }
}

function countBy(items,key){const counts=Object.create(null);for(const item of items){const value=item[key]||"unknown";counts[value]=(counts[value]||0)+1;}return counts;}
function sorted(counts){return Object.entries(counts||{}).sort((a,b)=>b[1]-a[1]||a[0].localeCompare(b[0]));}
function saveView(){try{preferences={...preferences,tab:document.querySelector('[data-tab][aria-selected="true"]').dataset.tab,inputs:Object.fromEntries(viewInputs.map(id=>[id,$(id).value])),page,appearance,display,charts:Object.fromEntries([...chartViews].map(([id,view])=>[id,view.settings])),settings:data?.settings||preferences.settings};localStorage.setItem(preferenceKey,JSON.stringify(preferences));}catch{}}
function toolCounts(threads,key="tools"){const counts=Object.create(null);for(const t of threads)for(const [name,count]of Object.entries(t[key]||{}))counts[name]=(counts[name]||0)+count;return counts;}
function title(t){return t.thread_name||ui("未命名對話");}
function svgNode(tag,attrs,text){const el=document.createElementNS("http://www.w3.org/2000/svg",tag);for(const [key,value]of Object.entries(attrs||{}))el.setAttribute(key,value);if(text!=null)el.textContent=text;return el;}
const chartTip=node("div",null,"chart-tooltip");chartTip.hidden=true;chartTip.id="chart-tooltip";chartTip.setAttribute("role","tooltip");document.body.append(chartTip);
const helpCatalog=new Set(Object.values(fieldDescriptions)),helpTip=node("div",null,"chart-tooltip help-tooltip");helpTip.id="interface-tooltip";helpTip.setAttribute("role","tooltip");helpTip.hidden=true;document.body.append(helpTip);let helpTarget=null;
let tooltipSuppressed=new WeakSet();
function hideHelp(){helpTip.hidden=true;helpTarget=null;}
function placeTooltip(tip,x,y,target){const rect=target?.closest("dialog")?.getBoundingClientRect(),left=rect?rect.left+8:8,right=rect?rect.right-8:innerWidth-8,top=rect?rect.top+8:8,bottom=rect?rect.bottom-8:innerHeight-8;tip.style.maxWidth=Math.min(420,right-left)+"px";tip.style.left=left+"px";tip.style.top=top+"px";tip.style.left=Math.max(left,Math.min(x+14+tip.offsetWidth>right?x-tip.offsetWidth-14:x+14,right-tip.offsetWidth))+"px";tip.style.top=Math.max(top,Math.min(y+14+tip.offsetHeight>bottom?y-tip.offsetHeight-14:y+14,bottom-tip.offsetHeight))+"px";}
function dragAllowed(handle){return !handle.closest("dialog")&&dragEnabled;}

function openDialog(dialog){if(document.activeElement instanceof HTMLElement)tooltipSuppressed.add(document.activeElement);hideHelp();hideChartTip();dialog.showModal();syncHelp();}
function bindHelp(el,text){
  const base=[...copyCatalog].find(key=>key===text||ui(key)===text)||text;if(copyCatalog.has(base)&&editableCopy(base)&&base.length>30)helpCatalog.add(base);el.dataset.help=text;if(copyCatalog.has(base))el.dataset.helpKey=base;else delete el.dataset.helpKey;el.removeAttribute("title");el.setAttribute("aria-describedby",helpTip.id);if(el.dataset.helpBound)return;el.dataset.helpBound="true";
  function show(event){if(tooltipSuppressed.has(el)||el.classList.contains("help-dismissed")||el.closest("dialog")&&el.matches("button,input,select")&&!el.matches('[role="tab"]'))return;const text=[el.dataset.help,dragAllowed(el)&&el.dataset.dragHelp?ui(el.dataset.dragHelp):""].filter(Boolean).join(". ");if(!text){if(helpTarget===el)hideHelp();return;}hideChartTip();helpTarget=el;helpTip.textContent=text;(el.closest("dialog")||document.body).append(helpTip);helpTip.hidden=false;const rect=el.getBoundingClientRect(),x=event.type==="focusin"?rect.left:event.clientX,y=event.type==="focusin"?rect.bottom:event.clientY;placeTooltip(helpTip,x,y,el);}
  el.addEventListener("pointermove",show);el.addEventListener("focusin",event=>requestAnimationFrame(()=>{if(document.activeElement===el&&!el.closest("[hidden]"))show(event);}));for(const event of ["pointerleave","focusout"])el.addEventListener(event,()=>{if(helpTarget===el)hideHelp();});
  for(const event of ["pointerleave","focusout"])el.addEventListener(event,()=>el.classList.remove("help-dismissed"));
}
function bindDragHelp(el,key){el.dataset.dragHelp=key;bindHelp(el,el.dataset.helpKey?ui(el.dataset.helpKey):el.dataset.help||"");}
function syncHelp(){for(const el of document.querySelectorAll("[title]"))bindHelp(el,el.getAttribute("title"));for(const title of document.querySelectorAll("svg title"))title.remove();if(helpTarget&&!helpTarget.isConnected)hideHelp();}
document.addEventListener("keydown",event=>{if(event.key==="Escape"&&!helpTip.hidden){event.preventDefault();event.stopPropagation();helpTarget?.classList.add("help-dismissed");hideHelp();}},true);addEventListener("scroll",hideHelp,true);addEventListener("blur",hideHelp);
document.addEventListener("pointerdown",()=>{hideHelp();hideChartTip();},true);
document.addEventListener("pointermove",event=>{const el=event.target.closest?.("[data-help]");if(el)tooltipSuppressed.delete(el);},true);document.addEventListener("keydown",event=>{if(event.key==="Tab")tooltipSuppressed=new WeakSet();},true);for(const dialog of document.querySelectorAll("dialog"))dialog.addEventListener("close",()=>{if(document.activeElement instanceof HTMLElement)tooltipSuppressed.add(document.activeElement);hideHelp();hideChartTip();});
function hideChartTip(){chartTip.hidden=true;}
function showChartTip(heading,value,event,target){
  hideHelp();
  (target.closest("dialog")||document.body).append(chartTip);
  chartTip.replaceChildren(node("div",heading),...(Array.isArray(value)?value:[node("strong",value)]));chartTip.hidden=false;
  const rect=target.getBoundingClientRect(),x=event?.clientX??rect.left+rect.width/2,y=event?.clientY??rect.top;
  placeTooltip(chartTip,x,y,target);
}
addEventListener("scroll",hideChartTip,true);addEventListener("blur",hideChartTip);
function bounds(id,items=[]){const s=chartViews.get(id)?.settings||{},end=s.range==="custom"?new Date(s.end).getTime():Date.now(),start=s.range==="all"?Math.min(...items.map(e=>new Date(e.timestamp||e.time||e.updated_at).getTime()).filter(Number.isFinite),end-86400000):s.range==="custom"?new Date(s.start).getTime():end-s.length*s.unit;return {start,end};}
function ranged(id,items){const {start,end}=bounds(id,items);return items.filter(item=>{const time=new Date(item.timestamp||item.time||item.updated_at).getTime();return Number.isFinite(time)&&time>=start&&time<=end;});}
function niceMax(value){if(value<=4)return Math.max(4,Math.ceil(value));const power=10**Math.floor(Math.log10(value/4)),step=Math.ceil(value/4/power)*power;return step*4;}
function statistics(values){const samples=values.filter(Number.isFinite).sort((a,b)=>a-b),size=samples.length;if(!size)return null;const percentile=ratio=>{const position=(size-1)*ratio,lower=Math.floor(position);return samples[lower]+(samples[Math.ceil(position)]-samples[lower])*(position-lower);};const total=samples.reduce((sum,value)=>sum+value,0);return {size,total,mean:total/size,p99:percentile(.99),p1:percentile(.01)};}
function measure(value,unit){if(unit==="bytes"){const scale=value>=1024**3?1024**3:value>=1024**2?1024**2:value>=1024?1024:1;return fmt(Math.round(value/scale*100)/100)+" "+({1:"B",1024:"KiB",1048576:"MiB",1073741824:"GiB"}[scale]);}return fmt(Math.round(value*100)/100)+" "+unit;}
function statisticLabels(stats,unit,basis){
  const text=node("span",null,"chart-statistics"),count=unit==="bytes"?["樣本數",fmt(stats?.size)]:["總數",Number.isFinite(stats?.total)?measure(stats.total,unit):"--"];
  for(const [label,value]of [["平均值",Number.isFinite(stats?.mean)?measure(stats.mean,unit):"--"],["P1 / P99",stats?measure(stats.p1,unit)+" / "+measure(stats.p99,unit):"--"],count,["統計範圍",basis?basis+" · "+fmt(stats?.size??0)+ui(" 筆樣本"):"--"]]){const item=node("span",null,"statistic-item");item.append(node("span",ui(label),"statistic-label"),node("b",value,"statistic-value"));const help=fieldDescription(label);if(help){bindHelp(item,help);item.tabIndex=0;}text.append(item);}return text;
}
function bars(id,counts,names,onClick,unit=ui("次"),segments){
  const view=chartViews.get(id);if(view)view.renderData={kind:"bars",args:[counts,names,onClick,unit,segments]};if(typeof counts==="function")counts=counts(id);
  const s=chartViews.get(id)?.settings||{},entries=sorted(counts).slice(0,rankingSize(s.top??display.ranking)),peak=Math.max(...entries.map(item=>item[1]),1),max=s.maximum||peak;
  if(["column","stacked"].includes(s.shape)){columnChart(id,entries,names,onClick,unit,s,segments);return;}
  if(["donut","pie"].includes(s.shape)){shareChart(id,counts,entries,names,onClick,unit,s.shape);return;}
  $(id).replaceChildren(...entries.map(([key,value])=>{
    const row=node("div",null,"bar-row"),label=names?.[key]||key;
    row.append(onClick?button(label,()=>onClick(key),"bar-label link"):node("span",label,"bar-label"),node("b",fmt(value)));
    const svg=svgNode("svg",{viewBox:"0 0 640 10",preserveAspectRatio:"none",role:"img","aria-label":label+" "+fmt(value)+" "+unit});
    svg.append(svgNode("rect",{width:640,height:10,rx:5,class:"bar-track"}),svgNode("rect",{width:640*Math.min(value,max)/max,height:10,rx:5,class:"bar-fill"}));
    const text=fmt(value)+" "+unit;row.tabIndex=onClick?-1:0;row.setAttribute("aria-label",label+": "+text);
    for(const event of ["pointerenter","pointermove","focusin"])row.addEventListener(event,e=>showChartTip(rankTitle(id,key,label),text,e.type==="focusin"?null:e,row));
    for(const event of ["pointerleave","focusout"])row.addEventListener(event,hideChartTip);
    row.append(svg);return row;
  }));
  $(id).append(node("p",unit+(peak>max?ui(" · 超過上限的長條已截短"):""),"chart-unit"));
  if(s.statistics!==0&&entries.length)$(id).append(statisticLabels(statistics(entries.map(item=>item[1])),unit,ui("目前顯示項目")));
  if(!entries.length)$(id).append(node("p","--","empty"));
}
function columnChart(id,entries,names,onClick,unit,settings,series){
  const root=$(id);root.replaceChildren();if(!entries.length){root.append(node("p","--","empty"));return;}
  const font=Math.max(11,parseFloat(getComputedStyle(root).fontSize)*.857),textSize=text=>[...text].reduce((sum,char)=>sum+(/[\u2e80-\u9fff]/.test(char)?font:font*.58),0);
  const max=settings.maximum||niceMax(Math.max(...entries.map(item=>item[1]),1)),width=root.clientWidth||640,compact=new Intl.NumberFormat(locale,{notation:"compact",maximumFractionDigits:1}),axisValue=value=>textSize(fmt(max))>width*.27?compact.format(value):fmt(value),left=Math.max(40,textSize(axisValue(max))+18),height=350,plotWidth=Math.max(1,width-left-16),slot=plotWidth/entries.length,wrap=node("div",null,"column-chart-scroll"),svg=svgNode("svg",{viewBox:"0 0 "+width+" "+height,role:"img","aria-label":ui(settings.shape==="stacked"?"堆疊直條圖":"直條圖"),class:"column-chart"});
  svg.style.setProperty("--column-font-size",font+"px");const slant=entries.some(([key])=>textSize(names?.[key]||key)>slot-8),labelStep=Math.max(1,Math.ceil((slant?font*1.6:font*2.5)/slot)),categories=[...new Set(entries.flatMap(([key,value])=>(series?.[key]||[["未分類",value]]).map(([name])=>name)))],colors=new Map(categories.map((name,index)=>[name,name==="未分類"?7:index%7]));
  for(let index=0;index<=4;index++){const y=240-index*47;svg.append(svgNode("line",{x1:left,x2:width-16,y1:y,y2:y,class:"grid-line"}),svgNode("text",{x:left-10,y:y+4,"text-anchor":"end"},axisValue(max*index/4)));}
  svg.append(svgNode("text",{x:left,y:18},unit),svgNode("line",{x1:left,x2:left,y1:52,y2:240,class:"axis-line"}),svgNode("line",{x1:left,x2:width-20,y1:240,y2:240,class:"axis-line"}));
  let labelBottom=270+font;
  for(const [index,[key,value]]of entries.entries()){const label=names?.[key]||key,x=left+index*slot+slot*.2,segments=settings.shape==="stacked"?series?.[key]||[["未分類",value]]:[[label,value]];let offset=0;
    for(const [part,[name,amount]]of segments.entries()){if(!Number.isFinite(amount))continue;const shown=Math.min(amount,Math.max(0,max-offset)),rect=svgNode("rect",{x,y:240-(offset+shown)/max*188,width:slot*.6,height:shown/max*188,rx:2,class:"column-segment palette-"+(settings.shape==="stacked"?colors.get(name):index)%8});const text=(settings.shape==="stacked"?ui(name)+": ":"")+fmt(amount)+" "+unit;rect.tabIndex=0;rect.setAttribute("role",onClick?"button":"img");rect.setAttribute("aria-label",label+": "+text);for(const event of ["pointerenter","pointermove","focusin"])rect.addEventListener(event,e=>showChartTip(rankTitle(id,key,label),text,e.type==="focusin"?null:e,rect));for(const event of ["pointerleave","focusout"])rect.addEventListener(event,hideChartTip);if(onClick){rect.addEventListener("click",()=>onClick(key));rect.addEventListener("keydown",event=>{if(["Enter"," "].includes(event.key)){event.preventDefault();onClick(key);}});}svg.append(rect);offset+=amount;}
    const size=textSize,center=x+slot*.3,limit=slant?Math.min(100,(center-8)/.82):slot-8;let caption=label;while(caption.length>1&&size(caption+(caption===label?"":"…"))>limit)caption=caption.slice(0,-1);if(caption!==label)caption+="…";const tick=svgNode("text",{x:center,y:slant?260:270,"text-anchor":slant?"end":"middle",...(slant?{transform:"rotate(-35 "+center+" 260)"}:{}),class:"column-label"},caption);tick.tabIndex=0;tick.setAttribute("aria-label",label);for(const event of ["pointerenter","pointermove","focusin"])tick.addEventListener(event,e=>showChartTip(rankTitle(id,key,label),fmt(value)+" "+unit,e.type==="focusin"?null:e,tick));for(const event of ["pointerleave","focusout"])tick.addEventListener(event,hideChartTip);svg.append(svgNode("line",{x1:center,x2:center,y1:240,y2:245,class:"axis-line"}));if(index%labelStep===0){svg.append(tick);const valueText=textSize(fmt(value))<=slot-4?fmt(value):compact.format(value);if(textSize(valueText)<=slot*labelStep-4)svg.append(svgNode("text",{x:center,y:240-Math.min(value,max)/max*188-8,"text-anchor":"middle",class:"column-value"},valueText));}
  }
  for(const tick of svg.querySelectorAll(".column-label"))if(slant)labelBottom=Math.max(labelBottom,260+textSize(tick.textContent)*Math.sin(35*Math.PI/180)+font);
  const fittedHeight=Math.ceil(labelBottom+12);svg.setAttribute("viewBox","0 0 "+width+" "+fittedHeight);svg.style.height=fittedHeight+"px";
  wrap.append(svg);root.append(wrap);if(settings.shape==="stacked"){const legend=node("div",null,"column-legend");for(const label of categories){const item=node("span");item.append(node("span",null,"pie-swatch palette-"+colors.get(label)),node("span",ui(label)));if(label==="未分類")bindHelp(item,ui("來源未提供完整分類, 或分類合計與總量不一致, 此項顯示已回報的總量"));legend.append(item);}root.append(legend);}
  if(settings.statistics!==0)root.append(statisticLabels(statistics(entries.map(item=>item[1])),unit,ui("目前顯示項目")));
}

function shareChart(id,counts,visible,names,onClick,unit,shape){
  const entries=visible.map(([key,value])=>({key,value,label:names?.[key]||key})),total=Object.values(counts).reduce((sum,value)=>sum+value,0),remaining=total-entries.reduce((sum,item)=>sum+item.value,0);
  if(remaining>0)entries.push({key:null,value:remaining,label:ui("其他項目")});
  const root=$(id);root.replaceChildren();if(!total){root.append(node("p","--","empty"));return;}
  const layout=node("div",null,"pie-layout"),svg=svgNode("svg",{viewBox:"0 0 240 240",role:"img","aria-label":ui("分布圖"),class:"pie-figure"}),legend=node("div",null,"pie-legend");let angle=-Math.PI/2;
  for(const [index,item]of entries.entries()){
    const share=item.value/total,end=angle+share*Math.PI*2,x1=120+96*Math.cos(angle),y1=120+96*Math.sin(angle),x2=120+96*Math.cos(end),y2=120+96*Math.sin(end),color="palette-"+index%8;
    const segment=share===1?svgNode("circle",{cx:120,cy:120,r:96,class:"pie-segment "+color}):svgNode("path",{d:`M 120 120 L ${x1} ${y1} A 96 96 0 ${share>.5?1:0} 1 ${x2} ${y2} Z`,class:"pie-segment "+color});
    const info=fmt(item.value)+" "+unit+" · "+fmt(Math.round(share*1000)/10)+"%";segment.tabIndex=0;segment.setAttribute("aria-label",item.label+": "+info);segment.setAttribute("role",onClick&&item.key?"button":"img");
    for(const event of ["pointerenter","pointermove","focusin"])segment.addEventListener(event,e=>showChartTip(rankTitle(id,item.key,item.label),info,e.type==="focusin"?null:e,segment));for(const event of ["pointerleave","focusout"])segment.addEventListener(event,hideChartTip);
    if(onClick&&item.key){segment.addEventListener("click",()=>onClick(item.key));segment.addEventListener("keydown",event=>{if(["Enter"," "].includes(event.key)){event.preventDefault();onClick(item.key);}});}
    svg.append(segment);angle=end;const row=node("div",null,"pie-legend-row"),swatch=node("span",null,"pie-swatch "+color);row.append(swatch,onClick&&item.key?button(item.label,()=>onClick(item.key),"link"):node("span",item.label),node("b",info));legend.append(row);
  }
  if(shape==="donut"){svg.append(svgNode("circle",{cx:120,cy:120,r:58,class:"pie-hole","pointer-events":"none"}),svgNode("text",{x:120,y:125,"text-anchor":"middle",class:"pie-unit","pointer-events":"none"},unit));}
  layout.append(svg,legend);root.append(layout,node("p",ui("合計 ")+fmt(total)+" "+unit,"chart-unit"));if(chartViews.get(id)?.settings.statistics!==0)root.append(statisticLabels(statistics(visible.map(item=>item[1])),unit,ui("目前顯示項目")));
}
function timeline(id,noteId,series,unit=ui("次"),average=false,lanes=[{key:"calls"}]){
  const recipe=chartViews.get(id);if(recipe&&!recipe.projecting)recipe.renderData={kind:"timeline",args:[series,unit,average,lanes]};
  const svg=$(id),view=chartViews.get(id);view?.statisticsRoot?.replaceChildren();view?.statisticsPanel?.classList.add("empty-statistics");svg.replaceChildren();svg.onpointermove=svg.onpointerleave=svg.onkeydown=svg.onfocus=svg.onblur=null;
  if(!series?.length){$(noteId).textContent="--";return;}
  const s=chartViews.get(id)?.settings||{};lanes=lanes.slice(0,s.lines??display.lines??displayDefaults.lines);const range=bounds(id,series),end=range.end;
  let interval=Math.max(60000,s.interval||3600000);while((end-range.start)/interval>240)interval*=2;
  const start=Math.floor(range.start/interval)*interval,slots=Math.max(1,Math.ceil((end-start)/interval)),values=Array.from({length:slots},(_,i)=>({time:start+interval*i,totals:lanes.map(()=>0),samples:lanes.map(()=>0)})),samples=lanes.map(()=>[]);
  for(const item of series){const time=new Date(item.time).getTime(),index=Math.min(slots-1,Math.floor((time-start)/interval));if(!Number.isFinite(time)||time<range.start||time>end||index<0)continue;for(const [lane,{key}]of lanes.entries())if(Number.isFinite(item[key])){values[index].totals[lane]+=item[key];values[index].samples[lane]++;samples[lane].push(item[key]);}}
  for(const item of values)item.calls=item.totals.map((value,lane)=>average?item.samples[lane]?Math.round(value/item.samples[lane]*100)/100:null:value);
  const stats=lanes.map((_,lane)=>statistics(average?samples[lane]:values.map(item=>item.calls[lane]))),multi=lanes.length>1;
  const peak=Math.max(...values.flatMap(item=>item.calls).filter(Number.isFinite),s.statistics===0?0:Math.max(...stats.map(stat=>stat?.p99||0)),1),max=s.maximum||niceMax(peak),scale=740/(svg.clientWidth||740),labelSize=Math.max(11,parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--font-size"))*.75||11),axisValue=value=>unit==="bytes"?measure(value,unit):fmt(value),left=Math.min(300,Math.max(58,(axisValue(max).length*labelSize*.66+12)*scale)),plotWidth=710-left,width=plotWidth/slots;
  svg.style.setProperty("--chart-label-size",labelSize*scale+"px");
  for(let i=0;i<=4;i++){const y=190-i*38;svg.append(svgNode("line",{x1:left,x2:710,y1:y,y2:y,class:"grid-line"}),svgNode("text",{x:left-10,y:y+4,"text-anchor":"end"},axisValue(max*i/4)));}
  svg.append(svgNode("text",{x:left,y:20},unit),svgNode("line",{x1:left,x2:left,y1:38,y2:190,class:"axis-line"}),svgNode("line",{x1:left,x2:710,y1:190,y2:190,class:"axis-line"}));
  for(const [lane]of lanes.entries()){
    const points=[];values.forEach((item,i)=>{const value=item.calls[lane];if(value===null){points.push(null);return;}const height=Math.min(value,max)/max*152,x=left+i*width;points.push((x+width/2)+","+(190-height));if(s.shape!=="line"){const barWidth=width/lanes.length;svg.append(svgNode("rect",{x:x+lane*barWidth,y:190-height,width:Math.max(.5,barWidth-1),height,rx:2,...multi?{class:"column-segment palette-"+lane%8}:{}}));}});
    if(s.shape==="line"){let segment=[];const draw=()=>{const color=multi?" palette-"+lane%8:"";if(segment.length>1)svg.append(svgNode("polyline",{points:segment.join(" "),class:"trend-line"+color,fill:"none",...multi&&lane%3?{"stroke-dasharray":lane%3===1?"7 3":"2 3"}:{}}));else if(segment.length){const [cx,cy]=segment[0].split(",");svg.append(svgNode("circle",{cx,cy,r:3,class:"trend-dot"+color}));}segment=[];};for(const point of points)if(point===null)draw();else segment.push(point);draw();}
  }
  if(!multi&&stats[0]&&s.statistics!==0)for(const [label,value,cls]of [["平均值",stats[0].mean,"mean-line"],["P99",stats[0].p99,"p99-line"],["Low 1% (P1)",stats[0].p1,"p1-line"]]){const y=190-Math.min(value,max)/max*152;svg.append(svgNode("line",{x1:left,x2:710,y1:y,y2:y,class:"statistic-line "+cls}));}
  const cursor=svgNode("g",{class:"chart-cursor",visibility:"hidden","pointer-events":"none"}),guide=svgNode("line",{y1:38,y2:190}),dots=lanes.map((_,lane)=>svgNode("circle",{r:5,...multi?{class:"trend-dot palette-"+lane%8}:{}}));cursor.append(guide,...dots);svg.append(cursor);svg.tabIndex=0;
  let selected=0;
  function showPoint(index,event){
    selected=Math.max(0,Math.min(index,values.length-1));const item=values[selected],x=left+(selected+.5)*width;
    guide.setAttribute("x1",x);guide.setAttribute("x2",x);for(const [lane,dot]of dots.entries()){const value=item.calls[lane];dot.setAttribute("cx",x);dot.setAttribute("cy",190-Math.min(value,max)/max*152);dot.setAttribute("visibility",value===null?"hidden":"visible");}cursor.setAttribute("visibility","visible");
    const heading=when(new Date(Math.max(item.time,range.start)).toISOString())+" - "+when(new Date(Math.min(item.time+interval,end)).toISOString()),readings=lanes.map(({label},lane)=>(label?ui(label)+": ":"")+(item.calls[lane]===null?"--":measure(item.calls[lane],unit))),text=readings.join(" · ");showChartTip(heading,multi?readings.map((value,lane)=>node("strong",value,"chart-tooltip-row palette-"+lane%8)):text,event,svg);
    svg.setAttribute("aria-label",ui(chartViews.get(id)?.title||"活動趨勢")+" · "+heading+": "+text);
  }
  svg.onpointermove=event=>{const matrix=svg.getScreenCTM();if(!matrix)return;const point=svg.createSVGPoint();point.x=event.clientX;point.y=event.clientY;const local=point.matrixTransform(matrix.inverse());if(local.x<left||local.x>710||local.y<38||local.y>190){cursor.setAttribute("visibility","hidden");hideChartTip();return;}showPoint(Math.floor((local.x-left)/width),event);};
  svg.onfocus=()=>showPoint(selected);svg.onkeydown=event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();showPoint(event.key==="Home"?0:event.key==="End"?values.length-1:selected+(event.key==="ArrowRight"?1:-1));};
  svg.onpointerleave=svg.onblur=()=>{cursor.setAttribute("visibility","hidden");hideChartTip();};
  const ticks=Math.max(1,Math.min(4,Math.floor(plotWidth/scale/(labelSize*10))));for(let i=0;i<=ticks;i++){const time=start+(end-start)*i/ticks,x=left+plotWidth*i/ticks,label=new Date(time).toLocaleString(locale,{month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit",hour12:false});svg.append(svgNode("line",{x1:x,x2:x,y1:190,y2:196}),svgNode("text",{x,y:218,"text-anchor":i===0?"start":i===ticks?"end":"middle"},label));}
  const summary=node("span",ui("每格 ")+fmt(interval/60000)+ui(" 分鐘 · 最高 ")+measure(Math.max(...values.flatMap(v=>v.calls).filter(Number.isFinite),0),unit)+(average?ui(" · 每格平均"):"")+(peak>max?ui(" · 超過上限的數值已截短"):""),"chart-note-summary");bindHelp(summary,summary.textContent);$(noteId).replaceChildren(summary);
  if(lanes.some(lane=>lane.label)){const legend=node("span",null,"trend-legend");for(const [lane,{label,key,title}]of lanes.entries()){const item=node("span");if(title)bindHelp(item,title);item.append(node("span",null,"pie-swatch palette-"+lane%8),node("span",ui(label||key)));legend.append(item);}$(noteId).prepend(legend);}
  if(s.statistics!==0)for(const [lane,stat]of stats.entries())if(stat){const group=node("span",null,"trend-statistic-group");if(multi)group.append(node("b",ui(lanes[lane].label||lanes[lane].key)));group.append(statisticLabels(stat,unit,ui(average?"區間內更新樣本":"區間內時間格, 含零活動")));if(view?.statisticsRoot)view.statisticsRoot.append(group);}
  if(view?.statisticsPanel)view.statisticsPanel.classList.toggle("empty-statistics",!view.statisticsRoot.childElementCount);
}
function categoryTimeline(id,noteId,events,key,names=null,weight=null){
  const view=chartViews.get(id);if(view)view.renderData={kind:"category",args:[events,key,names,weight]};
  const selected=ranged(id,events),totals=new Map();for(const event of selected){const category=event[key]||ui("未知"),count=weight?event[weight]:1;if(Number.isFinite(count)&&count>=0)totals.set(category,(totals.get(category)||0)+count);}
  const settings=chartViews.get(id)?.settings||{},categories=[...totals].sort(([a,x],[b,y])=>y-x||sortCollator.compare(a,b)).slice(0,settings.lines??display.lines??displayDefaults.lines),lanes=categories.map(([category],index)=>({key:"series"+index,label:names?.[category]||category,title:category})),indexes=new Map(categories.map(([category],index)=>[category,index]));
  const samples=selected.map(event=>{const index=indexes.get(event[key]||ui("未知")),sample={time:event.timestamp||event.time||event.updated_at};if(index!=null)sample["series"+index]=weight?event[weight]:1;return sample;});if(view)view.projecting=true;try{timeline(id,noteId,lanes.length?samples:[],ui("次"),false,lanes);}finally{if(view)view.projecting=false;}
}
function hourly(events){return events.filter(event=>event.timestamp).map(event=>({time:event.timestamp,calls:1}));}
function activeThreadsTimeline(id,noteId,events){
  const view=chartViews.get(id);if(view)view.renderData={kind:"threads",args:[events]};
  const {start,end}=bounds(id,events),s=chartViews.get(id)?.settings||{};let interval=Math.max(60000,s.interval||3600000);while((end-start)/interval>240)interval*=2;
  const first=Math.floor(start/interval)*interval,slots=Math.max(1,Math.ceil((end-first)/interval)),groups=new Map();
  for(const event of ranged(id,events)){if(!event.thread_id)continue;const time=new Date(event.timestamp||event.time).getTime(),slot=Math.min(slots-1,Math.floor((time-first)/interval));if(!Number.isFinite(time)||slot<0)continue;if(!groups.has(slot))groups.set(slot,new Set());groups.get(slot).add(event.thread_id);}
  if(view)view.projecting=true;try{timeline(id,noteId,[...groups].map(([slot,threads])=>({time:new Date(Math.max(start,first+slot*interval)).toISOString(),calls:threads.size})),ui("對話數"));}finally{if(view)view.projecting=false;}
}
const overviewShapes={"monitor-read-chart":["line","折線圖"],"sqlite-retained-chart":["line","折線圖"],"web-retained-chart":["line","折線圖"],"mcp-retained-chart":["line","折線圖"],"skill-time-chart":["line","折線圖"],"tool-time-chart":["line","折線圖"],"check-time-chart":["line","折線圖"],"overview-account-card":["bar","橫條圖"],"overview-account-quota":["bar","橫條圖"],"activity-chart":["line","折線圖"],"source-chart":["bar","橫條圖"],"model-chart":["column","直條圖"],"environment-chart":["bar","橫條圖"],"trigger-chart":["bar","橫條圖"],"overview-tools-chart":["bar","橫條圖"],"overview-token-chart":["bar","橫條圖"],"overview-error-chart":["bar","橫條圖"],"overview-file-chart":["column","直條圖"],"overview-sql-chart":["column","直條圖"],"overview-cpu-chart":["line","折線圖"],"overview-source-column":["column","直條圖"],"overview-model-ring":["donut","環圈圖"],"overview-model-pie":["pie","圓餅圖"],"overview-token-stack":["stacked","堆疊直條圖"],"overview-web-chart":["line","折線圖"],"overview-error-time-chart":["line","折線圖"],"overview-refresh-chart":["line","折線圖"],"overview-read-chart":["line","折線圖"],"overview-git-chart":["bar","橫條圖"],"overview-skill-chart":["bar","橫條圖"],"overview-check-chart":["bar","橫條圖"],"overview-quota-chart":["bar","橫條圖"],"overview-dot-chart":["bar","橫條圖"]};
function rankingSize(value){return value===0||value==="all"?undefined:Number.isInteger(value)&&value>0?value:displayDefaults.ranking;}
const cardLibrary=new Map(),overviewCopies=new Map();
function chartControls(id,trend=false,series=false,baseId=id){
  const controls=node("div",null,"chart-options"),form=node("div",null,"chart-controls");const saved=preferences.charts?.[id]||(id.endsWith("-retained-chart")?preferences.charts?.["monitor-retained-chart"]:null)||{},sampled=baseId.startsWith("monitor-")||baseId.endsWith("-retained-chart");
  const localDate=time=>new Date(time-new Date(time).getTimezoneOffset()*60000).toISOString().slice(0,16);
  const settings={range:trend?"recent":"all",length:sampled?60:24,unit:sampled?60000:3600000,interval:sampled?60000:3600000,top:display.ranking==="all"?0:display.ranking,lines:display.lines??displayDefaults.lines,maximum:0,statistics:trend?1:0,shape:trend?"line":"bar",start:localDate(Date.now()-86400000),end:localDate(Date.now())};
  const fields={};
  function copyNode(tag,text){const el=node(tag,ui(text));if(editableCopy(text))copyTargets.push({text:el.firstChild,base:text,before:"",after:""});return el;}
  function select(key,label,items){const el=node("select");for(const [value,text]of items){const option=copyNode("option",text);option.value=value;el.append(option);}fields[key]=el;const wrap=copyNode("label",label);wrap.append(el);form.append(wrap);}
  function input(key,label,type,min,max){const el=node("input");el.type=type;if(min!=null)el.min=min;if(max!=null)el.max=max;fields[key]=el;const wrap=copyNode("label",label);wrap.append(el);form.append(wrap);}
  select("range","時間範圍",[["all","全部紀錄"],["recent","最近一段時間"],["custom","指定起訖"]]);
  input("length","最近","number",1,365);select("unit","時間單位",[[60000,"分鐘"],[3600000,"小時"],[86400000,"天"]]);
  input("start","開始時間","datetime-local");input("end","結束時間","datetime-local");
  if(trend)select("interval","時間間隔",[[60000,"1 分鐘"],[300000,"5 分鐘"],[900000,"15 分鐘"],[3600000,"1 小時"],[21600000,"6 小時"],[86400000,"1 天"]]);
  if(!trend){select("top","顯示項目",[]);fillDisplayOptions(fields.top,saved.top??settings.top,true);form.prepend(fields.top.parentElement);}if(series)select("lines","顯示線數",[[3,"3 條"],[5,"5 條"],[10,"10 條"]]);
  const types=baseId!==id?cardLibrary.get(baseId).types:trend?[["line","折線圖"],["bar","長條圖"]]:overviewShapes[id]?[overviewShapes[id]]:[["bar","橫條圖"],["column","直條圖"],...(["usage-model-chart","overview-token-chart"].includes(id)?[["stacked","堆疊直條圖"]]:[]),["donut","環圈圖"],["pie","圓餅圖"]],shape=saved.shape;
  if(overviewShapes[id])settings.shape=overviewShapes[id][0];else if(types.some(([key])=>key===shape))settings.shape=shape;
  input("maximum",trend?"Y 軸上限 (0 = 自動)":"數值上限 (0 = 自動)","number",0,1000000000);
  if(trend&&!id.endsWith("-retained-chart"))select("statistics","統計指標",[[1,"顯示平均值與百分位數"],[0,"隱藏統計指標"]]);
  for(const [key,el]of Object.entries(fields)){const value=saved[key]??settings[key];if(el.tagName==="SELECT"?[...el.options].some(o=>o.value===String(value)):el.type==="number"?Number.isFinite(Number(value))&&Number(value)>=Number(el.min)&&Number(value)<=Number(el.max):typeof value==="string")settings[key]=["length","unit","interval","top","lines","maximum","statistics"].includes(key)?Number(value):value;el.value=settings[key];}
  if(id.endsWith("-retained-chart"))settings.statistics=0;
  function showFields(){fields.maximum.parentElement.hidden=!trend&&["donut","pie"].includes(settings.shape);for(const key of ["length","unit"])fields[key].parentElement.hidden=settings.range!=="recent";for(const key of ["start","end"])fields[key].parentElement.hidden=settings.range!=="custom";}
  form.addEventListener("change",event=>{const key=Object.entries(fields).find(([,el])=>el===event.target)?.[0];if(!key)return;if(key==="length")roundNumber(event.target);const next={...settings,[key]:["length","unit","interval","top","lines","maximum","statistics"].includes(key)?Number(event.target.value):event.target.value};if(!event.target.checkValidity()||next.range==="custom"&&(!next.start||!next.end||new Date(next.start)>=new Date(next.end))){event.target.setAttribute("aria-invalid","true");feedback("chart-settings-message",ui("請檢查時間起訖或數值"),"error");return;}event.target.removeAttribute("aria-invalid");Object.assign(settings,next);showFields();saveView();if(data)renderCharts();feedback("chart-settings-message",ui("圖表已更新"));});
  showFields();controls.append(form);controls.hidden=true;const heading=$(id).previousElementSibling,head=node("div",null,"panel-head chart-head"),actions=node("div",null,"chart-actions"),open=button("⚙",()=>openChartSettings(id),"chart-setting-button"),cycle=button("",()=>{settings.shape=types[(types.findIndex(([key])=>key===settings.shape)+1)%types.length][0];showFields();saveView();renderCharts();},"chart-type-button");

  function updateType(){const index=types.findIndex(([key])=>key===settings.shape),label=ui(types[index][1]),next=ui(types[(index+1)%types.length][1]);cycle.textContent=label+" ↻";cycle.setAttribute("aria-label",ui(heading.dataset.copyBase||heading.textContent)+ui(" · 切換圖表類型, 目前 ")+label);cycle.title=ui("下一個: ")+next;}
  updateType();cycle.hidden=types.length<2;cycle.dataset.chartType=id;open.setAttribute("aria-label",heading.textContent+ui(" 圖表設定"));heading.replaceWith(head);actions.append(cycle,open);head.append(heading,actions);$(id).before(controls);const title=heading.dataset.copyBase||heading.textContent;let statisticsRoot,statisticsPanel;
  if(trend&&!id.endsWith("-retained-chart")){statisticsPanel=node("section",null,"panel chart-statistics-panel empty-statistics");statisticsPanel.id=id+"-statistics";statisticsPanel.dataset.visibilityKey=statisticsPanel.id;statisticsPanel.dataset.cardKey=statisticsPanel.id;statisticsPanel.dataset.chartStatistics=id;const caption=ui(title)+ui(" · 統計");statisticsRoot=node("div",null,$(id).closest(".overview-panels")?"bar-chart":"statistics-content");statisticsRoot.id=id+"-statistics-content";statisticsPanel.append(node("h3",caption),statisticsRoot);$(id).closest(".panel").after(statisticsPanel);}
  chartViews.set(id,{settings,fields,controls,title,updateType,statisticsRoot,statisticsPanel});
  const owner=baseId!==id?cardLibrary.get(baseId).owner:cardOwner($(id)),kind=trend?series?"比較":"趨勢":/排行/.test(title)?"排行":"分布";cardLibrary.set(id,{id,baseId,owner,title,kind,types,trend,series});
  if(statisticsPanel)cardLibrary.set(statisticsPanel.id,{id:statisticsPanel.id,baseId,owner,title:title+" · 統計",kind:"統計",types:[],chartId:id});
  if(statisticsPanel&&$(id).closest(".overview-panels")){const key=statisticsPanel.id;statisticsPanel.dataset.tabOrder=key;statisticsPanel.classList.add("tab-order-row");overviewPanels.set(key,statisticsPanel);overviewDefault.push(key);overviewDefaultHidden.push(key);overviewOrder.push(key);if(!preferences.overview?.order?.includes(key)||preferences.overview.hidden?.includes(key))overviewHidden.add(key);}
}
function openChartSettings(id){$("chart-settings-message").textContent="";chartSelection=id;const view=chartViews.get(id);$("chart-settings-title").textContent=ui(view.title)+ui(" · 圖表設定");view.controls.hidden=false;$("chart-settings-content").replaceChildren(view.controls);if(!$("chart-dialog").open)openDialog($("chart-dialog"));}
$("chart-settings-close").addEventListener("click",()=>$("chart-dialog").close());$("chart-dialog").addEventListener("close",()=>{if(chartSelection){const view=chartViews.get(chartSelection);view.controls.hidden=true;$(chartSelection).before(view.controls);chartSelection=null;}});
function pathNames(paths){
  const parts=[...new Set(paths)].map(path=>({path,parts:path.replace(/\\/g,"/").split("/").filter(Boolean)})),names=Object.create(null);
  for(const item of parts){let size=1,label=item.parts.slice(-size).join("/");while(size<item.parts.length&&parts.some(other=>other!==item&&other.parts.slice(-size).join("/")===label))label=item.parts.slice(-++size).join("/");names[item.path]=label||item.path;}
  return names;
}
function rankTitle(id,key,label){return /^(?:file|folder)-(?:count|read|write|size|read-count|write-count)-rank$/.test(cardLibrary.get(id)?.baseId||id)?key:label;}
function renderRankingCard(id,kind,field=null,folder=false,operation=null){
  const groups=new Map();if(kind==="size"){const files=filteredFiles(),snapshot=fileSnapshots.get(activeSourceWindow()),locations=new Set(files.map(fileLocation)),sizes=new Map((snapshot?.files||[]).filter(file=>locations.has(fileLocation(file))&&Number.isFinite(file.bytes)).map(file=>[fileLocation(file),file]));bars(id,Object.fromEntries([...sizes].map(([path,file])=>[path,file.bytes])),pathNames([...sizes.keys()]),path=>openDetail({kind:"file",event:sizes.get(path),fileMetadata:sizes.get(path)}),"bytes");
  }else if(kind==="url"||kind==="site"){for(const item of references(ranged(id,(data.mcp?.events||[]).filter(event=>event.server==="web")))){const key=kind==="url"?item.url:new URL(item.url).hostname;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(item);}bars(id,Object.fromEntries([...groups].map(([key,items])=>[key,items.length])),null,key=>openDetail({kind:"web-reference",key,items:groups.get(key)}));
  }else{for(const item of ranged(id,filteredFiles())){if(operation==="read"&&item.operation!=="read"||operation==="change"&&item.operation==="read"||field&&!Number.isFinite(item[field]))continue;const path=fileLocation(item),key=folder?path.replace(/[\\/][^\\/]*$/,""):path;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(item);}bars(id,Object.fromEntries([...groups].map(([key,items])=>[key,field?items.reduce((total,item)=>total+item[field],0):items.length])),pathNames([...groups.keys()]),path=>openDetail({kind:"file-group",path,items:groups.get(path)}),field?"bytes":ui("次"));if(field&&!groups.size)$(id).querySelector(".empty").textContent=ui(field==="read_bytes"?"來源未提供讀取量":"來源未提供寫入量");}
  const view=chartViews.get(id);if(view)view.renderData={kind:"rank",args:[kind,field,folder,operation]};
}
function renderActivityRankings(){
  for(const [id,kind]of [["web-url-rank","url"],["web-site-rank","site"]])renderRankingCard(id,kind);
  for(const [id,field,folder,operation]of [["file-count-rank",null,false],["folder-count-rank",null,true],["file-read-count-rank",null,false,"read"],["file-write-count-rank",null,false,"change"],["file-read-rank","read_bytes",false],["file-write-rank","write_bytes",false]])renderRankingCard(id,"file",field,folder,operation);
  renderRankingCard("file-size-rank","size");
}

function renderCharts(){
  if(!data)return;for(const view of chartViews.values())view.updateType();renderActivityRankings();const c=data.codex,threads=c.threads||[],events=data.mcp?.events||[];
  bars("usage-model-chart",usageModelCounts,{unknown:"--"},model=>openDetail({kind:"usage-model",model}),"tokens",usageModelSegments);
  timeline("activity-chart","activity-chart-note",c.activity_series||[]);
  bars("top-tools",cardId=>ranged(cardId,c.tool_series||[]).reduce((out,e)=>(out[e.tool]=(out[e.tool]||0)+e.calls,out),Object.create(null)),null,tool=>openDetail({kind:"tool",tool}));
  bars("overview-source-column",cardId=>countBy(ranged(cardId,events),"server"));bars("overview-model-ring",cardId=>countBy(ranged(cardId,threads),"model"),{unknown:"--"},null,ui("對話數"));bars("overview-model-pie",cardId=>countBy(ranged(cardId,threads),"model"),{unknown:"--"},null,ui("對話數"));bars("overview-token-stack",usageModelCounts,{unknown:"--"},null,"tokens",usageModelSegments);
  bars("source-chart",cardId=>countBy(ranged(cardId,events),"server"),null,server=>{if(server==="web"){navigateToTab("web");return;}navigateToMcpSource(server);});
  bars("model-chart",cardId=>countBy(ranged(cardId,threads),"model"),{unknown:ui("未知")},null,ui("對話數"));
  bars("environment-chart",cardId=>countBy(ranged(cardId,threads),"environment"),labels.environment,null,ui("對話數"));
  bars("trigger-chart",cardId=>countBy(ranged(cardId,threads),"trigger"),labels.trigger,null,ui("對話數"));
  bars("mcp-source-chart",cardId=>({...Object.fromEntries((data.mcp?.servers||[]).filter(source=>source.server!=="web").map(source=>[source.server,0])),...countBy(ranged(cardId,events.filter(event=>event.server!=="web"&&!event.nested)),"server")}),null,navigateToMcpSource);
  bars("mcp-action-chart",cardId=>countBy(ranged(cardId,events.filter(e=>e.server!=="web"&&(mcpSource==="all"||e.server===mcpSource))),"action"),actionLabels);
  bars("sqlite-operation-chart",cardId=>countBy(ranged(cardId,c.sqlite?.events||[]),"operation"),sqlLabels);
  const git=(c.git?.events||[]).filter(e=>$("filter-git").value==="all"||e.operation===$("filter-git").value);
  bars("git-chart",cardId=>countBy(ranged(cardId,git),"operation"),null,key=>{$("filter-git").value=key;renderGit();renderCharts();});
  timeline("git-time-chart","git-chart-note",hourly(git));categoryTimeline("git-breakdown-chart","git-breakdown-note",git,"operation");
  bars("skill-chart",cardId=>countBy(ranged(cardId,c.skills?.events||[]),"skill"),null,skill=>openDetail({kind:"skill",skill}));
  bars("check-chart",cardId=>countBy(ranged(cardId,c.checks||[]),"operation"));
  timeline("web-time-chart","web-chart-note",hourly(events.filter(e=>e.server==="web")));
  const files=filteredFiles();bars("file-operation-chart",cardId=>countBy(ranged(cardId,files),"operation"),fileLabels);timeline("file-time-chart","file-chart-note",hourly(files));categoryTimeline("file-breakdown-chart","file-breakdown-note",files,"operation",fileLabels);
  timeline("conversation-time-chart","conversation-time-note",c.activity_series);
  bars("conversation-model-chart",cardId=>countBy(ranged(cardId,threads),"model"),{unknown:ui("未知")},null,ui("對話數"));bars("conversation-environment-chart",cardId=>countBy(ranged(cardId,threads),"environment"),labels.environment,null,ui("對話數"));
  const errors=data.errors?.events||[],logs=logData?.entries||[];
  timeline("log-time-chart","log-time-note",hourly(logs));categoryTimeline("log-level-chart","log-level-note",logs,"severity");bars("log-source-chart",cardId=>countBy(ranged(cardId,logs),"source"),Object.fromEntries((logData?.sources||[]).map(source=>[source.source,logSource(source.source)])));
  bars("error-source-chart",cardId=>countBy(ranged(cardId,errors),"category"),errorCategories,()=>navigateToTab("errors"));
  timeline("error-time-chart","error-chart-note",errors.map(e=>({time:e.timestamp,calls:1})),"次");
  for(const [id,key,unit,average]of [["refresh","refresh_ms","ms",true],["cpu","cpu_ms","ms",true]])timeline("monitor-"+id+"-chart","monitor-"+id+"-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample[key]})),unit,average);
  timeline("monitor-read-chart","monitor-read-note",data.monitor?.history,"bytes",true,[{key:"read_bytes",label:"本輪讀取量"},{key:"activity_cache_bytes",label:"活動快取大小"},{key:"snapshot_bytes",label:"上次資料大小"},{key:"transfer_bytes",label:"上次傳輸大小"}]);
  for(const [id,key]of [["sqlite","sql_records"],["web","web_records"],["mcp","mcp_records"]])timeline(id+"-retained-chart",id+"-retained-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample[key]})),ui("筆"),true);
  timeline("skill-time-chart","skill-time-note",hourly(c.skills?.events||[]));timeline("tool-time-chart","tool-time-note",(c.tool_series||[]).map(event=>({time:event.timestamp||event.time,calls:event.calls})));timeline("check-time-chart","check-time-note",hourly(c.checks||[]));timeline("sqlite-time-chart","sqlite-time-note",hourly(c.sqlite?.events||[]));
  categoryTimeline("skill-breakdown-chart","skill-breakdown-note",c.skills?.events||[],"skill");categoryTimeline("tool-breakdown-chart","tool-breakdown-note",c.tool_series||[],"tool",null,"calls");categoryTimeline("check-breakdown-chart","check-breakdown-note",c.checks||[],"operation");categoryTimeline("sqlite-breakdown-chart","sqlite-breakdown-note",c.sqlite?.events||[],"operation",sqlLabels);
  const calls=events.filter(event=>event.server!=="web"&&!event.nested&&(mcpSource==="all"||event.server===mcpSource));timeline("mcp-time-chart","mcp-time-note",hourly(calls));categoryTimeline("mcp-breakdown-chart","mcp-breakdown-note",calls,"server");
  const urls=references(events.filter(event=>event.server==="web")).map(item=>({...item,site:new URL(item.url).hostname}));categoryTimeline("web-url-time-chart","web-url-time-note",urls,"url");categoryTimeline("web-site-time-chart","web-site-time-note",urls,"site");
  const toolEvents=threads.flatMap(thread=>(thread.tool_events||[]).map(event=>({...event,thread_id:thread.thread_id})));timeline("tool-duration-chart","tool-duration-note",toolEvents.map(event=>({time:event.timestamp,calls:event.duration_ms})),"ms",true);
  activeThreadsTimeline("skill-thread-chart","skill-thread-note",c.skills?.events||[]);activeThreadsTimeline("git-thread-chart","git-thread-note",git);activeThreadsTimeline("check-thread-chart","check-thread-note",c.checks||[]);categoryTimeline("error-category-chart","error-category-note",errors,"category",errorCategories);categoryTimeline("error-level-chart","error-level-note",errors,"level");
  const paths=files.map(event=>({...event,location:fileLocation(event),folder:fileLocation(event).replace(/[\\/][^\\/]*$/,"")}));categoryTimeline("file-path-time-chart","file-path-time-note",paths,"location",pathNames(paths.map(event=>event.location)));categoryTimeline("file-folder-time-chart","file-folder-time-note",paths,"folder",pathNames(paths.map(event=>event.folder)));
  bars("overview-tools-chart",cardId=>ranged(cardId,c.tool_series||[]).reduce((out,e)=>(out[e.tool]=(out[e.tool]||0)+e.calls,out),Object.create(null)));bars("overview-token-chart",usageModelCounts,{unknown:"--"},null,"tokens",usageModelSegments);bars("overview-error-chart",cardId=>countBy(ranged(cardId,errors),"category"),errorCategories);bars("overview-file-chart",cardId=>countBy(ranged(cardId,c.file_activity?.events||[]),"operation"),fileLabels);bars("overview-sql-chart",cardId=>countBy(ranged(cardId,c.sqlite?.events||[]),"operation"),sqlLabels);timeline("overview-cpu-chart","overview-cpu-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample.cpu_ms})),"ms",true);bars("overview-git-chart",cardId=>countBy(ranged(cardId,c.git?.events||[]),"operation"),null,()=>{switchTab("workflow");selectSubPage("workflow","git","activity-tabs",activityPages);scrollToSection("view-workflow");});
  bars("overview-skill-chart",cardId=>countBy(ranged(cardId,c.skills?.events||[]),"skill"));bars("overview-check-chart",cardId=>countBy(ranged(cardId,c.checks||[]),"operation"));
  timeline("overview-web-chart","overview-web-note",hourly(events.filter(event=>event.server==="web")));timeline("overview-error-time-chart","overview-error-time-note",errors.map(event=>({time:event.timestamp,calls:1})),"次");
  for(const [id,key,unit,average]of [["refresh","refresh_ms","ms",true],["read","read_bytes","bytes",false]])timeline("overview-"+id+"-chart","overview-"+id+"-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample[key]})),unit,average);
  renderAllowance("overview-quota-chart",c.usage);renderAllowance("overview-account-quota",c.usage);
  bars("overview-dot-chart",cardId=>countBy(ranged(cardId,c.dots?.events||[]),"artifact_type"),null,null,ui("項"));
  renderOverviewCopies();applyOverview();for(const view of tableViews.values())adaptTable(view);arrangeOverview();
  applyContentVisibility();arrangeContentCards();syncHelp();
}
function applyAppearance(){
  if(!["auto","light","dark"].includes(appearance.mode))appearance.mode="dark";
  if(!["slate","neutral"].includes(appearance.theme))appearance.theme="slate";
  if(!["green","blue","orange"].includes(appearance.accent))appearance.accent="green";
  appearance.font=Math.min(18,Math.max(12,Number.isInteger(appearance.font)?appearance.font:14));
  const mode=appearance.mode==="auto"?(systemDark.matches?"dark":"light"):appearance.mode,root=document.documentElement;
  root.dataset.mode=mode;root.dataset.theme=appearance.theme;root.dataset.accent=appearance.accent;root.dataset.font=appearance.font;
  $("dark-toggle").setAttribute("aria-pressed",String(mode==="dark"));$("dark-toggle").title=mode==="dark"?ui("切換淺色模式"):ui("切換深色模式");$("dark-toggle").setAttribute("aria-label",$("dark-toggle").title);
  for(const el of document.querySelectorAll("#mode-options [data-mode]"))el.setAttribute("aria-pressed",String(el.dataset.mode===appearance.mode));
  $("theme-select").value=appearance.theme;$("accent-select").value=appearance.accent;$("font-size").value=appearance.font;if(data)for(const view of tableViews.values())adaptTable(view);syncHelp();
}
let diagnosticSource=preferences.tab==="logs"?"logs":preferences.diagnosticSource||"errors",activitySource=["git","checks"].includes(preferences.tab)?preferences.tab:["git","checks"].includes(preferences.activitySource)?preferences.activitySource:"git";
function selectSubPage(parent,key,navId,entries){for(const [value,tabId,panelId]of entries){const active=value===key;$(tabId).setAttribute("aria-selected",String(active));$(tabId).tabIndex=active?0:-1;$(panelId).hidden=!active;}if(parent==="errors"){diagnosticSource=key;preferences.diagnosticSource=key;if(key==="logs"&&data&&!$("view-errors").hidden)loadLogs();}else{activitySource=key;preferences.activitySource=key;$("git-cards").hidden=key!=="git";$("check-cards").hidden=key!=="checks";}if(data){syncSourceWindow();renderCharts();renderSources();saveView();if(data.codex?.activity_scope?.window!==activeSourceWindow()){version++;logData=null;refresh();}}}
const diagnosticPages=[["errors","diagnostic-errors","error-content"],["logs","tab-logs","view-logs"]],activityPages=[["git","tab-git","view-git"],["checks","tab-checks","view-checks"]];
function bindSubPages(parent,navId,entries,selected){for(const [index,[key,tabId]]of entries.entries()){$(tabId).addEventListener("click",()=>selectSubPage(parent,key,navId,entries));$(tabId).addEventListener("keydown",event=>{if(event.altKey||!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?entries.length-1:(index+(["ArrowRight","ArrowDown"].includes(event.key)?1:entries.length-1))%entries.length;$(entries[next][1]).click();$(entries[next][1]).focus();});}selectSubPage(parent,selected,navId,entries);}
function switchTab(name){
  if(["errors","logs"].includes(name)){selectSubPage("errors",name==="logs"?"logs":diagnosticSource,"diagnostic-tabs",diagnosticPages);name="errors";}
  if(["git","checks","workflow"].includes(name)){selectSubPage("workflow",name==="workflow"?activitySource:name,"activity-tabs",activityPages);name="workflow";}
  if(name==="jev"){name="mcp";mcpSource="jev";if(data)renderMcp();}
  for(const tab of document.querySelectorAll("[data-tab]")){const active=tab.dataset.tab===name;tab.setAttribute("aria-selected",String(active));tab.tabIndex=active?0:-1;$("view-"+tab.dataset.tab).hidden=!active;}
  if(data){const previousWindow=data.codex?.activity_scope?.window;syncSourceWindow();renderCharts();renderSources();saveView();if(name==="files"&&previousWindow===activeSourceWindow()&&!fileSnapshots.has(activeSourceWindow())&&!fileSnapshotRequests.has(activeSourceWindow())&&!fileSummaryBusy)loadFileSizes();if(previousWindow!==activeSourceWindow()){version++;logData=null;refresh();}}
  if(data&&(name==="errors"&&diagnosticSource==="logs"||name==="overview"&&overviewNeeds("view-logs")))loadLogs();if(data&&name==="overview"&&overviewNeeds("view-files")&&!fileSnapshots.has(activeSourceWindow())&&!fileSnapshotRequests.has(activeSourceWindow())&&!fileSummaryBusy)loadFileSizes();
}
function scrollToSection(id){requestAnimationFrame(()=>{const target=$(id);if(target&&!target.hidden)target.scrollIntoView({block:"start",behavior:"instant"});else window.scrollTo({top:0,behavior:"instant"});});}
function navigateToTab(name,anchor){switchTab(name);scrollToSection(anchor||"view-"+({jev:"mcp",logs:"errors",git:"workflow",checks:"workflow"}[name]||name));}
function navigateToMcpSource(source){switchTab("mcp");selectMcpSource(source);scrollToSection("view-mcp");}

function setupCollapsibleSections(){
  for(const section of document.querySelectorAll("details[data-collapsible]")){
    const key=section.id;section.open=preferences.sectionCollapsed?.[key]===false;
    section.addEventListener("toggle",()=>{if((preferences.sectionCollapsed?.[key]!==false)===!section.open)return;preferences.sectionCollapsed={...preferences.sectionCollapsed,[key]:!section.open};saveView();});
  }
}
function filteredThreads(){
  const query=$("thread-search").value.trim().toLocaleLowerCase(),filters=[["filter-type","activity_type"],["filter-environment","environment"],["filter-status","status"],["filter-reasoning","reasoning_effort"],["filter-model","model"]];
  return (data?.codex.threads||[]).filter(t=>{
    if(query&&!(title(t)+" "+t.thread_id).toLocaleLowerCase().includes(query))return false;
    if(filters.some(([id,key])=>$(id).value!=="all"&&$(id).value!==(t[key]||"unknown")))return false;
    const project=$("filter-project").value,trigger=$("filter-trigger").value;
    if(project!=="all"&&project!==(t.project_scope||"unknown")&&project!=="id:"+t.project_id)return false;
    const source=t.trigger==="orbit"?"dot":t.trigger==="automation"?"schedule":t.trigger||"unknown";
    return trigger==="all"||trigger===source||trigger==="schedule"&&t.has_schedule;
  });
}
function reasoningBadge(value){const level=["none","minimal","low","medium","high","xhigh","max","ultra"].includes(value)?value:"unknown";return tag(value||"--","reasoning-level effort-"+level);}
function threadLink(t,text){const el=button(text||title(t),()=>openDetail({kind:"thread",id:t.thread_id,thread:t}),text?"thread-open-button":"link");el.dataset.sortTime=t.updated_at||t.created_at||"";return el;}
function toolButton(tool,action,count){const match=tool.match(/^mcp__(.+)__(.+)$/),server=match?.[1]||(/^web[._]/.test(tool)?"web":null),label=match?server+"."+match[2]:server==="web"?"web.run":tool,el=button("",action,"tool"),identity=node("span",null,"tool-identity");identity.append(node("span",label,"tool-name mono"));if(server)identity.append(document.createTextNode(" "),tag(server==="web"?"Web":"MCP"));el.append(identity);if(count!=null)el.append(node("b",fmt(count)));el.title=tool;el.setAttribute("aria-label",tool);return el;}
function eventProject(event){const thread=threadIndex.get(event.thread_id)||event;return thread.project_name||thread.project_id||(thread.project_scope==="none"?ui("無專案"):null);}
function projectCell(row,event){return cell(row,eventProject(event)||"--");}
function eventThreadCell(row,event){const t=threadIndex.get(event.thread_id)||{...event,metadata_only:true,event_only:true};cell(row).append(threadLink(t));}
function updateProjects(){
  const select=$("filter-project"),selected=select.value,projects=new Map();
  for(const t of data.codex.threads||[])if(t.project_id)projects.set(t.project_id,t.project_name||t.project_id);
  const base=[["all",ui("全部專案")],["project",ui("專案內")],["none",ui("無專案")],["unknown",ui("未知")]];
  select.replaceChildren(...base.concat([...projects].map(([id,name])=>["id:"+id,name])).map(([value,label])=>{const option=node("option",label);option.value=value;return option;}));
  if([...select.options].some(option=>option.value===selected))select.value=selected;
  if(pendingProject){if([...select.options].some(option=>option.value===pendingProject))select.value=pendingProject;pendingProject=null;}
}
function statusBadge(thread){const status=thread.status,backfill=status==="observed"&&thread.status_backfill_pending,tone=backfill?"backfill":["running","active","in_progress"].includes(status)?"running":["completed","success","ok"].includes(status)?"completed":["error","failed","timeout"].includes(status)?"error":["pending","queued"].includes(status)?"waiting":"neutral",badge=tag(backfill?ui("回補中"):labels.status[status]||status||"--","thread-status status-"+tone);bindHelp(badge,ui("狀態確認時間")+": "+when(thread.status_updated_at)+(thread.status_source==="cached_lifecycle"?" · "+ui("沿用最近確認的工作事件"):"")+(thread.status_backfill_pending?" · "+ui("正在回補工作起訖紀錄"):""));return badge;}
function subagentHeaders(){return [ui("時間"),ui("對話"),ui("子代理程式"),ui("角色"),ui("狀態"),ui("模型"),ui("推理等級"),ui("工具呼叫"),ui("最近活動"),ui("父對話")];}
function subagentRows(threads){return threads.map(thread=>{const row=node("tr"),parent=(data.codex.threads||[]).find(item=>item.thread_id===thread.execution.parent_thread_id);cell(row,when(thread.created_at));cell(row).append(threadLink(thread));cell(row,thread.execution.agent_nickname||"--");cell(row,thread.execution.agent_role||"--");cell(row).append(statusBadge(thread));cell(row,thread.model||"--","mono");cell(row).append(reasoningBadge(thread.reasoning_effort));valueCell(row,thread.tool_calls);cell(row,when(thread.updated_at));if(parent)cell(row).append(threadLink(parent));else cell(row,thread.execution.parent_thread_id,"mono");return clickableRow(row,()=>openDetail({kind:"thread",id:thread.thread_id,thread}));});}
function projectIdentity(project){const label=node("span",null,"project-identity");if(project.icon&&typeof project.icon==="string"&&!/[<>\/]/.test(project.icon)&&project.icon.length<=8)label.append(node("span",project.icon,"project-icon"));label.append(node("span",project.name||project.id));return label;}
function projectRows(){const projects=new Map((data.codex.projects||[]).map(project=>[project.id,project]));for(const thread of data.codex.threads||[])if(thread.project_id&&!projects.has(thread.project_id))projects.set(thread.project_id,{id:thread.project_id,name:thread.project_name||thread.project_id,kind:thread.project_kind});return [...projects.values()];}
function projectKind(project){return labels.type[project.kind]||({local:ui("本機"),cloud:ui("雲端")})[project.kind]||project.kind||ui("來源未提供");}
function renderProjects(){const projects=projectRows();replaceRows("project-rows",...projects.map(project=>{const row=node("tr"),threads=(data.codex.threads||[]).filter(thread=>thread.project_id===project.id),recent=threads.map(thread=>thread.updated_at).filter(Boolean).sort().at(-1)||project.updated_at;cell(row).append(button(project.name||project.id,()=>openDetail({kind:"project",project}),"link"));cell(row).append(tag(projectKind(project)));valueCell(row,threads.length);cell(row,when(recent));return clickableRow(row,()=>openDetail({kind:"project",project}));}));$("project-empty").textContent=projects.length?ui("對話數與關聯只涵蓋目前已載入的對話"):ui("目前載入範圍沒有專案資料");let buttonRoot=$("global-instructions");if(!buttonRoot){buttonRoot=button(ui("查看全域 AGENTS.md"),()=>openDetail({kind:"instructions",scope:"global"}));buttonRoot.id="global-instructions";$("project-cards").append(buttonRoot);}}
function renderSubagents(){const threads=(data.codex.threads||[]).filter(thread=>thread.execution?.parent_thread_id);replaceRows("subagent-rows",...subagentRows(threads));$("subagent-empty").textContent=threads.length?ui("顯示已載入且有上層 Thread ID 的子代理程式, 狀態取自來源最近紀錄"):ui("目前載入範圍沒有可關聯的子代理程式 紀錄");}
function renderCodex(){
  const c=data.codex;renderSubagents();renderProjects();
  selectOptions("filter-status",[["all",ui("全部狀態")],...[...new Set(["running","completed","observed",...(c.threads||[]).map(t=>t.status).filter(Boolean)])].map(value=>[value,labels.status[value]||value])]);
  if(pendingStatus){if([...$("filter-status").options].some(option=>option.value===pendingStatus))$("filter-status").value=pendingStatus;pendingStatus=null;}
  selectOptions("filter-reasoning",[["all",ui("全部等級")],...[...new Set((c.threads||[]).map(t=>t.reasoning_effort).filter(Boolean))].sort().map(value=>[value,value]),["unknown",ui("未知")]]);
  if(pendingReasoning){if([...$("filter-reasoning").options].some(option=>option.value===pendingReasoning))$("filter-reasoning").value=pendingReasoning;pendingReasoning=null;}
  selectOptions("filter-model",[["all",ui("全部模型")],...[...new Set((c.threads||[]).map(t=>t.model).filter(value=>value&&value!=="unknown"))].sort().map(value=>[value,value]),["unknown",ui("未知")]]);
  if(pendingModel){if([...$("filter-model").options].some(option=>option.value===pendingModel))$("filter-model").value=pendingModel;pendingModel=null;}
  const threads=sortRecords("codex-rows",filteredThreads(),[title,t=>t.thread_id,t=>t.project_name||t.project_id,t=>labels.type[t.activity_type],t=>labels.environment[t.environment],t=>labels.trigger[t.trigger],t=>t.model,t=>labels.status[t.status]||t.status,t=>t.reasoning_effort,...Object.keys(tokenLabels).map(key=>t=>t.tokens?.[key]),t=>cacheHit(t.tokens),t=>t.tool_calls,t=>t.task_duration_ms,t=>t.updated_at],t=>t.updated_at),tools=toolCounts(threads),all=$("page-size").value==="all";
  const size=all?Math.max(threads.length,1):Number($("page-size").value);
  pageCount=Math.max(1,Math.ceil(threads.length/size));page=Math.min(Math.max(page,1),pageCount);
  const start=(page-1)*size,visible=threads.slice(start,start+(all?lazyLimit:size));
  $("codex-scope").textContent=c.health==="disabled"?ui("檢查已關閉"):ui("追蹤 ")+fmt(c.files||0)+ui(" 個 session 檔案 · ")+fmt(c.threads?.length||0)+ui(" 個對話");
  syncSessionTracking();
  cards("codex-cards",[[ui("符合篩選的對話"),fmt(threads.length),threads.length!==(c.threads?.length||0)?ui("全部 ")+fmt(c.threads?.length||0)+ui(" 個"):""],[ui("執行中"),fmt(threads.filter(t=>t.status==="running").length),ui("已完成 ")+fmt(threads.filter(t=>t.status==="completed").length)+ui(" 個")],[ui("工具呼叫"),fmt(Object.values(tools).reduce((sum,count)=>sum+count,0)),ui("工具種類 ")+fmt(Object.keys(tools).length)],[ui("累計讀取量"),fmt(Math.round((c.bytes_read||0)/1024))+" KiB",ui("損壞紀錄 ")+fmt(c.malformed_lines||0)+ui(" 行")]]);
  replaceRows("codex-rows",...visible.map(t=>{
    const row=node("tr"),identity=cell(row,null,"thread-cell");
    identity.append(node("span",title(t),"thread-name"));
    cell(row,t.thread_id,"mono thread-id");cell(row,t.project_name||t.project_id||(t.project_scope==="none"?ui("無專案"):"--"));
    for(const value of [labels.type[t.activity_type],labels.environment[t.environment],labels.trigger[t.trigger]])cell(row).append(tag(value||"--"));
    cell(row,t.model||"--","mono");cell(row).append(statusBadge(t));
    cell(row).append(reasoningBadge(t.reasoning_effort));
    for(const key of Object.keys(tokenLabels))valueCell(row,t.tokens?.[key]);
    const rate=cacheHit(t.tokens),cache=valueCell(row,rate);if(rate!=null)cache.textContent=fmt(rate)+"%";
    valueCell(row,t.tool_calls);valueCell(row,t.task_duration_ms==null?null:Math.round(t.task_duration_ms/100)/10);cell(row,when(t.updated_at));
    return clickableRow(row,()=>openDetail({kind:"thread",id:t.thread_id,thread:t}),ui("開啟對話與工具明細"));
  }));
  $("codex-empty").textContent=threads.length?"":ui("沒有符合條件的對話");
  $("page-summary").textContent=threads.length?ui("第 ")+fmt(start+1)+" - "+fmt(Math.min(start+(all?lazyLimit:size),threads.length))+ui(" 筆, 共 ")+fmt(threads.length)+ui(" 筆"):ui("共 0 筆");
  $("load-more").hidden=!all||lazyLimit>=threads.length;$("load-more").textContent=ui("載入更多對話 (已顯示 ")+visible.length+" / "+threads.length+")";
  syncPageInput($("page-number"),page,pageCount);$("page-total").textContent="/ "+pageCount;
  for(const id of ["page-first","page-prev"])$(id).disabled=page===1;
  for(const id of ["page-next","page-last"])$(id).disabled=page===pageCount;
}
function selectOptions(id,items){const el=$(id),previous=el.value||preferences.inputs?.[id];el.replaceChildren(...items.map(([value,text])=>{const option=node("option",text);option.value=value;return option;}));if([...el.options].some(o=>o.value===previous))el.value=previous;}
function references(events){return events.flatMap(event=>[...new Set([...(event.metadata?.references||[]),...(event.result?.references||[])])].map(url=>({...event,url,reference_source:event.result?.references?.includes(url)?ui("工具回傳"):ui("開啟頁面")})));}
function referenceLink(url){const link=node("a",url,"reference-link");try{const parsed=new URL(url);if(!["https:","http:"].includes(parsed.protocol)||parsed.username||parsed.password)return node("span","--");link.href=parsed.href;}catch{return node("span","--");}link.target="_blank";link.rel="noopener noreferrer";return link;}
function renderTools(){
  const c=data.codex,threads=c.threads||[],tools=c.tools||{},nested=c.nested_tools||{},m=data.mcp||{servers:[],events:[],categories:{}};
  const servers=m.servers.filter(s=>s.enabled),direct=servers.reduce((n,s)=>n+s.calls,0),errors=servers.reduce((n,s)=>n+s.errors,0),known=servers.reduce((n,s)=>n+s.known_status,0);
  $("tools-scope").textContent=fmt(Object.keys(tools).length)+ui(" 種工具 · ")+fmt(c.observed_tool_calls||0)+ui(" 次呼叫");
  cards("tool-cards",[[ui("工具呼叫"),fmt(c.observed_tool_calls||0),""],[ui("工具種類"),fmt(Object.keys(tools).length),""],[ui("exec 辨識"),fmt(Object.values(nested).reduce((n,value)=>n+value,0)),ui("內層工具 ")+fmt(Object.keys(nested).length)+ui(" 種")],[ui("有回傳時間"),fmt(threads.flatMap(t=>t.tool_events||[]).filter(e=>e.completed_at).length),""]]);
  $("nested-tools").replaceChildren(...sorted(nested).map(([tool,count])=>{const el=toolButton(tool,()=>openDetail({kind:"nested-tool",tool}),count);return el;}));
  if(!Object.keys(nested).length)$("nested-tools").append(node("p","--","empty"));
  const statistics=c.tool_statistics||[...Object.entries(c.tools||{}).map(([tool,calls])=>({tool,calls,nested:false})),...Object.entries(c.nested_tools||{}).map(([tool,calls])=>({tool,calls,nested:true}))];
  replaceRows("tool-stat-rows",...statistics.map(item=>{const row=node("tr");cell(row).append(toolButton(item.tool,()=>openDetail({kind:item.nested?"nested-tool":"tool",tool:item.tool})));cell(row,ui(item.nested?"程式碼辨識":"直接呼叫"));for(const key of ["calls","thread_count","returned","known_duration_count","average_ms","p99_ms"])valueCell(row,item[key]);cell(row,when(item.last_at));cell(row,when(item.last_response_at));return row;}));
  $("tool-stat-empty").textContent=statistics.length?"":ui("尚無工具紀錄");
  attachTables();
}
let mcpSection="activity";const mcpFileLists=new Map();
let mcpSource=typeof preferences.mcpSource==="string"?preferences.mcpSource:preferences.tab==="jev"?"jev":"all";
function selectMcpSource(source){
  mcpSource=source;preferences.mcpSource=source;mcpPage=1;$("filter-mcp-server").value=source;$("filter-mcp-category").value="all";$("filter-mcp-result").value="all";
  if(data){renderMcp();renderCharts();attachTables();renderSources();saveView();}if($("mcp-source-picker")?.tagName==="DETAILS"&&!dragEnabled)$("mcp-source-picker").open=false;
}

function dragSubTabs(nav,key,identity){
  if(nav.closest("dialog"))return;
  const tabs=[...nav.children],saved=Array.isArray(preferences.subOrders?.[key])?preferences.subOrders[key]:[],order=[...new Set(saved.filter(id=>tabs.some(tab=>identity(tab)===id)).concat(tabs.map(identity)))];
  for(const id of order)nav.append(tabs.find(tab=>identity(tab)===id));
  const config={root:nav,horizontal:true,order:()=>[...nav.children].map(tab=>tab.dataset.tabOrder),label:id=>[...nav.children].find(tab=>tab.dataset.tabOrder===id)?.textContent||id,move:(id,target,after)=>{const order=config.order().filter(key=>key!==id),index=order.indexOf(target);if(index<0||id===target)return;order.splice(index+(after?1:0),0,id);for(const key of order)nav.append([...nav.children].find(tab=>tab.dataset.tabOrder===key));preferences.subOrders={...preferences.subOrders,[key]:order};saveView();}};
  for(const tab of nav.children){tab.classList.add("tab-order-row");tab.dataset.tabOrder=identity(tab);if(tab.dataset.dragWired)return;tab.dataset.dragWired="true";dragTab(tab,tab.dataset.tabOrder,config);tab.addEventListener("keydown",event=>{if(event.altKey||!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();event.stopImmediatePropagation();const tabs=[...nav.children].filter(tab=>!tab.hidden),index=tabs.indexOf(tab),next=event.key==="Home"?0:event.key==="End"?tabs.length-1:(index+(event.key==="ArrowRight"?1:tabs.length-1))%tabs.length;tabs[next].click();tabs[next].focus();},true);}
}
function arrangeHighlights(){const root=$("overview-highlights"),cards=[...root.children],saved=Array.isArray(preferences.highlightOrder)?preferences.highlightOrder:[],order=[...new Set(saved.filter(id=>cards.some(card=>card.dataset.tabOrder===id)).concat(cards.map(card=>card.dataset.tabOrder)))];for(const id of order)root.append(cards.find(card=>card.dataset.tabOrder===id));const config={root,grid:true,order:()=>[...root.children].map(card=>card.dataset.tabOrder),label:id=>[...root.children].find(card=>card.dataset.tabOrder===id)?.querySelector("h3").textContent||id,move:(id,target,after)=>{const order=config.order().filter(key=>key!==id),index=order.indexOf(target);if(index<0||id===target)return;order.splice(index+(after?1:0),0,id);for(const key of order)root.append([...root.children].find(card=>card.dataset.tabOrder===key));preferences.highlightOrder=order;saveView();}};for(const card of root.children)dragTab(card,card.dataset.tabOrder,config);}


function dragCardGroup(root,key,identity){const cards=[...root.children].filter(card=>card.matches(".panel,.source-card"));if(cards.length<2)return;const saved=Array.isArray(preferences.cardOrders?.[key])?preferences.cardOrders[key]:[],order=[...new Set(saved.filter(id=>cards.some(card=>identity(card)===id)).concat(cards.map(identity)))];for(const id of order)root.append(cards.find(card=>identity(card)===id));const config={root,grid:true,order:()=>[...root.children].filter(card=>!card.hidden&&!card.classList.contains("layout-hidden")).map(identity),label:id=>cards.find(card=>identity(card)===id)?.querySelector("h3,.source-name,.table-disclosure>summary")?.textContent||id,move:(id,target,after)=>{const order=[...root.children].map(identity).filter(key=>key!==id),index=order.indexOf(target);if(index<0||id===target)return;order.splice(index+(after?1:0),0,id);for(const key of order)root.append(cards.find(card=>identity(card)===key));preferences.cardOrders={...preferences.cardOrders,[key]:order};saveView();}};for(const card of cards){card.classList.add("tab-order-row");card.dataset.tabOrder=identity(card);const handle=card.tagName==="BUTTON"?card:card.querySelector("h3,.table-disclosure>summary");if(!handle||handle.dataset.cardDragWired)continue;handle.dataset.cardDragWired="true";if(handle.tagName!=="BUTTON"){handle.tabIndex=0;handle.classList.add("direct-drag");}dragTab(handle,identity(card),config);}}
function arrangeContentCards(){
  const isChart=card=>card.matches("section.panel")&&!card.matches(".table-panel,.source-reference,.source-disclosure,.mcp-source-picker,#mcp-purpose-panel,#mcp-files-content")&&!card.querySelector("table")&&!card.closest(".overview-panels")&&!card.parentElement.closest(".panel");
  for(const [index,root]of [...document.querySelectorAll("main .panels:not(.overview-panels)")].entries())root.dataset.cardGroup||="panels-"+index;
  for(const panel of document.querySelectorAll("main .panel"))if(isChart(panel)&&!panel.parentElement.matches(".panels")){const root=node("div",null,"panels");root.dataset.cardGroup="charts-"+(panel.querySelector(".chart,.bar-chart,[id]")?.id||panel.dataset.cardKey||panel.id);panel.before(root);root.append(panel);}
  const grids=[...document.querySelectorAll("main .panels:not(.overview-panels)")],charts=root=>root?.matches(".panels")&&root.children.length&&[...root.children].every(isChart);
  for(const [index,root]of grids.entries())root.dataset.cardGroup||="panels-"+index;
  for(const root of grids){const previous=root.previousElementSibling;if(charts(root)&&charts(previous)){previous.append(...root.children);root.remove();}}
  for(const root of document.querySelectorAll("main .panels:not(.overview-panels)")){for(const [position,card]of [...root.children].entries())card.dataset.cardKey||=card.querySelector(".chart,.bar-chart,[id]")?.id||card.id||"card-"+position;dragCardGroup(root,root.dataset.cardGroup,card=>card.dataset.cardKey);}dragCardGroup($("mcp-source-cards"),"mcp-sources",card=>card.dataset.sourceKey);arrangeContentPanels();
}

function renderMcpTabs(servers){
  const choices=[["all",ui("全部來源")],...servers.map(source=>[source.server,source.server])];
  for(const server of Object.keys(data.mcp?.telemetry||{}))if(!choices.some(([key])=>key===server))choices.push([server,server]);
  if(!choices.some(([key])=>key===mcpSource)){mcpSource="all";preferences.mcpSource="all";}
  const nav=$("mcp-source-tabs"),signature=JSON.stringify(choices);
  if(nav.dataset.signature!==signature){nav.dataset.signature=signature;nav.replaceChildren(...choices.map(([key,label],index)=>{const tab=button(label,()=>selectMcpSource(key));tab.id=key==="all"?"mcp-source-all":"mcp-source-"+index;tab.dataset.mcpSource=key;tab.setAttribute("role","tab");tab.setAttribute("aria-controls","mcp-source-content");tab.addEventListener("keydown",event=>{if(event.altKey)return;if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?choices.length-1:(index+(event.key==="ArrowRight"?1:choices.length-1))%choices.length;selectMcpSource(choices[next][0]);nav.children[next].focus();});return tab;}));}
  dragSubTabs(nav,"mcp",tab=>tab.dataset.mcpSource);
  for(const tab of nav.children){const active=tab.dataset.mcpSource===mcpSource;tab.setAttribute("aria-selected",String(active));tab.tabIndex=active?0:-1;}
  const selected=[...nav.children].find(tab=>tab.dataset.mcpSource===mcpSource);$("mcp-source-content").setAttribute("aria-labelledby",selected?.id||"mcp-source-all");
  $("mcp-cards").hidden=false;
  const sourcePanel=$("mcp-catalog-panel");sourcePanel.hidden=mcpSource!=="all";
  $("filter-mcp-server").closest("label").hidden=mcpSource!=="all";
}
const mcpFields=editableLabels({written:"已寫入",truncated:"內容截斷",content_chars:"文字長度 (字元)",ocr_status:"OCR 結果",ocr_eligible_items:"待處理 OCR 項目",ocr_processed_items:"已處理 OCR 項目",ocr_omitted_items:"省略 OCR 項目",ocr_errors:"OCR 錯誤數",changed_files:"差異檔案數",only_in_source_files:"只在來源的檔案數",only_in_target_files:"只在目標的檔案數",evidence_runs:"驗證紀錄數",checks_passed:"檢查通過數",checks_failed:"檢查失敗數",checks_skipped:"略過檢查數",findings:"發現項目數",retries:"重試次數",http_attempts:"HTTP 嘗試次數",input_tokens:"輸入 Token",output_tokens:"輸出 Token"});
function metricLabel(key){const credits={credits:"Credits",credits_balance:"Credits 餘額",credits_used:"已使用 credits",credits_remaining:"剩餘 credits",credits_limit:"Credits 上限",credits_total:"Credits 總量",credits_has_credits:"Credits 可用",credits_unlimited:"Credits 無上限"};const base=key.startsWith("usage_")?key.slice(6):key;const common={plan_type:"方案",has_credits:"Credits 可用",unlimited:"無上限",calls:"操作次數",timestamp:"時間",time:"時間",operation:"操作",source:"來源",model:"Model",status:"結果",latency_ms:"耗時 (ms)",average_latency_ms:"平均耗時 (ms)",http_status:"HTTP status",request_bytes:"送出大小 (bytes)",response_bytes:"回傳大小 (bytes)",known_response_bytes:"已知回傳大小 (bytes)",request_body_bytes:"送出大小 (bytes)",since:"最早紀錄",attempts:"HTTP 嘗試",input_known_calls:"輸入 Token 有值",output_known_calls:"輸出 Token 有值",input_unknown_calls:"輸入 Token 缺值",output_unknown_calls:"輸出 Token 缺值",response_unknown_attempts:"回傳大小缺值",statuses:"結果分布"};return mcpFields[key]||mcpFields[base]||ui(credits[base]||common[base]||key.replaceAll("_"," "));}
function metricUnit(key){return /(?:^|_)credits(?:_|$)/.test(key)?"credits":/(?:^|_)tokens(?:_|$)/.test(key)?"tokens":key.endsWith("_ms")?"ms":key.endsWith("_bytes")?"bytes":key.endsWith("_percent")?"%":key.endsWith("_ratio")?ui("比例"):key.endsWith("_seconds")?"s":key.endsWith("_chars")?ui("字元"):key.endsWith("_pages")?ui("頁"):key.endsWith("_files")?ui("檔案"):/(?:_attempts|_retries|_hits|_misses)$/.test(key)?ui("次"):/(?:_items|_errors|_passed|_failed|_skipped)$/.test(key)?ui("項"):/(?:_count|_nodes)$/.test(key)?ui("個"):"--";}
function mcpBrief(event){return Object.entries(event.result||{}).filter(([key,value])=>key!=="status"&&["boolean","number"].includes(typeof value)).slice(0,8).map(([key,value])=>metricLabel(key)+": "+(typeof value==="boolean"?ui(value?"是":"否"):fmt(value))).join(" · ")||"--";}
function mcpMetrics(events,key){
  const values=new Map();for(const event of events)for(const [name,value]of Object.entries(event.result||{}))if(typeof value==="boolean"||typeof value==="number"&&Number.isFinite(value)){const id=event.server+":"+name,time=event.completed_at||event.timestamp;if(!values.has(id))values.set(id,{server:event.server,key:name,latest:value,timestamp:time,values:[]});const metric=values.get(id);if(typeof value==="number")metric.values.push(value);if((time||"")>(metric.timestamp||"")){metric.latest=value;metric.timestamp=time;}}
  if(!values.size)return null;const container=node("div",null,"mcp-metric-groups"),groups=new Map();for(const metric of [...values.values()].slice(0,160)){const group=/(?:credits?|balance|cost|price|billing)/.test(metric.key)?"Credits 與費用":/(?:tokens?|usage|quota|limit)/.test(metric.key)?"用量":/_(?:ms|seconds)$/.test(metric.key)?"耗時":/(?:saved|saving|compress|reduction)/.test(metric.key)?"節省與壓縮":/(?:browser|page|node|locator|request)/.test(metric.key)?"瀏覽器與頁面":/(?:checks|tests|passed|failed|skipped)/.test(metric.key)?"驗證結果":"執行結果";if(!groups.has(group))groups.set(group,[]);groups.get(group).push(metric);}
  for(const [name,metrics]of groups){
    const section=node("section"),timing=name==="耗時",columns=[ui("來源"),ui("指標"),ui("最新值"),ui("單位"),ui("最近更新時間")];
    if(timing)columns.push(ui("樣本數"),ui("平均值"),"P99",ui("Low 1% (P1)"));
    section.append(node("h4",ui(name)),table(columns,metrics.map(metric=>{const row=node("tr");cell(row,metric.server,"mono");cell(row,metricLabel(metric.key));cell(row,typeof metric.latest==="boolean"?ui(metric.latest?"是":"否"):fmt(metric.latest));cell(row,typeof metric.latest==="boolean"?"--":metricUnit(metric.key));cell(row,when(metric.timestamp));if(timing){const stats=statistics(metric.values);valueCell(row,stats?.size);valueCell(row,stats?.mean);valueCell(row,stats?.p99);valueCell(row,stats?.p1);}return row;}),ui(name),key+":"+name));container.append(section);
  }return container;
}
function appendThreadMcp(content,thread){
  const events=(data.mcp?.events||[]).filter(event=>event.thread_id===thread.thread_id),servers=[...new Set(events.map(event=>event.server).concat(thread.jev_calls?.length?["jev"]:[]))].sort();
  if(!servers.length)return;
  const container=node("div",null,"thread-mcp-content"),groups=[];content.append(node("h4",ui("MCP 檢查")),container);
  for(const server of servers){const panel=node("section"),records=events.filter(event=>event.server===server);groups.push([server,server,panel]);
    panel.append(metadataList([[ui("工具呼叫"),fmt(records.filter(event=>!event.nested).length)],[ui("exec 辨識"),fmt(records.filter(event=>event.nested).length)],[ui("已提供結果"),fmt(records.filter(event=>event.result?.status).length)]]));
    const durations=records.filter(event=>Number.isFinite(event.duration_ms)).map(event=>event.duration_ms);if(durations.length)panel.append(statisticLabels(statistics(durations),"ms",ui("有獨立耗時的工具呼叫")));
    const metrics=mcpMetrics(records,"detail:thread:"+server+":metrics");if(metrics)panel.append(node("h4",ui("來源提供的指標")),metrics);
    if(server==="jev")appendJevCalls(panel,thread);
    if(records.length)panel.append(node("h4",ui("操作紀錄")),table([ui("時間"),ui("工具"),ui("操作類型"),ui("結果"),ui("耗時 (ms)"),ui("操作摘要")],records.slice(0,100).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,event.tool,"mono");cell(row,actionLabels[event.action]||event.action);cell(row).append(tag(labels.status[event.result?.status]||event.result?.status||"--"));valueCell(row,event.duration_ms);cell(row,mcpBrief(event));return clickableRow(row,()=>openDetail({kind:"mcp",event}));}),ui("操作紀錄"),"detail:thread:"+server+":operations"));
  }
  modalTabs(container,groups,"thread-mcp",detail.mcpSource||groups[0][0],source=>detail.mcpSource=source);
}


function telemetryFields(record){return Object.entries(record||{}).filter(([,value])=>value==null||["number","boolean","string"].includes(typeof value));}
function telemetryValue(value){return value==null?"--":typeof value==="boolean"?ui(value?"是":"否"):typeof value==="number"?fmt(value):/^\d{4}-\d\d-\d\dT/.test(value)?when(value):labels.status[value]||value;}
function metricPriority(key){return /(?:credits?|balance|remaining|quota|cost)/.test(key)?100:/(?:tokens?|saved|saving|reduction|compress)/.test(key)?80:/(?:errors?|failed|latency|_ms$)/.test(key)?70:/(?:calls|operations|attempts|retries)/.test(key)?60:50;}
function telemetryPanel(server,report,section="all"){
  const panel=node("section",null,"panel source-telemetry");panel.dataset.cardKey="telemetry:"+server;
  const heading=node("h3"),caption=server+" · "+ui(section==="summary"?"活動摘要":"來源紀錄");heading.append(section==="summary"?button(caption,()=>openDetail({kind:"mcp-source",server}),"link"):document.createTextNode(caption));panel.append(heading);
  if(section!=="records"){
    panel.append(metadataList([[ui("狀態"),logHealthLabels[report.health]||report.health],[ui("開始紀錄"),when(report.enabled_at)]]));
    const metrics=telemetryFields(report.summary).filter(([,value])=>value!=null).map(([key,value])=>[key,telemetryValue(value)]);
    for(const [key,value]of Object.entries(report.summary||{}))if(value&&typeof value==="object"&&!Array.isArray(value))for(const [name,count]of telemetryFields(value))metrics.push([key+" · "+name,telemetryValue(count)]);
    const selected=section==="summary"?[...metrics].sort((a,b)=>metricPriority(b[0])-metricPriority(a[0])).slice(0,8):metrics;
    if(selected.length)panel.append(node("h4",ui("用量摘要")),metadataList(selected.map(([key,value])=>[metricLabel(key),value])));
    if(section==="summary")panel.append(button(ui("查看明細"),()=>openDetail({kind:"mcp-source",server}),"detail-open"));
  }
  if(section!=="summary"){
    const recent=report.recent||[];if(recent.length){const fields=[...new Set(recent.flatMap(record=>telemetryFields(record).map(([key])=>key)))];panel.append(node("h4",ui("操作紀錄")),table(fields.map(metricLabel),recent.map(record=>{const row=node("tr");for(const key of fields)cell(row,telemetryValue(record[key]));return clickableRow(row,()=>openDetail({kind:"source-record",server,record}));}),ui("來源紀錄"),"source:"+server+":recent"));}
    if(report.series?.length){const fields=[...new Set(report.series.flatMap(record=>telemetryFields(record).map(([key])=>key)))];panel.append(node("h4",ui("活動紀錄")),table(fields.map(metricLabel),report.series.map(record=>{const row=node("tr");for(const key of fields)cell(row,telemetryValue(record[key]));return row;}),ui("活動紀錄"),"source:"+server+":series"));}
  }return panel;
}
function connectionBadge(source){const connection=source.connection||{},names={response:"最近有回應",pending:"等待回應",error:"連線錯誤",paused:"檢查停用"};if(!names[connection.state])return document.createDocumentFragment();const badge=tag(ui(names[connection.state]),"connection-status connection-"+connection.state);bindHelp(badge,ui("依本機呼叫, 回傳與診斷紀錄判斷")+(connection.observed_at?" · "+when(connection.observed_at):""));return badge;}
function mcpObservationPanel(source,events){
  if(!events.length&&!source.calls&&!source.recognized&&!Object.keys(source.latest_metrics||{}).length)return null;
  const panel=node("section",null,"panel source-observation");panel.dataset.cardKey="mcp:"+source.server;
  const heading=node("h3");heading.append(button(source.server+" · "+ui("活動摘要"),()=>openDetail({kind:"mcp-source",server:source.server}),"link"));panel.append(heading);
  const direct=events.filter(event=>!event.nested),replies=direct.filter(event=>event.completed_at),recent=[...events].sort((a,b)=>(b.completed_at||b.timestamp||"").localeCompare(a.completed_at||a.timestamp||"")),summary=[[ui("工具呼叫"),fmt(source.calls)],[ui("exec 辨識"),fmt(source.recognized)],[ui("工具已回傳"),fmt(source.returned)],[ui("最近活動"),when(source.last_at)]];
  panel.append(metadataList(summary));
  const values=new Map(Object.entries(source.latest_metrics||{}).map(([key,item])=>[key,item.value]));for(const event of recent)for(const [key,value]of Object.entries(event.result||{}))if(!values.has(key)&&(typeof value==="boolean"||typeof value==="number"&&Number.isFinite(value)))values.set(key,value);
  const selected=[...values].sort((a,b)=>metricPriority(b[0])-metricPriority(a[0])).slice(0,8);
  if(selected.length)panel.append(node("h4",ui("主要指標")),metadataList(selected.map(([key,value])=>[metricLabel(key),telemetryValue(value)])));
  panel.append(button(ui("查看明細"),()=>openDetail({kind:"mcp-source",server:source.server}),"detail-open"));return panel;
}
function mcpTags(source,events){
  if(Array.isArray(source.tags)&&source.tags.length)return source.tags.map(label=>tag(label,"category"));
  const tools=[...new Set(events.filter(event=>event.server===source.server).map(event=>event.tool))],words=(source.server.replace(/([a-z])([A-Z])/g,"$1 $2")+" "+tools.join(" ")+" "+(source.description||"")+" "+events.filter(event=>event.server===source.server).flatMap(event=>Object.keys(event.result||{})).join(" ")).toLowerCase(),rules=[[/(?:saved_tokens|saving|compress|reduction)/,"Token 節省"],[/(?:docs?|documentation|wiki|manual|context)/,"文件查詢"],[/(?:search|find)/,"搜尋"],[/(?:sqlite|sql|database|query)/,"資料庫"],[/(?:ocr|extract|document|pdf)/,"文件處理"],[/(?:browser|navigate|screenshot)/,"瀏覽器"],[/(?:test|check|validate|verify)/,"驗證"],[/(?:^|[\s_.:-])(?:create|write|update|edit)(?:[\s_.:-]|$)/,"寫入"],[/(?:^|[\s_.:-])(?:read|get|list)(?:[\s_.:-]|$)/,"讀取"],[/(?:rank|evaluate|summari)/,"分析"]],tags=[];
  if(source.category!=="other")tags.push(ui(data.mcp.categories[source.category])||source.category);
  for(const [pattern,label]of rules)if(pattern.test(words)&&!tags.includes(ui(label)))tags.push(ui(label));
  return (tags.length?tags:[ui("其他 MCP")]).slice(0,2).map(label=>{const badge=tag(label,"category");bindHelp(badge,ui("依來源用途, 工具名稱與回傳指標分類"));return badge;});
}
function selectMcpSection(value){mcpSection=value;renderMcp();attachTables();}
function renderMcp(){
  const source=data.mcp||{servers:[],events:[],categories:{}},m={...source,servers:source.servers.filter(s=>s.server!=="web"),events:source.events.filter(e=>e.server!=="web")};
  renderMcpTabs(m.servers);const servers=m.servers.filter(s=>s.enabled&&(mcpSource==="all"||s.server===mcpSource)),selectedEvents=m.events.filter(e=>mcpSource==="all"||e.server===mcpSource);
  $("mcp-scope").textContent=(mcpSource==="all"?fmt(m.servers.length)+ui(" 個來源 · "):"")+fmt(selectedEvents.length)+ui(" 筆操作紀錄");
  cards("mcp-cards",[[ui("MCP 呼叫"),fmt(servers.reduce((n,s)=>n+s.calls,0)),mcpSource==="all"?ui("來源 ")+fmt(servers.length)+ui(" 個"):""],[ui("exec 辨識"),fmt(servers.reduce((n,s)=>n+s.recognized,0)),ui("程式碼中的呼叫位置")],[ui("已提供結果"),fmt(servers.reduce((n,s)=>n+s.known_status,0)),""],[ui("錯誤"),fmt(servers.reduce((n,s)=>n+s.errors,0)),ui("包含失敗與逾時")]]);
  $("mcp-panel").hidden=!m.servers.length;$("mcp-chart-panel").hidden=!m.servers.length;$("mcp-chart-panel").dataset.fullWidth="false";
  selectOptions("filter-mcp-category",[["all",ui("全部分類")],...[...new Set(m.servers.map(s=>s.category).concat(m.events.map(e=>e.category)))].map(key=>[key,ui(m.categories[key])||key])]);
  selectOptions("filter-mcp-server",[["all",ui("全部來源")],...m.servers.map(s=>[s.server,s.server])]);
  $("mcp-source-cards").replaceChildren(...m.servers.map(s=>{const el=button("",()=>navigateToMcpSource(s.server),"source-card"),tags=node("div",null,"tag-line");el.dataset.sourceKey=s.server;tags.append(...mcpTags(s,m.events),connectionBadge(s),tag(s.enabled?s.origin==="configured"?ui("已設定"):ui("有紀錄"):ui("已關閉"),s.enabled?s.origin==="configured"?"configured":"observed":"disabled"));el.append(node("div",s.server,"source-name mono"),node("p",s.description||ui("來源尚未提供說明"),s.description?"source-description":"source-description muted"),tags,node("p",fmt(s.calls)+ui(" 次呼叫 · ")+fmt(s.recognized)+ui(" 處 exec 辨識")),...s.known_status||Number.isFinite(s.average_ms)||Number.isFinite(s.p99_ms)?[node("p",[s.known_status?ui("錯誤 ")+fmt(s.errors):null,Number.isFinite(s.average_ms)?ui("平均 ")+fmt(s.average_ms)+" ms":null,Number.isFinite(s.p99_ms)?"P99 "+fmt(s.p99_ms)+" ms":null].filter(Boolean).join(" · "),"muted")]:[]);bindHelp(el.querySelector(".source-description"),s.description||ui("來源尚未提供說明"));return el;}));
  const selectedSource=m.servers.find(source=>source.server===mcpSource);$("mcp-purpose-panel").hidden=mcpSource==="all"||!selectedSource;$("mcp-purpose").textContent=selectedSource?.description||ui("來源尚未提供說明");$("mcp-purpose").hidden=false;$("mcp-purpose-panel").querySelector("h3").hidden=false;$("mcp-purpose-origin").replaceChildren(...selectedSource?[connectionBadge(selectedSource),...selectedSource.description_source?[node("span",ui("說明來源")+": "+(selectedSource.description_source==="config"?ui("Codex 設定"):selectedSource.description_source))]:[]]:[]);

  $("mcp-section-tabs").hidden=mcpSource==="all";const filesActive=mcpSource!=="all"&&mcpSection==="files";$("mcp-activity-content").hidden=filesActive;$("mcp-files-content").hidden=!filesActive;
  for(const [id,active]of [["mcp-activity-tab",!filesActive],["mcp-files-tab",filesActive]]){$(id).setAttribute("aria-selected",String(active));$(id).tabIndex=active?0:-1;}
  if(filesActive){let selected=mcpFileLists.get(mcpSource);if(!selected){selected={server:mcpSource};mcpFileLists.set(mcpSource,selected);}$("mcp-files").replaceChildren();appendMcpFiles($("mcp-files"),selected);if(!selected.mcpFiles&&!selected.mcpFilesError&&!selected.mcpFilesLoading)loadMcpFiles(selected);}

  if(mcpSource!=="all")$("filter-mcp-server").value=mcpSource;
  $("source-record-range").hidden=!Object.keys(data.mcp?.telemetry||{}).some(server=>mcpSource==="all"||server===mcpSource);$("mcp-telemetry").replaceChildren(...Object.entries(data.mcp?.telemetry||{}).filter(([server,report])=>(mcpSource==="all"||server===mcpSource)&&(report.recent?.length||report.series?.length)).map(([server,report])=>telemetryPanel(server,report,"records")));
  const metrics=mcpSource==="all"?null:mcpMetrics(selectedEvents,"mcp-metric-rows");$("mcp-metrics-panel").hidden=!metrics;$("mcp-metrics").replaceChildren(...metrics?[metrics]:[]);
  $("mcp-ranking-panel").hidden=mcpSource!=="all"||!m.servers.length;$("mcp-retained-panel").hidden=mcpSource!=="all";
  $("mcp-observations").replaceChildren(...Object.entries(data.mcp?.telemetry||{}).filter(([server])=>mcpSource==="all"||server===mcpSource).map(([server,report])=>telemetryPanel(server,report,"summary")),...(mcpSource==="all"?m.servers:[]).filter(source=>!data.mcp?.telemetry?.[source.server]).map(source=>mcpObservationPanel(source,m.events.filter(event=>event.server===source.server))).filter(Boolean));
  const filtered=m.events.filter(e=>(mcpSource==="all"||e.server===mcpSource)&&($("filter-mcp-category").value==="all"||e.category===$("filter-mcp-category").value)&&($("filter-mcp-server").value==="all"||e.server===$("filter-mcp-server").value)&&($("filter-mcp-result").value==="all"||$("filter-mcp-result").value==="error"&&["error","failed","timeout","cancelled","canceled","missing_dependencies"].includes(e.result?.status)||$("filter-mcp-result").value==="known"&&e.result?.status||$("filter-mcp-result").value==="unknown"&&!e.result?.status));
  $("mcp-rows").dataset.recordCount=filtered.length;
  const rows=sortRecords("mcp-rows",filtered,[e=>e.timestamp,e=>e.thread_name||e.thread_id,eventProject,e=>e.server,e=>data.mcp.categories[e.category]||e.category,e=>e.tool,e=>actionLabels[e.action],e=>e.nested?ui("exec 辨識"):ui("工具呼叫"),e=>e.result?.status,e=>e.duration_ms,mcpBrief],e=>e.timestamp),size=tableSize("mcp-rows"),pages=Math.max(1,Math.ceil(rows.length/size));mcpPage=Math.max(1,Math.min(mcpPage,pages));
  replaceRows("mcp-rows",...rows.slice((mcpPage-1)*size,mcpPage*size).map(e=>{const row=node("tr");cell(row,when(e.timestamp));eventThreadCell(row,e);projectCell(row,e);cell(row,e.server,"mono");cell(row).append(tag(ui(m.categories[e.category])||e.category));cell(row,e.tool,"operation-cell mono");cell(row).append(tag(actionLabels[e.action]||e.action));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));cell(row).append(tag(labels.status[e.result?.status]||e.result?.status||"--",e.result?.status||""));valueCell(row,e.duration_ms);cell(row,mcpBrief(e));return clickableRow(row,()=>openDetail({kind:"mcp",event:e}));}));
  $("mcp-empty").textContent=rows.length?"":ui("沒有符合條件的操作");$("mcp-page-summary").textContent=ui("第 ")+mcpPage+" / "+pages+ui(" 頁 · ")+fmt(rows.length)+ui(" 筆");$("mcp-prev").disabled=mcpPage===1;$("mcp-next").disabled=mcpPage===pages;syncPageInput($("mcp-page-number"),mcpPage,pages);
  attachTables();

}
function renderWeb(){
  const events=(data.mcp?.events||[]).filter(e=>e.server==="web"),refs=references(events),urls=new Set(refs.map(e=>e.url));
  $("web-scope").textContent=ui("查看網路工具操作與參考頁面");
  cards("web-cards",[[ui("網路工具呼叫"),fmt(events.filter(e=>!e.nested).length),ui("exec 辨識 ")+fmt(events.filter(e=>e.nested).length)+ui(" 處")],[ui("參考網址"),fmt(urls.size),""],[ui("涉及對話"),fmt(new Set(events.map(e=>e.thread_id)).size),""],[ui("已提供結果"),fmt(events.filter(e=>e.result?.status).length),ui("錯誤 ")+fmt(events.filter(e=>["error","failed","timeout"].includes(e.result?.status)).length)+ui(" 次")]]);
  replaceRows("web-event-rows",...events.map(e=>{const row=node("tr"),links=references([e]),tool=node("span",null,"tool-identity");tool.append(node("span","web.run","tool-name mono"),tag("Web"));cell(row,when(e.timestamp));eventThreadCell(row,e);projectCell(row,e);cell(row).append(tool);cell(row).append(tag(labels.status[e.result?.status]||e.result?.status||"--"));valueCell(row,links.length);cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"mcp",event:e}));}));
  $("web-event-empty").textContent=events.length?"":ui("無");
  const ranked=new Map(),sites=new Map();for(const ref of refs){const host=new URL(ref.url).hostname;for(const [map,key]of [[ranked,ref.url],[sites,host]]){if(!map.has(key))map.set(key,[]);map.get(key).push(ref);}}
  const rankRows=map=>[...map].map(([key,items])=>{const row=node("tr");cell(row,key,"path-cell mono");valueCell(row,items.length);valueCell(row,new Set(items.map(item=>item.thread_id)).size);valueCell(row,new Set(items.map(item=>item.url)).size);return clickableRow(row,()=>openDetail({kind:"web-reference",key,items}));});
  replaceRows("web-reference-rows",...rankRows(ranked));replaceRows("web-domain-rows",...rankRows(sites));

}
function fileLocation(event){return event.workdir&&!/^(?:[A-Za-z]:[\\/]|[\\/])/.test(event.path)?event.workdir.replace(/[\\/]$/u,"")+(event.workdir.includes("\\")?"\\":"/")+event.path:event.path;}
function fileProjectKey(event){const thread=threadIndex.get(event.thread_id)||{};return thread.project_id?"project:"+thread.project_id:thread.project_name?"name:"+thread.project_name:thread.project_scope==="none"?"none":"unknown";}
function filteredFiles(){const query=$("file-search").value.trim().toLowerCase(),operation=$("filter-file-operation").value,method=$("filter-file-method").value,project=$("filter-file-project").value,tool=$("filter-file-tool").value;return (data.codex.file_activity?.events||[]).filter(e=>(!query||fileLocation(e).toLowerCase().includes(query))&&(operation==="all"||operation===e.operation)&&(method==="all"||e.nested===(method==="nested"))&&(project==="all"||fileProjectKey(e)===project)&&(tool==="all"||e.tool===tool));}
function fileRows(events,includeThread=true){return events.map(e=>{const row=node("tr");cell(row,when(e.timestamp));if(includeThread){eventThreadCell(row,e);projectCell(row,e);}cell(row).append(tag(fileLabels[e.operation]||e.operation));cell(row,fileLocation(e),"mono path-cell");const tool=cell(row,null,"operation-cell");tool.append(toolButton(e.tool||"apply_patch",()=>openDetail({kind:e.nested?"nested-tool":"tool",tool:e.tool||"apply_patch",ids:[e.thread_id]})));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));cell(row,when(e.completed_at));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"file",event:e}));});}
function renderFiles(){
  if($("tab-files").getAttribute("aria-selected")==="true"&&data.codex?.activity_scope?.window===activeSourceWindow()&&!fileSnapshots.has(activeSourceWindow())&&!fileSnapshotRequests.has(activeSourceWindow())&&!fileSummaryBusy)loadFileSizes();
  const records=data.codex.file_activity||{events:[],total:0},projects=new Map();for(const event of records.events){const key=fileProjectKey(event);if(!["none","unknown"].includes(key))projects.set(key,eventProject(event)||key);}
  selectOptions("filter-file-project",[["all",ui("全部專案")],["none",ui("無專案")],["unknown",ui("未知")],...[...projects].sort((a,b)=>a[1].localeCompare(b[1]))]);selectOptions("filter-file-tool",[["all",ui("全部工具")],...[...new Set(records.events.map(event=>event.tool).filter(Boolean))].sort().map(tool=>[tool,tool])]);
  for(const [id,value]of [["filter-file-project",pendingFileProject],["filter-file-tool",pendingFileTool]])if(value&&[...$(id).options].some(option=>option.value===value))$(id).value=value;pendingFileProject=pendingFileTool=null;
  const events=filteredFiles();$("files-scope").textContent=ui("從工具參數辨識檔案讀寫操作")+" · "+fmt(records.total)+ui(" 筆操作");
  cards("file-cards",[[ui("符合篩選的紀錄"),fmt(events.length),events.length!==records.total?ui("全部 ")+fmt(records.total)+ui(" 筆"):""],[ui("讀取"),fmt(events.filter(e=>e.operation==="read").length),ui("有指定檔案位置")],[ui("寫入與變更"),fmt(events.filter(e=>e.operation!=="read").length),ui("新增 / 修改 / 刪除 / 移動")],[ui("不同檔案位置"),fmt(new Set(events.map(fileLocation)).size),""]]);
  replaceRows("file-rows",...fileRows(events));$("file-empty").textContent=events.length?"":ui("無");
  const files=new Map(),folders=new Map();for(const event of events){const path=fileLocation(event),folder=path.replace(/[\\/][^\\/]*$/,"");for(const [map,key]of [[files,path],[folders,folder===path?event.workdir||"--":folder]]){if(!map.has(key))map.set(key,[]);map.get(key).push(event);}}
  const rows=map=>[...map].map(([path,items])=>{const row=node("tr");cell(row,path,"mono path-cell");valueCell(row,items.length);valueCell(row,items.filter(item=>item.operation==="read").length);valueCell(row,items.filter(item=>item.operation!=="read").length);for(const key of ["read_bytes","write_bytes","submitted_utf8_bytes"]){const known=items.map(item=>item[key]).filter(Number.isFinite);valueCell(row,known.length?known.reduce((a,b)=>a+b,0):null);}return clickableRow(row,()=>openDetail({kind:"file-group",path,items}));});
  replaceRows("file-rank-rows",...rows(files));replaceRows("folder-rank-rows",...rows(folders));
  const snapshot=fileSnapshots.get(activeSourceWindow()),paths=new Set(events.map(fileLocation));replaceRows("file-size-rows",...(snapshot?.files||[]).filter(file=>paths.has(fileLocation(file))).map(file=>{const row=node("tr");cell(row,fileLocation(file),"mono path-cell");valueCell(row,file.bytes);cell(row,when(file.modified_at));cell(row,logHealthLabels[file.health]||file.health);return clickableRow(row,()=>openDetail({kind:"file",event:file,fileMetadata:file}));}));
  $("file-size-note").textContent=snapshot?ui("檢查時間")+": "+when(snapshot.checked_at)+" · "+fmt(snapshot.files.length)+" / "+fmt(snapshot.total):ui("檢查目前檔案大小後顯示排行");

}
function renderGit(){
  const git=data.codex.git||{events:[],operations:{},total:0},select=$("filter-git"),selected=select.value;
  select.replaceChildren(...[["all",ui("全部操作")],...sorted(git.operations).map(([key,count])=>[key,key+" ("+fmt(count)+")"])].map(([value,label])=>{const el=node("option",label);el.value=value;return el;}));
  if([...select.options].some(option=>option.value===selected))select.value=selected;
  if(pendingGit){if([...select.options].some(option=>option.value===pendingGit))select.value=pendingGit;pendingGit=null;}
  const events=git.events.filter(event=>select.value==="all"||event.operation===select.value);
  $("git-scope").textContent=ui("Git 檢查已關閉");$("git-scope").hidden=data.settings?.observations.git!==false;
  cards("git-cards",[[ui("Git 操作"),fmt(git.total),ui("操作種類 ")+fmt(Object.keys(git.operations).length)],[ui("涉及對話"),fmt(new Set(git.events.map(e=>e.thread_id)).size),""],[ui("工具已回傳"),fmt(git.events.filter(e=>e.completed_at).length),""],[ui("工作目錄"),fmt(new Set(git.events.map(e=>e.repository).filter(Boolean)).size),ui("已辨識的目錄名稱")]]);
  replaceRows("git-rows",...events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.operation,()=>openDetail({kind:"operation",event}),"link mono"));cell(row,event.repository||ui("未知"));cell(row,when(event.completed_at));cell(row,fmt(event.duration_ms));return row;}));
  $("git-empty").textContent=events.length?"":ui("無");
}
function renderWorkflow(){
  const skills=data.codex.skills||{events:[],counts:{}},checks=data.codex.checks||[];
  cards("skill-cards",[[ui("技能數"),fmt(Object.keys(skills.counts).length),""],[ui("讀取次數"),fmt(Object.values(skills.counts).reduce((sum,count)=>sum+count,0)),""],[ui("涉及對話"),fmt(new Set(skills.events.map(event=>event.thread_id)).size),""],[ui("最近讀取"),when(skills.events.map(event=>event.timestamp).filter(Boolean).sort().at(-1)),""]]);
  replaceRows("skill-rows",...skills.events.map(event=>{const row=node("tr"),thread=(data.codex.threads||[]).find(item=>item.thread_id===event.thread_id),tool=event.tool||(thread?.tool_events||[]).find(item=>item.call_id===event.call_id)?.tool;cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.skill,()=>openDetail({kind:"skill",skill:event.skill,event}),"link"));projectCell(row,event);cell(row,tool||"--","mono");cell(row).append(tag(tool?ui(tool==="exec"?"exec 辨識":"工具參數辨識"):"--"));cell(row,when(event.completed_at));valueCell(row,event.duration_ms);cell(row,event.call_id||"--","mono");return clickableRow(row,()=>openDetail({kind:"skill",skill:event.skill,event}));}));
  $("skill-empty").textContent=skills.events.length?"":ui("無");
  cards("check-cards",[[ui("驗證操作"),fmt(checks.length),""],[ui("操作種類"),fmt(new Set(checks.map(e=>e.operation)).size),ui("test / build / lint")],[ui("涉及對話"),fmt(new Set(checks.map(e=>e.thread_id)).size),""],[ui("工具已回傳"),fmt(checks.filter(e=>e.completed_at).length),""]]);
  replaceRows("check-rows",...checks.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.operation,()=>openDetail({kind:"operation",event}),"link mono"));cell(row,when(event.completed_at));cell(row,fmt(event.duration_ms));return row;}));
  $("check-empty").textContent=checks.length?"":ui("無");
}

let uptimeSample=null;
function renderUptime(){if(!uptimeSample||document.hidden)return;const elapsed=Math.max(0,Math.floor(uptimeSample.seconds+(performance.now()-uptimeSample.received)/1000)),days=Math.floor(elapsed/86400),clock=[Math.floor(elapsed/3600)%24,Math.floor(elapsed/60)%60,elapsed%60].map(value=>String(value).padStart(2,"0")).join(":");$("program-uptime").textContent=ui("已執行 ")+(days?fmt(days)+ui(" 天")+" ":"")+clock;}
setInterval(renderUptime,1000);

function render(next){
  data=next;threadIndex=new Map((data.codex.threads||[]).map(thread=>[thread.thread_id,thread]));const c=data.codex,j=data.jev,threads=c.threads||[];
  $("program-version").textContent=data.monitor?.version?"v"+data.monitor.version:"--";document.documentElement.dataset.revision=data.revision||"";$("program-started").textContent=ui("啟動 ")+when(data.started_at);$("program-started").dateTime=data.started_at||"";$("program-started").title=ui("此監測程式的啟動時間");$("program-uptime").title=ui("此監測程式持續執行的時間");if(Number.isFinite(data.monitor?.uptime_seconds))uptimeSample={seconds:data.monitor.uptime_seconds,received:performance.now()};renderUptime();
  renderConnectionStatus();
  $("live").textContent=ui(data.monitor?.health==="error"?data.updated_at?"資料整理失敗, 顯示上次結果":"資料整理失敗, 等待重試":data.updated_at?"已連線":"正在整理資料");
  const duration=threads.filter(t=>t.task_duration_ms!=null).reduce((n,t)=>n+t.task_duration_ms,0);
  cards("overview-cards",[[ui("對話"),fmt(threads.length),ui("執行中 ")+fmt(threads.filter(t=>t.status==="running").length)+ui(" 個")],[ui("工具呼叫"),fmt(c.observed_tool_calls||0),ui("工具種類 ")+fmt(Object.keys(c.tools||{}).length)],[ui("工作累計時間"),fmt(Math.round(duration/60000))+ui(" 分鐘"),ui("已取得時間的對話 ")+fmt(threads.filter(t=>t.task_duration_ms!=null).length)+ui(" 個")],[ui("資料來源"),fmt(data.mcp?.servers?.length||0),ui("檔案修改 ")+fmt(threads.reduce((n,t)=>n+(t.file_changes?.length||0),0))+ui(" 筆")]]);
  for(const [id,available]of [["mcp",data.availability?.jev||data.mcp?.servers?.some(s=>s.server!=="web")],["web",data.mcp?.servers?.some(s=>s.server==="web")]]){$("tab-"+id).hidden=!available;if(!available&&$("tab-"+id).getAttribute("aria-selected")==="true")switchTab("overview");}

  renderMonitor();renderErrors();renderLogSummary();renderSQLite();renderSources();if(!$("view-errors").hidden&&!$("view-logs").hidden||overviewNeeds("view-logs"))loadLogs();
  updateProjects();renderCodex();renderUsage();renderDots();renderGit();renderWorkflow();renderTools();renderMcp();renderWeb();renderFiles();renderCharts();renderHighlights();
  $("updated").textContent=data.updated_at?ui("更新 ")+new Date(data.updated_at).toLocaleTimeString(locale,{hour12:false}):ui("尚未更新");$("updated").title=when(data.updated_at);$("updated").dateTime=data.updated_at||"";
  if(!settingsBusy&&data.settings){if(document.activeElement!==$("refresh-interval"))$("refresh-interval").value=data.settings.interval;syncIdleSettings();syncRefreshState(data.activity);schedule(data.settings.interval);}
  if(detail&&!["jev","mcp-file"].includes(detail.kind))renderDetail();attachTables();
}
const errorCategories=editableLabels({conversation:"對話",mcp:"MCP",tool:"工具",codex:"Codex",monitor:"監測程式"});
const errorSources=editableLabels({codex_desktop:"Codex App",codex_core:"Codex Core",session:"對話紀錄",tool_result:"工具回傳",jev_telemetry:"Jev 紀錄",monitor:"監測程式"});
const errorReasons=editableLabels({message_submit_failed:"對話送出失敗",stream_interrupted:"回應連線中斷 / 重試",mcp_diagnostic:"MCP 診斷事件",tool_diagnostic:"工具診斷事件",codex_diagnostic:"Codex 診斷事件",process_exit:"命令回傳非零代碼",file_not_found:"找不到檔案",http_error:"HTTP 請求失敗",network_unavailable:"網路無法連線",invalid_response:"回傳格式無法辨識",response_too_large:"回傳內容過大",tool_error:"工具回報錯誤",refresh_failed:"資料整理失敗",http_response_error:"HTTP 回應錯誤",error:"對話回報錯誤",turn_failed:"對話執行失敗",turn_aborted:"對話中止"});
function errorReason(e){return errorReasons[e.reason||e.code]||labels.status[e.code]||ui("來源回報錯誤");}
function errorRows(events){return events.map(e=>{const row=node("tr");cell(row,when(e.timestamp));cell(row).append(severityTag(e.severity));cell(row,errorCategories[e.category]||e.category);if(e.thread_id)eventThreadCell(row,e);else cell(row,"--");projectCell(row,e);cell(row,errorSources[e.source]||e.source);if(e.tool)cell(row).append(toolButton(e.server?"mcp__"+e.server+"__"+e.tool:e.tool,()=>openDetail({kind:"tool",tool:e.server?"mcp__"+e.server+"__"+e.tool:e.tool})));else cell(row,"--");cell(row,errorReason(e));cell(row,e.code||"--","mono");return clickableRow(row,()=>openDetail({kind:"error",event:e}));});}
function renderErrors(){
  const errors=data.errors||{},events=errors.events||[],failed=events.filter(e=>e.severity==="error"),warnings=events.filter(e=>e.severity==="warning"),recent=failed.filter(e=>{const time=new Date(e.timestamp).getTime();return time>=Date.now()-86400000&&time<=Date.now()+60000;});
  $("errors-quick").textContent=ui("錯誤 ")+fmt(recent.length);$("errors-quick").classList.toggle("has-errors",!!recent.length);$("errors-quick").title=ui("最近 24 小時 · ")+fmt(recent.length)+ui(" 筆錯誤")+(failed[0]?ui(" · 最新 ")+when(failed[0].timestamp):"");
  const health=errors.diagnostics?.health||{},names={ok:ui("正常"),missing:ui("未找到"),unsupported:ui("格式未支援"),unavailable:ui("無法讀取"),partly_unavailable:ui("部分無法讀取"),disabled:ui("未啟用"),waiting:ui("等待中")};
  $("error-scope").textContent=(errors.enabled?ui("診斷紀錄: App ")+(names[health.desktop]||ui("未知"))+" · Core "+(names[health.core]||ui("未知")):ui("對話與工具錯誤檢查已停用"))+ui(" · 保留最近 ")+fmt(errors.limit||1000)+ui(" 筆事件")+ui(" · 保存最近 24 小時錯誤摘要")+(errors.history?.health==="unavailable"?ui(" · 摘要保存失敗, 目前顯示記憶體資料"):"");
  cards("error-cards",[[ui("最近 24 小時錯誤"),fmt(recent.length),""],[ui("目前載入的錯誤"),fmt(failed.length),""],[ui("警告"),fmt(warnings.length),ui("包含連線重試與診斷警告")],[ui("涉及對話"),fmt(new Set(events.map(e=>e.thread_id).filter(Boolean)).size),ui("可點整列查看明細")]]);
  replaceRows("error-rows",...errorRows(events));$("error-empty").textContent=events.length?"":ui("無");
}
$("errors-quick").addEventListener("click",()=>{diagnosticSource="errors";switchTab("errors");const view=tableViews.get("error-rows");if(view){tableStates["error-rows"]={...tableStates["error-rows"],page:1,filters:{[ui("等級")]:"error"}};renderTableFilters("error-rows");paginateTable("error-rows");persistTables();}scrollToSection("view-errors");});
const monitorEventLabels=editableLabels({started:"程式啟動",settings_applied:"設定已套用",recording_enabled:"Jev 紀錄已啟用",recording_disabled:"Jev 紀錄已停用",refresh_failed:"資料整理失敗",recovered:"資料整理恢復",http_response_error:"HTTP 回應錯誤"});
const logHealthLabels=editableLabels({ok:"正常",ready:"就緒",enabled:"已啟用",missing:"未找到",config_missing:"尚未設定",waiting:"等待讀取",disabled:"已停用",paused:"已暫停",memory:"記憶體保存",unsupported:"格式未支援",oversized:"超過讀取上限",unavailable:"無法存取",partly_unavailable:"部分無法存取",invalid_config:"設定無效",error:"讀取失敗"});
const logLevelLabels={error:"error",warning:"warn",warn:"warn",info:"info",debug:"debug",trace:"trace",fatal:"fatal",critical:"critical"};
function logSource(source){return errorSources[source]||source;}
function severityTag(level){const name=String(level||"unknown").toLowerCase(),kind=["error","fatal","critical"].includes(name)?"error":["warning","warn"].includes(name)?"warning":["info","debug","trace"].includes(name)?name:"unknown";return tag(logLevelLabels[name]||level||"--","log-level level-"+kind);}
function logMessage(event){return monitorEventLabels[event.kind]||(event.reason==="codex_diagnostic"&&!['error','warning'].includes(event.severity)?ui("一般診斷事件"):event.reason?errorReason(event):event.code||ui("來源事件"));}
function renderLogSources(sources){
  replaceRows("log-source-rows",...sources.map(source=>{const row=node("tr");cell(row,logSource(source.source));cell(row).append(tag(logHealthLabels[source.health]||source.health));cell(row,when(source.checked_at));valueCell(row,source.file_count??source.files?.length);valueCell(row,source.read_bytes??source.bytes);valueCell(row,source.read_lines);valueCell(row,source.unsupported_lines);valueCell(row,source.oversized_lines);cell(row,source.error_type||"--","mono");return clickableRow(row,()=>openDetail({kind:"log-source",source}));}));
}
const sqlLabels=editableLabels({open:"連線",close:"關閉連線",read:"查詢",write:"資料寫入",schema:"結構變更",pragma:"資料庫設定",maintenance:"維護",transaction:"交易",attach:"附加資料庫",operation:"其他操作",diagnostic:"資料庫診斷"});
const sqlResults=editableLabels({returned:"工具已回傳",failed:"工具失敗",unknown:"尚無回傳",log_recorded:"來源已記錄",log_error:"來源回報錯誤"});
function sqlMethod(event){return ui(event.recognition==="mcp_call"?"工具呼叫":event.recognition==="diagnostic_log"?"SQL 診斷紀錄":"程式碼辨識");}
function sqlLocation(event){return !event.database?null:event.database===":memory:"||event.database.startsWith("file:")?event.database:fileLocation({path:event.database,workdir:event.workdir});}
function renderSQLite(){
  const sql=data.codex.sqlite||{},events=sql.events||[];
  $("sqlite-scope").textContent=data.settings.observations.sqlite===false?ui("SQL 操作檢查已停用"):ui("從工具呼叫與 SQL 診斷辨識操作類型")+" · "+fmt(events.length)+ui(" 筆操作");
  cards("sqlite-cards",[[ui("SQL 操作"),fmt(events.length),ui("最近保留的操作紀錄")],[ui("資料庫"),fmt(new Set(events.map(sqlLocation).filter(Boolean)).size),ui("可確認的檔案與記憶體資料庫")],[ui("SQL 診斷紀錄"),fmt(events.filter(e=>e.recognition==="diagnostic_log").length),ui("來源實際提供的 SQL metadata")],[ui("工具失敗"),fmt(new Set(events.filter(e=>e.result==="failed").map(e=>e.thread_id+":"+e.call_id)).size),ui("整次工具呼叫的回傳狀態")]]);
  replaceRows("sqlite-rows",...events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,event.statement,"mono");cell(row,sqlLabels[event.operation]||event.operation);cell(row,event.engine);cell(row,sqlLocation(event)||"--","path-cell mono");cell(row).append(tag(sqlResults[event.result]||event.result));if(event.thread_id)eventThreadCell(row,event);else cell(row,"--");projectCell(row,event);cell(row,sqlMethod(event));cell(row,event.tool||"--","mono");valueCell(row,event.container_duration_ms);cell(row,event.call_id||"--","mono");cell(row,event.source?logSource(event.source):ui("對話紀錄"));valueCell(row,event.duration_ms);valueCell(row,event.rows_affected);valueCell(row,event.rows_returned);return clickableRow(row,()=>openDetail({kind:"sqlite",event}));}));
  $("sqlite-empty").textContent=events.length?"":ui("無");
  const measured=events.filter(event=>Number.isFinite(event.duration_ms));$("sqlite-measured").replaceChildren(node("h4",ui("SQL 耗時統計")),measured.length?statisticLabels(statistics(measured.map(event=>event.duration_ms)),"ms",ui("僅統計來源有提供耗時的 SQL 紀錄")):node("p","--","muted"));
}
const sourceScopes=editableLabels({retained_activity_metadata:"SQL, 網路與 MCP 活動摘要, 依設定的保留天數循環保存",
  recent_tail_and_incremental_metadata:"先讀取各 session 的近期尾端, 再增量讀取新增紀錄. 工作起訖與錯誤紀錄依讀取預算分批回補, 表格與圖表使用已載入的資料",
  titles_classification_and_local_thread_metadata:"從實際可讀取的 catalog, session index 與 app state 補充對話名稱, 專案, 模型與執行設定. 下方列出各檔案的讀取結果與選取欄位",
  recent_metadata_and_24h_error_backfill:"增量讀取診斷檔案與資料庫, 選取 Log, 錯誤與 SQL 操作的 metadata. 各讀取器的預算, 已讀取數量與回補進度分別列出, SQL 文字在點開明細時讀取",
  completed_operations_in_local_database:"唯讀查詢來源已寫入的完成操作. 摘要依目前時間範圍計算, 明細與趨勢受各自的讀取上限限制, 用量與 Credits 沿用來源回報的欄位",
  runtime_samples_and_bounded_event_metadata:"收集此監測程式的效能樣本與執行事件. 效能樣本存於記憶體, 程式事件與錯誤摘要依容量循環保存, 下方列出目前保留量與上限",
  confirmed_lifecycle_and_skill_checkpoints:"保存已確認的工作起訖與技能讀取識別資料. 更新與重啟時沿用 checkpoint 並去重, 超過保存上限後移除較舊資料",
  configured_sources_and_purposes:"從目前 MCP 設定取得啟用的來源名稱與用途, 再合併已取得的呼叫來源. 只選取顯示需要的欄位, 不顯示啟動參數與憑證",
  reported_telemetry:"來源已載入的檢查資料. 下方列出可取得的欄位, 讀取結果與上限"
});
const sourceLimitLabels=editableLabels({retention_days:"保留天數",sql_limit:"SQL 紀錄上限",web_limit:"網路紀錄上限",mcp_limit:"MCP 紀錄上限",sql_records:"SQL 保留筆數",web_records:"網路保留筆數",mcp_records:"MCP 保留筆數",file_limit:"追蹤檔案上限",sql_event_limit:"SQL 明細上限",git_event_limit:"Git 明細上限",check_event_limit:"驗證明細上限",file_event_limit:"檔案明細上限",skill_event_limit:"技能明細上限",tool_limit:"每個 MCP 的工具摘要上限",file_count:"已追蹤檔案",read_limit:"每次讀取上限",tail_bytes:"首次尾端讀取 (bytes)",scan_seconds:"檔案盤點間隔 (秒)",call_limit:"工具紀錄上限",buffer_limit:"片段暫存上限 (bytes)",history_pending_files:"待回補檔案",listed_file_limit:"列出來源檔案上限",read_bytes:"累計讀取量 (bytes)",unsupported_lines:"無法解析的紀錄 (行)",retained:"保留紀錄",skills:"技能紀錄",entry_limit:"Checkpoint 紀錄上限",skill_limit:"技能紀錄上限",byte_limit:"保存上限 (bytes)",row_limit:"讀取列數上限",rows_read:"已讀取列數",selected_threads:"查詢對話數",matched_threads:"符合的對話數",event_limit:"事件上限",configured_sources:"設定的來源數",loaded_sources:"已載入來源數",loaded_records:"已載入明細",series_points:"趨勢資料點",record_limit:"明細讀取上限",series_limit:"趨勢資料點上限",history_limit:"效能樣本上限",retained_samples:"保留效能樣本",retained_events:"保留執行事件",error_history_limit:"錯誤摘要上限",error_history_byte_limit:"錯誤摘要容量 (bytes)",history_hours:"錯誤回補範圍 (小時)",backfill_pending:"待回補資料",read_lines:"已讀取 (行)",parsed_lines:"已辨識 (行)",oversized_lines:"超過長度上限 (行)"});
function sourceMetrics(values){return Object.entries(values||{}).filter(([,value])=>value!=null&&["number","boolean"].includes(typeof value)).map(([key,value])=>[sourceLimitLabels[key]||metricLabel(key),typeof value==="boolean"?ui(value?"是":"否"):fmt(value)+(key==="read_limit"?" "+(values.read_unit||"bytes"):"")]);}
function sourceFeature(tab){const id=tab.id.slice(5);return id==="codex"?(conversationSource==="dots"?"dots":"codex"):id==="errors"?diagnosticSource:id==="workflow"?activitySource:id;}
const sourceWindowLabels=editableLabels({"1h":"最近 1 小時","24h":"最近 24 小時","7d":"最近 7 天",all:"全部本機紀錄"});
const sourceReaderNames=editableLabels({"Checkpoint load":"Checkpoint 載入","Checkpoint save":"Checkpoint 保存"});
function sourceReader(reader){
  const group=node("section",null,"source-reader"),head=node("h4",sourceReaderNames[reader.name]||reader.name||logSource(reader.source)||reader.source);
  if(reader.health)head.append(document.createTextNode(" "),tag(logHealthLabels[reader.health]||reader.health));
  group.append(head,metadataList(sourceMetrics(reader)));
  if(reader.fields?.length){const fields=node("div",null,"source-fields");fields.append(...reader.fields.map(field=>tag(field)));group.append(fields);}
  for(const query of reader.queries||[])group.append(sourceReader(query));
  return group;
}
function sourceDetails(key,source,opened){
  const item=node("details"),summary=node("summary",source.name);item.dataset.source=key;item.open=opened.has(key);
  if(source.health)summary.append(tag(logHealthLabels[source.health]||source.health));item.append(summary,node("p",sourceScopes[source.scope]||source.scope||"--","source-description"));
  const modes=editableLabels({read_only:"唯讀",bounded_metadata_cache:"循環保存"});item.append(metadataList([[ui("讀取方式"),modes[source.mode]||source.mode],...(source.display_window?[[ui("顯示時間範圍"),sourceWindowLabels[source.display_window]||source.display_window]]:[]),...(source.window?[[ui("來源保留範圍"),sourceWindowLabels[source.window]||source.window]]:[]),...sourceMetrics(source.limits)]));
  if(source.locations?.length){item.append(node("h4",ui("來源位置")));for(const location of source.locations)item.append(node("p",location,"mono source-location"));}
  if(source.observations?.length){const fields=node("div",null,"source-fields");fields.append(...source.observations.map(field=>tag(observationLabels[field]||metricLabel(field))));item.append(node("h4",ui("檢查項目")),fields);}
  if(source.fields?.length){const fields=node("div",null,"source-fields");fields.append(...source.fields.map(field=>tag(field)));item.append(node("h4",ui("選取欄位")),fields);}
  for(const reader of source.readers||[])item.append(sourceReader(reader));
  return item;
}
function renderSources(){
  for(const footer of document.querySelectorAll("main .source-reference[data-source-tab]")){const tab=$("view-"+footer.dataset.sourceTab);if(tab&&footer.parentElement!==tab){const old=footer.parentElement;tab.append(footer);if(old.matches(".panels")&&!old.childElementCount)old.remove();}}
  for(const tab of document.querySelectorAll('main>section[role="tabpanel"]')){
    const id=tab.id.slice(5),feature=sourceFeature(tab),sources=Object.entries(data.sources||{}).filter(([key,source])=>(id==="overview"||source.features?.includes(feature))&&(id!=="mcp"||mcpSource==="all"||!key.startsWith("telemetry:")||key==="telemetry:"+mcpSource));let footer=tab.querySelector(":scope>.source-reference");
    if(!footer){footer=node("details",null,"panel source-reference");footer.dataset.sourceTab=id;footer.open=preferences.sectionCollapsed?.["source:"+id]===false;tab.append(footer);footer.addEventListener("toggle",event=>{if(event.target!==footer)return;preferences.sectionCollapsed={...preferences.sectionCollapsed,["source:"+id]:!footer.open};if(footer.open)populateSourceReference(footer);saveView();});}
    footer.dataset.fullWidth="true";tab.append(footer);for(const extra of tab.querySelectorAll(":scope>.source-reference"))if(extra!==footer)extra.remove();footer.sourceItems=sources;let summary=footer.querySelector(":scope>summary");if(!summary){summary=node("summary");footer.prepend(summary);}const meta=node("span",null,"source-summary-meta"),updated=node("time",when(data.updated_at));if(data.updated_at)updated.dateTime=data.updated_at;meta.append(node("span",ui("最近更新時間")+": "),updated,node("span",fmt(sources.length)+ui(" 個來源"),"source-count"));summary.replaceChildren(node("span",ui("資料來源與讀取範圍")),meta);
    if(footer.open&&!tab.hidden)populateSourceReference(footer);
  }
}
function populateSourceReference(footer){
  const old=footer.querySelector(":scope>.source-reference-content"),opened=new Set([...(old?.querySelectorAll("details[open]")||[])].map(item=>item.dataset.source)),content=node("div",null,"source-reference-content"),grid=node("div",null,"source-read-grid");
  content.append(node("p",ui("下方列出此頁讀取器的實際設定與載入結果. 列表與操作統計依來源紀錄範圍篩選, 圖表與表格條件可再縮小範圍. 未知時間紀錄只在全部範圍顯示"),"source-description"));
  grid.append(...footer.sourceItems.map(([key,source])=>sourceDetails(key,source,opened)));content.append(grid);if(old)old.replaceWith(content);else footer.append(content);
}

function renderLogSummary(){
  const logs=data.logs||{},sources=logData?.sources||logs.sources||[],problems=sources.filter(source=>["missing","config_missing","unsupported","unavailable","partly_unavailable","invalid_config","error"].includes(source.health));
  cards("log-cards",[[ui("保留 Log 事件"),fmt(logData?.entries?.length??logs.retained),ui("點整列查看紀錄明細")],[ui("資料來源"),fmt(sources.length),ui("來源診斷與歷史回補")],[ui("需要檢查的來源"),fmt(problems.length),ui("點來源列查看原因")],[ui("程式 Log 容量"),measure(sources.find(source=>source.source==="monitor")?.bytes,"bytes"),ui("最多 128 KiB, 自動輪替")]]);
  $("log-scope").textContent=logs.enabled===false?ui("Log 檢查已停用"):ui("來源 Log 與最近 24 小時錯誤回補");
  if(logs.enabled===false){logData=null;replaceRows("log-rows");$("log-message").textContent=ui("Log 檢查已停用");renderLogSources(logs.sources||[]);}else renderLogSources(sources);
}
async function loadLogs(){
  if(logBusy){logQueued=true;return;}logBusy=true;
  if(!logData)$("log-message").textContent=ui("正在讀取 Log 紀錄");
  try{
    const window=activeSourceWindow(),current=version,response=await request("/api/logs?window="+encodeURIComponent(window),{cache:"no-store"});if(!response.ok)throw new Error();if(current!==version||window!==activeSourceWindow())return;logData=response.data;
    if(!data)return;if($("view-logs").hidden){if(overviewNeeds("view-logs"))renderCharts();return;}
    renderLogSummary();
    replaceRows("log-rows",...(logData?.entries||[]).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row).append(severityTag(event.severity));cell(row,logSource(event.source));cell(row,event.module||"--","mono");cell(row,logMessage(event));cell(row,event.code||"--","mono");if(event.thread_id)eventThreadCell(row,event);else cell(row,"--");projectCell(row,event);cell(row,event.method||event.tool||"--","mono");cell(row,event.error_type||"--","mono");cell(row,event.file||"--","mono");valueCell(row,event.record_id);return clickableRow(row,()=>openDetail({kind:"log",event}));}));
    $("log-message").textContent=logData?.entries?.length?ui("保留最近 ")+fmt(logData.entries.length)+ui(" 筆. 紀錄明細依來源可取得的欄位顯示"):ui(logData?.enabled===false?"Log 檢查已停用":"無");renderCharts();attachTables();
  }catch{$("log-message").textContent=ui(logData?"Log 讀取失敗, 顯示上次取得的紀錄. 將自動重試":"Log 讀取失敗, 將自動重試");}finally{logBusy=false;if(logQueued){logQueued=false;if(!$("view-logs").hidden||overviewNeeds("view-logs"))loadLogs();}}
}
function memorySize(value){return Number.isFinite(value)?fmt(Math.round(value/1024**3*100)/100)+" GiB":"--";}
function usageMeter(percent,label){
  if(!Number.isFinite(percent)||percent<0)return "--";percent=Math.min(100,percent);const meter=node("div",null,"memory-meter"),bar=node("progress"),text=label+" ("+fmt(percent)+"%)";meter.dataset.level=percent>=85?"low":"normal";bar.max=100;bar.value=percent;bar.setAttribute("aria-label",label);bar.setAttribute("aria-valuetext",text);meter.append(node("span",text),bar);return meter;
}
function cliVersionList(versions){if(!versions.length)return "--";const list=node("div",null,"version-list");for(const version of versions.slice(0,5))list.append(node("div",version,"mono"));if(versions.length>5)list.append(button(ui("查看全部版本")+" ("+fmt(versions.length)+")",()=>openDetail({kind:"cli-versions",versions}),"link"));return list;}
function renderMonitor(){
  const m=data.monitor||{},c=data.codex,enabled=Object.values(data.settings?.observations||{}).filter(Boolean).length;
  const activeHelp=document.activeElement.closest("#monitor-environment .metadata-help"),helpRoot=activeHelp?.closest("[id]")?.id,helpIndex=activeHelp?[...activeHelp.parentElement.querySelectorAll(".metadata-help")].indexOf(activeHelp):-1,helpDismissed=activeHelp?.classList.contains("help-dismissed");
  cards("monitor-cards",[[ui("整理狀態"),ui(m.health==="error"?"整理失敗":m.health==="ok"?"正常":"啟動中"),ui("累計錯誤 ")+fmt(m.errors||0)+ui(" 次")],[ui("執行時間"),fmt(Math.floor((m.uptime_seconds||0)/60))+ui(" 分鐘"),ui("已整理 ")+fmt(m.refreshes||0)+ui(" 次")],[ui("本輪整理耗時"),fmt(m.refresh_ms)+" ms",ui("CPU 時間 ")+fmt(m.cpu_ms)+" ms"],[ui("保留工具紀錄"),fmt(m.retained_calls||0),ui("上限 ")+fmt(m.call_limit)+ui(" 筆")]]);
  const cliVersions=[...new Set((data.codex.threads||[]).map(thread=>thread.execution?.cli_version).filter(Boolean))];
  $("monitor-runtime").replaceChildren(metadataList([[ui("LAM 版本"),m.version||ui("未知")],[ui("Python 版本"),m.python||ui("未知")],[ui("Python 實作"),m.python_implementation],[ui("Codex CLI (紀錄)"),cliVersionList(cliVersions),ui("已載入對話紀錄中使用過的 Codex CLI 版本")]]));
  $("monitor-system").replaceChildren(metadataList([[ui("系統"),m.platform||ui("未知")],[ui("系統版本"),m.system_release],[ui("系統組建"),m.system_version],[ui("執行架構"),m.process_bits?fmt(m.process_bits)+ui(" 位元"):null]]));
  $("monitor-hardware").replaceChildren(metadataList([[ui("處理器"),m.processor],[ui("處理器架構"),m.architecture||ui("未知")],[ui("可用邏輯核心"),fmt(m.logical_cpus)],[ui("實體記憶體"),memorySize(m.physical_memory_bytes),ui("作業系統可使用的實體記憶體總量, 以 GiB 顯示")]]));
  const total=m.physical_memory_bytes,available=m.available_memory_bytes,used=Number.isFinite(total)&&Number.isFinite(available)&&total>0&&available>=0&&available<=total?total-available:null,percent=used==null?null:Math.round(used/total*1000)/10;
  $("monitor-device-usage").replaceChildren(metadataList([[ui("CPU 使用率"),usageMeter(m.cpu_usage_percent,ui("CPU")),ui(m.cpu_scope==="processor_group"?"目前 Windows 處理器群組的 CPU 使用率":"最近兩次取樣之間的整體 CPU 使用率")],[ui("已使用記憶體"),usageMeter(percent,memorySize(used)+" / "+memorySize(total)),ui("可用記憶體")+": "+memorySize(available)]]));const updated=$("monitor-device-updated");updated.textContent=ui("更新時間")+": "+when(m.device_checked_at);if(m.device_checked_at)updated.dateTime=m.device_checked_at;else updated.removeAttribute("datetime");
  const gpus=m.gpus?.length?m.gpus:[{}],environment=$("monitor-environment");
  for(const panel of environment.querySelectorAll(".monitor-gpu-panel"))if(Number(panel.dataset.gpuIndex)>=gpus.length)panel.remove();
  for(const [index,gpu]of gpus.entries()){
    let panel=$("monitor-gpu-panel-"+index);
    if(!panel){panel=node("section",null,"panel monitor-gpu-panel");panel.id="monitor-gpu-panel-"+index;panel.dataset.cardKey="monitor-gpu-"+index;panel.dataset.gpuIndex=index;const body=node("div");body.id="monitor-gpu-data-"+index;panel.append(node("h3"),body);environment.insertBefore(panel,$("monitor-service").parentElement);}
    panel.querySelector("h3").textContent=ui("GPU")+(gpus.length>1?" "+fmt(index+1):"");
    $("monitor-gpu-data-"+index).replaceChildren(metadataList([[ui("GPU 型號"),gpu.name||"--",ui("依平台與驅動提供的本機資料顯示, 無法取得時顯示 --")],[ui("GPU 專用記憶體"),Number.isFinite(gpu.dedicated_memory_bytes)&&gpu.dedicated_memory_bytes<1024**3?fmt(Math.round(gpu.dedicated_memory_bytes/1024**2))+" MiB":memorySize(gpu.dedicated_memory_bytes),ui("GPU 專用視訊記憶體容量")],[ui("GPU 共享記憶體上限"),memorySize(gpu.shared_memory_bytes),ui("GPU 可使用的系統共享記憶體容量上限")],[ui("GPU 驅動版本"),gpu.driver_version||"--",ui("啟動時取得的驅動版本, 部分平台不提供獨立版本, 更新驅動後需重新啟動 LAM")],...(gpu.memory_type==="unified"?[[ui("GPU 記憶體架構"),ui("統一記憶體"),ui("CPU 與 GPU 共用實體記憶體")]]:[])]));
  }
  $("monitor-service").replaceChildren(metadataList([["PID",m.pid||ui("未知")],[ui("啟用的檢查項目"),fmt(enabled)],[ui("HTTP 請求"),fmt(m.requests||0)],[ui("HTTP 錯誤回應"),fmt(m.http_errors||0)],[ui("上次整理錯誤"),m.last_error_at?when(m.last_error_at):ui("無")],[ui("目前錯誤類型"),m.error_type||ui("無")]]));
  $("monitor-storage").replaceChildren(metadataList([
    [ui("追蹤 session 檔案"),fmt(c.files||0)+" / "+fmt(m.file_limit),ui("目前追蹤的 session 檔案數 / 程式上限. 一個對話可能有多個檔案, 所以不等於對話數")],
    [ui("保留效能樣本"),fmt(m.history?.length||0)+" / "+fmt(m.history_limit),ui("每次資料整理完成會保存一筆耗時, CPU 與讀取量, 供效能圖表使用. 顯示目前筆數 / 上限, 超過上限移除最舊樣本, 重啟後清空")],
    [ui("保留狀態事件"),fmt(m.events?.length||0)+" / "+fmt(m.event_limit),ui("程式啟動, 設定變更, 整理錯誤與恢復等事件的目前筆數 / 上限. 保存在記憶體, 超過上限移除最舊事件, 重啟後清空")],
    [ui("待完成的紀錄片段 (bytes)"),fmt(m.buffer_bytes),ui("session 最後一行尚未寫完時, 暫存等待下次讀取的內容大小")],
    [ui("片段保留上限 (bytes)"),fmt(m.buffer_limit),ui("所有 session 未完成行的暫存總上限. 超過時移除片段, 避免記憶體持續增加")],
    [ui("累計移除的舊工具紀錄"),fmt(m.trimmed_calls||0),ui("工具紀錄超過每個 session 或整體保留上限時, 移除舊資料的累計筆數. 重新建立 Codex 檢查後重新累積")],
    [ui("累計 session 讀取量 (bytes)"),fmt(c.bytes_read),ui("目前 Codex 檢查累計讀取的 session 位元組數, 包含初次尾端, 增量與狀態回查. 重新建立 Codex 檢查後重新累積")],
    [ui("損壞紀錄 (行)"),fmt(c.malformed_lines),ui("讀到無法解析為 JSON 的完整行數. 未寫完的最後一行會先暫存等待, 不直接算成損壞")],
    [ui("上次資料大小 (bytes)"),fmt(m.snapshot_bytes),ui("最近一次資料 API 回應的 JSON 大小, 尚未 gzip 壓縮")],
    [ui("上次傳輸大小 (bytes)"),fmt(m.transfer_bytes),ui("最近一次資料 API 回應實際送出的內容大小. 使用 gzip 時為壓縮後大小, 不包含 HTTP header")],
    [ui("Jev 資料庫大小 (bytes)"),fmt(m.jev_database_bytes),ui("Jev 的 SQLite 主資料庫檔案大小, 不含 WAL / SHM 暫存檔")]
  ]));
  if(helpIndex>=0){const term=$(helpRoot)?.querySelectorAll(".metadata-help")[helpIndex];if(term){term.classList.toggle("help-dismissed",!!helpDismissed);term.focus({preventScroll:true});}}
  $("monitor-retention").textContent=ui("效能樣本與狀態事件保存在記憶體, 超過上限會移除舊資料, 重新啟動後重新累積");
  const names=monitorEventLabels;
  replaceRows("monitor-event-rows",...(m.events||[]).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,names[event.kind]||event.kind);cell(row,event.error_type||"--","mono");return row;}));
}
function renderConnectionStatus(stale=false){
  const value=data?.codex?.connection||{},names={response:"最近有回應",error:"連線錯誤",paused:"檢查停用",unknown:"待確認"},status=$("codex-live"),state=stale?"unknown":data?.settings?.observations?.codex===false?"paused":Date.now()-Date.parse(value.observed_at)>(value.recent_seconds||300)*1000?"unknown":value.state||"unknown";
  status.textContent=ui(stale?"資料未更新":names[state]||"待確認");$("codex-connection").dataset.state=state;$("lam-connection").dataset.state=stale?"error":"response";
  const evidence={model_usage:"模型用量回報",connection_error:"連線錯誤紀錄"},parts=[ui("依本機紀錄檢查 Codex 連線, 最近 5 分鐘有模型回報時顯示最近有回應"),ui("最後回應")+": "+when(value.last_response_at),ui("最近檢查時間")+": "+when(value.observed_at)];if(value.evidence)parts.push(ui("依據")+": "+ui(evidence[value.evidence]||value.evidence));bindHelp($("codex-connection"),parts.join(". "));
}
const highlightDescriptions={tools:"查看已載入紀錄中的工具呼叫次數與工具種類",git:"查看目前來源範圍內的 Git 操作與所屬對話",skills:"查看技能文件讀取次數與關聯對話",checks:"查看 test, build, lint 與型別檢查操作",files:"查看目前來源範圍內的檔案讀寫與修改紀錄",web:"查看網路工具操作與參考網址",errors:"查看來源範圍內的錯誤與警告紀錄",logs:"查看來源 Log 的執行紀錄與讀取狀態",sqlite:"查看已辨識的 SQL 操作與診斷紀錄"};
function renderHighlights(){
  const c=data.codex,m=data.mcp||{events:[],categories:{}};
  const items=[[ui("工具"),"tools",fmt(c.observed_tool_calls||0)+ui(" 次呼叫"),fmt(Object.keys(c.tools||{}).length)+ui(" 種工具")],[ui("Git 操作"),"git",fmt(c.git?.total||0)+ui(" 筆"),Object.entries(c.git?.operations||{}).sort((a,b)=>b[1]-a[1]).slice(0,3).map(([key,n])=>key+" "+n).join(" · ")],[ui("技能"),"skills",fmt(Object.keys(c.skills?.counts||{}).length)+ui(" 個技能"),fmt(c.skills?.events?.length||0)+ui(" 筆讀取紀錄")],[ui("驗證"),"checks",fmt(c.checks?.length||0)+ui(" 筆"),ui("test / build / lint")],[ui("檔案"),"files",fmt(c.file_activity?.total||0)+ui(" 筆"),ui("讀取 ")+fmt(c.file_activity?.operations?.read||0)+ui(" 次")],[ui("網路"),"web",fmt(m.events.filter(e=>e.server==="web").length)+ui(" 筆"),ui("參考網址 ")+fmt(new Set(references(m.events).map(e=>e.url)).size)+ui(" 個")]];
  items.push([ui("錯誤紀錄"),"errors",fmt(data.errors?.severities?.error||0)+ui(" 筆錯誤"),fmt(data.errors?.severities?.warning||0)+ui(" 筆警告")]);
  items.push([ui("Log 紀錄"),"logs",fmt(data.logs?.retained||0)+ui(" 筆事件"),ui("來源健康狀態與執行紀錄")]);
  items.push([ui("SQL / SQLite"),"sqlite",fmt(c.sqlite?.total||0)+ui(" 筆操作"),ui("工具紀錄與 SQL 診斷")]);

  for(const source of data.mcp?.servers||[]){if(!source.enabled||source.server==="web")continue;const latest=m.events.filter(event=>event.server===source.server).sort((a,b)=>(b.completed_at||b.timestamp||"").localeCompare(a.completed_at||a.timestamp||""))[0],report=data.mcp.telemetry?.[source.server],info=report?telemetryFields(report.summary).slice(0,3).map(([key,value])=>metricLabel(key)+": "+telemetryValue(value)).join(" · "):latest?mcpBrief(latest):source.description||ui(data.mcp.categories[source.category]||"MCP 工具");items.push([source.server,"mcp",fmt(source.calls)+ui(" 次呼叫 · ")+fmt(source.recognized)+ui(" 處 exec 辨識"),info,source.server]);}
  const highlightOrder=["usage","files","sqlite","git","checks","skills","tools","web","mcp","errors","logs"],usage=c.usage,known=(c.threads||[]).map(thread=>thread.tokens?.total_tokens).filter(Number.isFinite);items.unshift([ui("用量與額度"),"usage",known.length?fmt(known.reduce((sum,value)=>sum+value,0))+" tokens":"--",usage?.plan_type||"--"]);items.sort((a,b)=>highlightOrder.indexOf(a[1])-highlightOrder.indexOf(b[1]));
  $("overview-highlights").replaceChildren(...items.map(([name,tab,value,info,category])=>{const el=button(null,()=>{if(category&&tab==="mcp"){openDetail({kind:"mcp-source",server:category});return;}if(tab==="jev"){openDetail({kind:"mcp-source",server:"jev"});return;}navigateToTab(tab);},"highlight-card tab-order-row");el.dataset.tabOrder=category?"mcp:"+category:tab;el.append(node("h3",name),node("div",value,"highlight-value"),node("p",info,"muted"));bindHelp(el,[ui(highlightDescriptions[tab]||"查看來源的活動統計與操作明細"),value,info].filter(Boolean).join(". "));return el;}));arrangeHighlights();
}
function metadataList(items){
  const list=node("dl",null,"metadata-grid");
  for(const [label,value,description]of items){
    const help=description||fieldDescription(label);
    const term=node("dt",label);
    if(help){const mark=node("span","ⓘ","help-mark");mark.setAttribute("aria-hidden","true");term.classList.add("metadata-help");term.tabIndex=0;term.setAttribute("aria-label",label);term.replaceChildren(node("span",label,"help-label"),mark);bindHelp(term,help);for(const event of ["mouseleave","blur"])term.addEventListener(event,()=>term.classList.remove("help-dismissed"));}
    if(!help)bindHelp(term,label);const entry=node("dd");if(value instanceof Node)entry.append(value);else entry.textContent=value==null||value===""?"--":value;list.append(term,entry);
  }
  return list;
}
function table(headers,rows,title,key){const wrap=node("div",null,"table-wrap"),el=node("table"),head=node("thead"),tr=node("tr"),body=node("tbody");el.dataset.tableKey=key||"detail:"+detail.kind+":"+headers.join("|");if(key)body.id=key;if(title)el.dataset.tableTitle=title;for(const label of headers)tr.append(node("th",label));head.append(tr);body.append(...rows);el.append(head,body);wrap.append(el);return wrap;}

function toolEventRows(events){return events.slice(0,100).map(event=>{const row=node("tr");cell(row,when(event.timestamp));const outer=cell(row,null,"tool-column");outer.append(toolButton(event.tool,()=>openDetail({kind:"tool",tool:event.tool})));const inner=cell(row,null,"nested-tool-column"),list=node("div",null,"tools");for(const [tool,count]of sorted(event.nested_tools))list.append(toolButton(tool,()=>openDetail({kind:"nested-tool",tool}),count));inner.append(list.childElementCount?list:node("span","--"));cell(row,when(event.completed_at));valueCell(row,event.duration_ms);return clickableRow(row,()=>openDetail({kind:"tool-call",event}),ui("查看執行內容"));});}
const toolPurposes={exec:"執行 JavaScript, 可在一次操作中呼叫其他工具並整理回傳結果",exec_command:"執行終端機命令, 例如讀取檔案, Git 操作或測試",write_stdin:"向已啟動的命令送入資料, 或取得後續執行結果",apply_patch:"依 patch 新增, 修改或刪除檔案",web__run:"搜尋網路, 開啟參考頁面, 或取得天氣與其他網路資料",view_image:"檢視本機圖片",js:"在持續存在的 JavaScript runtime 中執行操作",js_reset:"重設 JavaScript runtime 與其中的變數",document_status:"檢查文件處理套件與 OCR 是否就緒",extract_document:"取得文件文字與位置資訊, 必要時執行 OCR, 可預覽或寫出 Markdown",inspect_markdown:"讀取 Markdown 的標題, 指定段落與 SHA-256",locate_markdown_extracts:"尋找與原始文件相符的 Markdown 抽出檔",update_markdown:"檢查目前 hash 後, 預覽或寫入 Markdown 更新",workspace_status:"查看環境檢查工具的版本與可讀取範圍",compare_environment:"比對兩個目錄的檔案與 SHA-256, 找出環境差異",validation_evidence:"整理既有驗證紀錄與結果",jev_rank:"依問題將候選內容排序, 決定優先閱讀順序",jev_evaluate:"依明確的評分項目評估指定內容",jev_status:"檢查 Jev 設定, 可選擇測試 API 連線",pull_requests_checks:"讀取 PR / MR 的 CI 檢查結果",create_worktree:"建立供目前工作使用的 Git worktree",archive_worktree:"保存 worktree 的工作快照並封存",restore_worktree:"還原已封存的 worktree",fork_thread:"從既有對話建立保留脈絡的分支",handoff_thread:"移轉工作與 Git 狀態",automation_update:"建立或調整排程工作",read_thread:"讀取指定工作的狀態與摘要",list_threads:"列出目前工作與對話",open_in_codex:"在 Codex 面板開啟檔案, 頁面或其他成果"};
function defaultPurpose(tool){const match=tool.match(/^mcp__(.+)__(.+)$/);if(match)return data?.mcp?.servers?.find(source=>source.server===match[1])?.description||"--";const suffix=tool.split("__").at(-1).split(".").at(-1);return ui(toolPurposes[tool]||toolPurposes[suffix]||"--");}
function purpose(tool){return data?.settings?.tool_descriptions?.[tool]||defaultPurpose(tool);}
function purposeBlock(content,tool){const section=node("div",null,"purpose-block"),edit=button("✎",()=>openToolDocs(tool),"purpose-edit");edit.setAttribute("aria-label",ui("編輯工具說明"));edit.title=ui("編輯工具說明");section.append(node("p",purpose(tool),"tool-purpose"),edit);content.append(section);}
function appendJevCalls(content,t){if(!data.availability?.jev&&!t.jev_calls?.length)return;content.append(node("h4",ui("Jev 呼叫")));if(!t.jev_calls?.length){content.append(node("p","--","empty"));return;}for(const call of t.jev_calls){const row=node("div",null,"detail-row");row.append(node("span",when(call.timestamp)),button(call.operation+ui(" · 送出 / 回傳"),()=>openJev(call,t),"link"));content.append(row);}}
const detailHistory=[];let renderedDetail=null;
function rememberDetail(){if(detail&&$("detail-dialog").open){detailHistory.push({view:detail,scroll:$("detail-content").scrollTop});if(detailHistory.length>20)detailHistory.shift();}}
function clearDetailTabs(){const nav=$("detail-content").previousElementSibling;if(nav?.classList.contains("modal-tabs"))nav.remove();}
function updateDetailBack(){$("detail-back").disabled=!detailHistory.length;}
function openDetail(value,remember=true){if(remember)rememberDetail();detail=value;renderDetail();$("detail-content").scrollTop=0;updateDetailBack();if(!$("detail-dialog").open)openDialog($("detail-dialog"));if(value.kind==="skill-file"&&value.fileInfo?.readable&&!value.documents?.some(file=>file.relative_path===value.documentPath))loadSkillDocuments(value,value.documentPath);if(value.kind==="mcp-file"&&!value.documentContent&&!value.documentLoading)loadMcpDocument(value);if(["error","log"].includes(value.kind)&&value.event.content_id&&!value.errorLoaded&&!value.errorLoading)loadErrorContent(value);if(value.kind==="tool-call"&&!value.toolLoaded&&!value.toolLoading&&!value.toolError)loadToolContent(value);if(value.kind==="mcp"&&!value.mcpLoaded&&!value.mcpLoading&&!value.mcpError)loadMcpContent(value);if(value.kind==="file"&&!value.fileMetadata&&!value.fileMetadataLoading&&!value.fileMetadataError)loadFileMetadata(value);if(value.kind==="project"&&!value.projectLoaded&&!value.projectLoading)loadProjectContent(value);if(value.kind==="instructions"&&!value.instructionsLoaded&&!value.instructionsLoading)loadInstructions(value);if(value.kind==="sqlite"&&!value.sqlLoaded&&!value.sqlLoading)loadSqlContent(value);if(value.kind==="operation"&&(data.codex.git?.events||[]).some(event=>event.thread_id===value.event.thread_id&&event.call_id===value.event.call_id&&event.operation===value.event.operation)&&!value.gitLoaded&&!value.gitLoading)loadGitContent(value);}
function parseDisplayData(text){
  try{return {ok:true,value:JSON.parse(text)};}catch{}
  // Normalize JSON-compatible literal spelling only, without evaluating expressions.
  try{const normalized=text.replace(/"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\b(?:True|False|None)\b/g,token=>token[0]==="'"?'"'+token.slice(1,-1).replace(/\\'/g,"'").replace(/"/g,'\\"')+'"':({True:"true",False:"false",None:"null"})[token]||token);return {ok:true,value:JSON.parse(normalized)};}catch{return {ok:false};}
}
const payloadObservers=new Map();
function clearPayloadObservers(scope){for(const [pre,observer]of payloadObservers)if(scope.contains(pre)){observer.disconnect();payloadObservers.delete(pre);}}
function highlightCode(target,text,format="text"){
  const tokens=/\/\*[\s\S]*?\*\/|(?:--|\/\/|#)[^\n]*|"(?:\\.|[^"\\])*"|'(?:\\.|''|[^'\\])*'|\b(?:true|false|null|True|False|None)\b|-?\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b|\b[A-Za-z_]\w*\b/g;let end=0;
  const keywords=/^(?:select|from|where|insert|into|values|update|set|delete|create|table|alter|drop|join|left|right|inner|outer|on|as|and|or|not|null|group|order|by|having|limit|offset|union|distinct|with|case|when|then|else|end|begin|commit|rollback|pragma|explain|return|const|let|var|function|async|await|if|for|while|def|class|import|export|try|except|catch|finally|raise)$/i;
  for(const match of text.matchAll(tokens)){target.append(document.createTextNode(text.slice(end,match.index)));const token=match[0],tail=text.slice(match.index+token.length),kind=/^(?:\/\*|--|\/\/|#)/.test(token)?"comment":/^["']/.test(token)?/^\s*:/.test(tail)?"key":"string":/^(?:true|false|null|True|False|None)$/.test(token)?"boolean":/^-?\d/.test(token)?"number":/^(?:yaml|yml|json)$/.test(format)&&/^\s*:/.test(tail)?"key":keywords.test(token)?"keyword":null;target.append(kind?node("span",token,"code-"+kind):document.createTextNode(token));end=match.index+token.length;}
  target.append(document.createTextNode(text.slice(end)));
}

function markdownInline(target,text){
  const tokens=/(`+)([^`]*?)\1|!\[([^\]]*)\]\(([^)]+)\)|\[([^\]]+)\]\(([^)]+)\)|\*\*([^*]+)\*\*|__([^_]+)__|\*([^*]+)\*|_([^_]+)_/g;let end=0;
  for(const match of text.matchAll(tokens)){target.append(document.createTextNode(text.slice(end,match.index)));if(match[1])target.append(node("code",match[2]));else if(match[3]!=null)target.append(node("span",match[3]));else if(match[5]){let link=node("span",match[5]);try{const url=new URL(match[6]);if(["http:","https:"].includes(url.protocol)&&!url.username&&!url.password){link=node("a",match[5]);link.href=url.href;link.target="_blank";link.rel="noopener noreferrer";}}catch{}target.append(link);}else target.append(node(match[7]||match[8]?"strong":"em",match[7]||match[8]||match[9]||match[10]));end=match.index+match[0].length;}target.append(document.createTextNode(text.slice(end)));
}
function markdownRenderer(target){
  let carry="",fence=null,code=null,list=null,paragraph=null,table=null,previous=null;
  function inline(tag,text,root=target){const element=node(tag);markdownInline(element,text);root.append(element);return element;}
  function line(text){
    const marker=text.match(/^\s*(`{3,}|~{3,})(.*)$/);if(fence){if(marker&&marker[1][0]===fence[0]&&marker[1].length>=fence.length){fence=null;code=null;}else{highlightCode(code,text+"\n",code.dataset.format);}}else if(marker){fence=marker[1];const pre=node("pre",null,"markdown-code");code=node("code");code.dataset.format=marker[2].trim()||"text";pre.append(code);target.append(pre);paragraph=list=table=null;
    }else if(!text.trim()){paragraph=list=table=null;
    }else if(/^\s*\|?\s*:?-{3,}/.test(text)&&previous?.includes("|")&&paragraph){const headers=previous.replace(/^\||\|$/g,"").split("|");table=node("table");const row=node("tr");for(const value of headers)inline("th",value.trim(),row);const head=node("thead"),body=node("tbody");head.append(row);table.append(head,body);paragraph.replaceWith(table);paragraph=list=null;
    }else if(table&&text.includes("|")){const row=node("tr");for(const value of text.replace(/^\||\|$/g,"").split("|"))inline("td",value.trim(),row);table.lastChild.append(row);
    }else{table=null;const heading=text.match(/^\s{0,3}(#{1,6})\s+(.+)$/),bullet=text.match(/^\s*(?:([-+*])|\d+[.)])\s+(.+)$/);if(heading){inline("h"+heading[1].length,heading[2]);paragraph=list=null;}else if(/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(text)){target.append(node("hr"));paragraph=list=null;}else if(bullet){const tag=bullet[1]?"ul":"ol";if(!list||list.tagName.toLowerCase()!==tag){list=node(tag);target.append(list);}inline("li",bullet[2],list);paragraph=null;}else if(/^\s*>/.test(text)){inline("blockquote",text.replace(/^\s*>\s?/,""));paragraph=list=null;}else{list=null;if(!paragraph)paragraph=inline("p",text);else{paragraph.append(document.createTextNode("\n"));markdownInline(paragraph,text);}}}previous=text;
  }
  return (chunk,complete)=>{carry+=chunk;const lines=carry.split("\n");carry=lines.pop();for(const value of lines)line(value.replace(/\r$/, ""));if(complete&&carry){line(carry.replace(/\r$/,""));carry="";}};
}
function documentPayload(value,format="text"){
  const text=value?.content_page?value.text:String(value??"--"),kind=value?.format||format,markdown=/^(?:md|markdown)$/i.test(kind)||kind==="text"&&/(?:^|\n)#{1,6}\s+|(?:^|\n)```(?:markdown|md)?\s*\n/.test(text);if(!markdown)return codePayload(value,format);
  const root=node("section",null,"document-preview"),modes=node("div",null,"document-modes"),body=node("div"),preview=button(ui("預覽"),()=>show(true)),raw=button(ui("原文"),()=>show(false));modes.setAttribute("role","group");modes.setAttribute("aria-label",ui("文件顯示模式"));modes.append(preview,raw);root.append(modes,body);
  function show(selected){clearPayloadObservers(body);body.replaceChildren(codePayload(value,"markdown",selected));preview.setAttribute("aria-pressed",String(selected));raw.setAttribute("aria-pressed",String(!selected));}show(true);return root;
}

function codePayload(value,format="text",preview=false){
  const page=value?.content_page?value:null,pre=node("pre",null,"payload syntax-payload"),code=node(preview?"div":"code",null,preview?"markdown-preview":""),more=button(ui("載入更多內容"),load,"payload-more"),state=node("span",null,"payload-progress");let text=page?.text??String(value??"--"),offset=0,busy=false,failed=false,observer;
  pre.append(code,more,state);const markdown=preview?markdownRenderer(code):null;if(preview)pre.classList.add("markdown-payload");
  function draw(){const end=Math.min(offset+32768,text.length);if(markdown)markdown(text.slice(offset,end),end===text.length&&page?.next==null);else highlightCode(code,text.slice(offset,end),format==="text"?page?.format||format:format);offset=end;more.hidden=offset>=text.length&&page?.next==null;state.textContent=more.hidden?"":ui("已載入")+" "+fmt(offset)+" / "+fmt(page?.total??text.length)+" "+ui("字元");if(more.hidden){observer?.disconnect();payloadObservers.delete(pre);}}
  async function load(){if(busy||failed)return;busy=true;more.disabled=true;try{if(offset>=text.length&&page?.next!=null){if(!page.load)throw new Error(ui("內容讀取失敗"));const next=await page.load();page.text+=next.text;page.next=next.next;text=page.text;}draw();}catch(error){failed=true;observer?.disconnect();state.textContent=error.message||ui("內容讀取失敗");more.hidden=true;}finally{busy=false;more.disabled=false;}}
  draw();if(!more.hidden&&typeof IntersectionObserver!=="undefined"){observer=new IntersectionObserver(entries=>{if(entries.some(entry=>entry.isIntersecting))load();},{root:pre,rootMargin:"100px"});observer.observe(more);payloadObservers.set(pre,observer);}return pre;
}
function jsonPayload(value){return codePayload(JSON.stringify(value,null,2)??"null","json");}
function structuredPayload(label,value){
  const fold=node("details",null,"structured-payload"),body=node("div",null,"structured-payload-body");fold.append(node("summary",label),body);let initialized=false;
  function render(target,item,depth=0){
    if(item?.content_page){target.append(documentPayload(item));return;}
    if(depth>64){target.append(codePayload(String(item)));return;}
    if(typeof item==="string"){const text=item.trim(),parsed=/^[\[{]/.test(text)?parseDisplayData(text):{ok:false};if(parsed.ok)return render(target,parsed.value,depth+1);target.append(documentPayload(item||"--"));return;}
    if(Array.isArray(item)&&item.every(value=>value&&typeof value.text==="string")){let offset=0;const more=button(ui("載入更多內容"),append,"payload-more");function append(){for(const entry of item.slice(offset,offset+25))render(target,entry.text,depth+1);offset+=25;more.hidden=offset>=item.length;target.append(more);}append();return;}
    target.append(jsonPayload(item));
    if(item&&typeof item==="object")for(const [key,value]of Object.entries(item))if(typeof value==="string"&&(value.includes("\n")||/^[\[{]/.test(value.trim())))target.append(structuredPayload(metricLabel(key),value));
  }
  fold.addEventListener("toggle",()=>{if(fold.open&&!initialized){initialized=true;render(body,value);}});return fold;
}
async function lazyContent(path,values){
  const query=new URLSearchParams({...values,lazy:"1"}),response=await request(path+"?"+query,{cache:"no-store"});if(!response.ok)throw new Error(ui("內容讀取失敗"));
  for(const [field,page]of Object.entries(response.data))if(page?.content_page)page.load=async()=>{const nextQuery=new URLSearchParams(query);nextQuery.set("field",field);nextQuery.set("offset",page.next);nextQuery.set("revision",page.revision);const result=await request(path+"?"+nextQuery,{cache:"no-store"});if(!result.ok)throw new Error(ui(result.status===409?"來源內容已更新, 請重新開啟明細":"內容讀取失敗"));return result.data;};return response.data;
}
function renderDetail(){
  clearDetailTabs();
  if(!detail||!data)return;
  const content=$("detail-content"),restore=renderedDetail===detail,folds=restore?[...content.querySelectorAll("details")].map(el=>el.open):null,scroll=content.scrollTop,payloadScroll=restore?[...content.querySelectorAll(".payload,.table-wrap")].map(el=>[el.scrollTop,el.scrollLeft]):null,focus=restore?[...content.querySelectorAll("details > summary")].indexOf(document.activeElement):-1;renderedDetail=detail;clearPayloadObservers(content);content.replaceChildren();$("detail-title").parentElement.querySelector(".detail-time")?.remove();
  if(detail.kind==="thread"||detail.kind==="thread-tools"){
    const t=data.codex.threads.find(item=>item.thread_id===detail.id)||detail.thread;detail.thread=t;
    $("detail-title").textContent=(detail.kind==="thread-tools"?ui("工具明細 · "):"")+title(t);
    if(detail.kind==="thread"){
      content.append(metadataList([["Thread ID",t.thread_id],[ui("對話建立時間"),when(t.created_at)],[ui("Token 更新時間"),when(t.token_updated_at)],[ui("類型"),labels.type[t.activity_type]||ui("未知")],[ui("執行位置"),labels.environment[t.environment]||ui("未知")],[ui("專案"),t.project_name||t.project_id||(t.project_scope==="none"?ui("無專案"):ui("未知"))],[ui("活動來源"),labels.trigger[t.trigger]||ui("未知")],[ui("排程綁定"),t.has_schedule==null?ui("未知"):ui(t.has_schedule?"有":"無")],[ui("狀態"),statusBadge(t)],[ui("Model"),t.model||"--"],[ui("Reasoning 等級"),reasoningBadge(t.reasoning_effort)],[ui("Model 設定更新時間"),when(t.context_updated_at)]]));
      content.append(metadataList([[ui("狀態確認時間"),when(t.status_updated_at)],[ui("資料來源"),t.event_only?ui("操作或診斷紀錄中的對話 ID"):t.metadata_only?ui("本機對話目錄"):ui("Codex session 與本機目錄")],[ui("狀態來源"),t.status_source==="session_lifecycle"?ui("工作開始 / 完成事件"):t.status_source==="catalog_status"?ui("本機目錄狀態"):t.status_source==="cached_lifecycle"?ui("最近確認的工作事件"):t.status_backfill_pending?ui("正在回查較早的工作事件"):ui("來源未提供狀態")]]));
      const project=projectRows().find(item=>item.id===t.project_id);if(project)content.append(button(ui("查看專案")+" · "+(project.name||project.id),()=>openDetail({kind:"project",project})));
      if(t.metadata_only)content.append(node("p",ui(t.event_only?"目前只取得紀錄中的對話 ID, 尚未載入對應的對話資訊":t.activity_type==="chat"?"這筆雲端對話的資料來自本機目錄. 可取得的欄位依本機目錄資料":"目前只取得對話目錄資料, 尚未載入對應的 session"),"muted"));
      const executionLabels={model_provider:"供應商",cli_version:ui("Codex 版本"),originator:ui("執行程式"),history_mode:ui("歷史紀錄模式"),approval_policy:ui("操作審核模式"),sandbox_mode:ui("沙盒模式"),collaboration_mode:ui("協作模式"),context_window:ui("Context window (tokens)"),git_branch:ui("紀錄時的 Git 分支"),git_commit:ui("紀錄時的 Git commit"),parent_thread_id:ui("上層 Thread ID"),agent_nickname:ui("Agent 名稱"),agent_role:ui("Agent 角色"),service_tier:ui("服務等級"),reasoning_summary:ui("Reasoning 摘要模式"),approvals_reviewer:ui("操作審核來源")},execution=Object.entries(t.execution||{}).map(([key,value])=>[executionLabels[key]||key,typeof value==="number"?fmt(value):value]);
      if(execution.length)content.append(node("h4",ui("執行設定")),metadataList(execution));
      content.append(metadataList([[ui("工作總耗時 (秒)"),fmt(t.task_duration_ms==null?null:Math.round(t.task_duration_ms/100)/10)],[ui("已取得起訖的工作次數"),fmt(t.task_runs)]]));
      content.append(node("h4",ui("Token 用量 (tokens)")),metadataList(Object.entries({...tokenLabels,cache_write_input_tokens:ui("快取寫入 Token")}).map(([key,label])=>[label,fmt(t.tokens?.[key])])),node("h4",ui("工具使用 (次)")));
    }else content.append(threadLink(t,ui("查看對話細節")));
    const counts=node("div",null,"tools");
    for(const [tool,count]of sorted(t.tools)){const el=toolButton(tool,()=>openDetail({kind:"tool",tool,ids:[t.thread_id]}),count);counts.append(el);}
    content.append(counts,node("p",ui("工具種類 ")+fmt(Object.keys(t.tools||{}).length),"muted"));
    if(Object.keys(t.nested_tools||{}).length){const nested=node("div",null,"tools");for(const [tool,count]of sorted(t.nested_tools)){const el=toolButton(tool,()=>openDetail({kind:"nested-tool",tool,ids:[t.thread_id]}),count);nested.append(el);}content.append(node("h4",ui("exec 內辨識到的工具")),nested);}
    if(t.tool_events?.length)content.append(node("h4",ui("工具呼叫紀錄")),table([ui("呼叫時間"),ui("工具"),ui("內層工具"),ui("回傳時間"),ui("耗時 (ms)")],toolEventRows(t.tool_events.map(event=>({...event,thread_id:t.thread_id})))));
    content.append(node("h4",ui("檔案讀寫紀錄")));const files=[...(t.file_reads||[]),...(t.file_changes||[])];
    if(files.length)content.append(table([ui("時間"),ui("操作"),ui("檔案位置"),ui("工具"),ui("紀錄方式"),ui("工具回傳時間"),ui("耗時 (ms)")],fileRows(files,false)));else content.append(node("p",ui("無"),"empty"));
    const threadErrors=(data.errors?.events||[]).filter(e=>e.thread_id===t.thread_id);if(threadErrors.length)content.append(node("h4",ui("錯誤與警告")),table([ui("時間"),ui("等級"),ui("類型"),ui("對話"),ui("專案"),ui("來源"),ui("工具"),ui("錯誤說明"),ui("代碼")],errorRows(threadErrors)));
    appendThreadMcp(content,t);
  }else if(detail.kind==="cli-versions"){
    $("detail-title").textContent=ui("Codex CLI (紀錄)");const list=node("div",null,"version-list");for(const version of detail.versions)list.append(node("div",version,"mono"));content.append(node("p",ui("已載入對話紀錄中使用過的 Codex CLI 版本"),"muted"),list);
  }else if(detail.kind==="project"){
    const project=detail.project,threads=(data.codex.threads||[]).filter(thread=>thread.project_id===project.id);$("detail-title").replaceChildren(projectIdentity(project));content.append(metadataList([["Project ID",project.id],[ui("類型"),projectKind(detail.projectContent||project)],[ui("已載入對話"),fmt(threads.length)]]));if(detail.projectError)content.append(node("p",detail.projectError,"empty"));else if(!detail.projectLoaded)content.append(node("p",ui("讀取專案設定中"),"empty"));else{const folders=detail.projectContent?.folders||[];if(folders.length){const list=node("ul",null,"project-folders");for(const folder of folders)list.append(node("li",typeof folder==="string"?folder:folder.path||folder.name,"mono path-cell"));content.append(node("h4",ui("可使用的資料夾")),list);}else content.append(node("p",ui("來源未提供資料夾設定"),"muted"));}content.append(button(ui("查看專案 AGENTS.md"),()=>openDetail({kind:"instructions",scope:"project",project})),node("h4",ui("所屬對話")),node("p",ui("對話數與關聯只涵蓋目前已載入的對話"),"muted"));if(threads.length)content.append(table([ui("對話"),ui("狀態"),ui("模型"),ui("最近活動")],threads.map(thread=>{const row=node("tr");cell(row).append(threadLink(thread));cell(row).append(statusBadge(thread));cell(row,thread.model||"--");cell(row,when(thread.updated_at));return row;})));else content.append(node("p",ui("目前載入範圍沒有所屬對話"),"empty"));
  }else if(detail.kind==="instructions"){
    $("detail-title").textContent=detail.scope==="global"?ui("全域 AGENTS.md"):(detail.project.name||detail.project.id)+" · AGENTS.md";content.append(node("p",ui("檢視目前來源的指示文件"),"muted"));if(detail.instructionsError)content.append(node("p",detail.instructionsError,"empty"));else if(!detail.instructionsLoaded)content.append(node("p",ui("讀取文件中"),"empty"));else{const documents=detail.instructionsContent?.documents||[];if(!documents.length)content.append(node("p",ui("目前來源沒有可讀取的 AGENTS.md"),"empty"));for(const document of documents){content.append(node("h4",document.file||"AGENTS.md"),metadataList([[ui("最後更新時間"),when(document.modified_at)],[ui("讀取大小 (bytes)"),fmt(document.read_bytes)]]),documentPayload(document.text||"--","markdown"));if(document.truncated)content.append(node("p",ui("文件較長, 顯示前段內容"),"muted"));}}
  }else if(detail.kind==="tool"||detail.kind==="nested-tool"){
    const nested=detail.kind==="nested-tool",key=nested?"nested_tools":"tools",threads=(data.codex.threads||[]).filter(t=>(!detail.ids||detail.ids.includes(t.thread_id))&&t[key]?.[detail.tool]);
    $("detail-title").textContent=(nested?ui("exec 內層工具 · "):ui("工具明細 · "))+detail.tool;
    purposeBlock(content,detail.tool);
    content.append(node("p",ui("共 ")+fmt(threads.reduce((sum,t)=>sum+t[key][detail.tool],0))+(nested?ui(" 處辨識紀錄"):ui(" 次"))+" · "+fmt(threads.length)+ui(" 個對話"),"detail-total"));
    content.append(table([ui("對話"),nested?ui("辨識數量"):ui("使用次數")],threads.map(t=>{const row=node("tr");cell(row).append(threadLink(t));cell(row,fmt(t[key][detail.tool]));return row;}),ui("相關對話")));
    const events=threads.flatMap(t=>(t.tool_events||[]).filter(e=>nested?e.nested_tools?.[detail.tool]:e.tool===detail.tool).map(event=>({...event,thread_id:t.thread_id}))).sort((a,b)=>(b.timestamp||"").localeCompare(a.timestamp||""));
    if(events.length)content.append(table([ui("呼叫時間"),ui("工具"),ui("內層工具"),ui("回傳時間"),ui("耗時 (ms)")],toolEventRows(events),ui("工具呼叫紀錄")));
  }else if(detail.kind==="tool-call"){
    const event=detail.event;$("detail-title").textContent=event.tool+" · "+ui("執行內容");
    if(detail.toolError)content.append(node("p",detail.toolError,"empty"));else if(!detail.toolLoaded)content.append(node("p",ui("讀取內容中"),"empty"));else{content.append(structuredPayload(ui("送出內容"),detail.toolContent?.request??"--"),structuredPayload(ui("工具回覆"),detail.toolContent?.response??"--"));if(detail.toolContent?.truncated)content.append(node("p",ui("顯示內容已達長度上限"),"muted"));}
    const info=node("tr");cell(info,when(event.timestamp));cell(info,when(event.completed_at));valueCell(info,event.duration_ms);cell(info,event.call_id,"mono");content.append(table([ui("呼叫時間"),ui("工具回傳時間"),ui("工具耗時 (ms)"),"Call ID"],[info],ui("執行資訊"),"detail:tool-call:info"));
    const files=(data.codex.file_activity?.events||[]).filter(file=>file.thread_id===event.thread_id&&file.call_id===event.call_id);if(files.length)content.append(table([ui("操作"),ui("檔案位置"),ui("工具回傳時間"),ui("耗時 (ms)")],files.map(file=>{const row=node("tr");cell(row).append(tag(fileLabels[file.operation]||file.operation));cell(row,fileLocation(file),"mono path-cell");cell(row,when(file.completed_at));valueCell(row,file.duration_ms);return clickableRow(row,()=>openDetail({kind:"file",event:file}));}),ui("相關檔案"),"detail:tool-call:files"));
  }else if(detail.kind==="operation"){
    const event=detail.event;$("detail-title").textContent=event.operation;
    content.append(metadataList([[ui("呼叫時間"),when(event.timestamp)],[ui("工具回傳時間"),when(event.completed_at)],[ui("工具耗時 (ms)"),fmt(event.duration_ms)],[ui("工作目錄名稱"),event.repository||ui("未知")],[ui("工具 Call ID"),event.call_id]]));
    if((data.codex.git?.events||[]).some(item=>item.thread_id===event.thread_id&&item.call_id===event.call_id&&item.operation===event.operation)){
      content.append(node("h4",ui("Git 指令")),codePayload(detail.gitError||(!detail.gitLoaded?ui("讀取內容中"):(detail.gitContent?.commands||[]).join("\n")||"--"),"shell"));
      content.append(structuredPayload(ui(detail.gitContent?.output_scope==="outer_scope"?"外層工具回覆":"Git 回覆"),detail.gitError||(!detail.gitLoaded?ui("讀取內容中"):detail.gitContent?.output??"--")));
      if(detail.gitContent?.output_scope==="outer_scope")content.append(node("p",ui("此回覆來自整次工具呼叫, 可能包含其他指令的輸出"),"muted"));
      if(detail.gitContent?.truncated)content.append(node("p",ui("顯示前 32,768 字元"),"muted"));
    }
    const t=data.codex.threads.find(item=>item.thread_id===event.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
  }else if(detail.kind==="file"){
    const e=detail.event;if(detail.fileMetadataError)content.append(node("p",detail.fileMetadataError,"muted"));else if(detail.fileMetadata?.health&&detail.fileMetadata.health!=="ok")content.append(node("p",ui(detail.fileMetadata.health==="missing"?"檔案已不存在":detail.fileMetadata.health==="rejected"?"此檔案不提供本機資訊":"無法讀取檔案資訊"),"muted"));$("detail-title").textContent=ui("檔案操作 · ")+(fileLabels[e.operation]||e.operation);
    content.append(metadataList([[ui("檔案位置"),fileLocation(e)],[ui("目前檔案大小 (bytes)"),fmt(detail.fileMetadata?.bytes)],[ui("讀取量 (bytes)"),fmt(e.read_bytes)],[ui("寫入量 (bytes)"),fmt(e.write_bytes)],[ui("送出文字大小 (UTF-8 bytes)"),fmt(e.submitted_utf8_bytes)],[ui("讀寫範圍"),Object.entries(e.range||{}).map(([key,value])=>key+": "+value).join(", ")||"--"],[ui("紀錄中的路徑"),e.path],[ui("工作目錄"),e.workdir||"--"],[ui("紀錄方式"),e.nested?ui("exec 程式碼中的呼叫位置"):ui("工具呼叫")],[ui("呼叫時間"),when(e.timestamp)],[ui("工具回傳時間"),when(e.completed_at)],[ui("最後更新時間"),when(detail.fileMetadata?.modified_at)],[ui("耗時 (ms)"),fmt(e.duration_ms)],[ui("exec 整次耗時 (ms)"),fmt(e.container_duration_ms)],["Call ID",e.call_id]]));
    content.append(button(ui("查看執行內容"),()=>openDetail({kind:"tool-call",event:{...e,tool:e.tool||(threadIndex.get(e.thread_id)?.tool_events||[]).find(item=>item.call_id===e.call_id)?.tool}}),"link"));
    content.append(node("h4",ui("工具用途")));purposeBlock(content,e.tool);const t=data.codex.threads.find(t=>t.thread_id===e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
  }else if(detail.kind==="skill"){
    $("detail-title").textContent=ui("技能讀取 · ")+detail.skill;
    const events=(data.codex.skills?.events||[]).filter(event=>event.skill===detail.skill);
    if(detail.event)content.append(metadataList([[ui("時間"),when(detail.event.timestamp)],["Thread ID",detail.event.thread_id],["Call ID",detail.event.call_id]]));
    content.append(node("p",fmt(data.codex.skills?.counts?.[detail.skill]||0)+ui(" 次讀取"),"detail-total"));
    content.append(node("h4",ui("技能檔案")));
    if(!detail.event)content.append(node("p",ui("選擇讀取紀錄以查看文件"),"muted"));
    else if(!detail.documents){content.append(node("p",detail.documentError||ui("讀取文件中"),"empty"));if(!detail.documentError&&!detail.documentLoading)loadSkillDocuments(detail);}
    else if(!detail.files?.length)content.append(node("p",ui("找不到這個技能的本機文件"),"empty"));
    else{
      if(detail.root)content.append(metadataList([[ui("技能位置"),detail.root]]));
      const kinds={document:ui("文件"),python:ui("Python 腳本"),file:ui("其他檔案")};
      content.append(table([ui("更新時間"),ui("檔案"),ui("檔案位置"),ui("類型"),ui("大小 (bytes)")],detail.files.map(file=>{const row=node("tr");cell(row,when(file.modified_at));cell(row,null,"path-cell").append(button(file.relative_path,()=>openDetail({kind:"skill-file",skill:detail.skill,event:detail.event,fileInfo:file,files:detail.files,root:detail.root,documents:detail.documents,documentPath:file.relative_path}),"link mono"));cell(row,file.path,"path-cell mono");cell(row).append(tag(kinds[file.kind]||file.kind),file.format?tag(file.format.toUpperCase()):node("span"));valueCell(row,file.bytes);return row;}),ui("技能檔案")));
      if(detail.filesTruncated)content.append(node("p",ui("檔案較多, 清單顯示目前掃描到的項目"),"muted"));

    }
    content.append(node("h4",ui("近期讀取紀錄")),table([ui("時間"),ui("對話"),ui("操作")],events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(ui("查看明細"),()=>openDetail({kind:"skill",skill:event.skill,event}),"link"));return row;})));

  }else if(detail.kind==="skill-file"){
    const file=detail.fileInfo,selected=detail.documents?.find(item=>item.relative_path===detail.documentPath);$("detail-title").textContent=detail.skill+" · "+file.relative_path;
    content.append(metadataList([[ui("檔案位置"),file.path],[ui("最後更新時間"),when(selected?.modified_at||file.modified_at)],[ui("大小 (bytes)"),fmt(file.bytes)],[ui("讀取大小 (bytes)"),fmt(selected?.read_bytes)],["SHA-256",selected?.sha256||"--"]]));
    if(detail.documentLoading)content.append(node("p",ui("讀取文件中"),"empty"));else if(detail.documentError)content.append(node("p",detail.documentError,"empty"));else if(selected){content.append(documentPayload(selected.text,file.format));if(selected.truncated)content.append(node("p",ui("文件較長, 顯示前段內容"),"muted"));}else content.append(node("p",ui("此檔案提供位置與大小資訊"),"muted"));
  }else if(detail.kind==="usage-model"){
    const threads=(data.codex.threads||[]).filter(thread=>(thread.model||"unknown")===detail.model);$("detail-title").textContent=ui("模型用量分析")+" · "+(detail.model==="unknown"?ui("未知"):detail.model);
    content.append(metadataList([[ui("對話數"),fmt(threads.length)],[ui("有 Token 資料的對話"),fmt(threads.filter(thread=>Object.values(thread.tokens||{}).some(Number.isFinite)).length)]]));
    content.append(table([ui("最近活動時間"),ui("對話"),ui("專案"),...Object.values(tokenLabels)],threads.map(thread=>{const row=node("tr");cell(row,when(thread.updated_at));cell(row).append(threadLink(thread));projectCell(row,thread);for(const key of Object.keys(tokenLabels))valueCell(row,thread.tokens?.[key]);return clickableRow(row,()=>openDetail({kind:"thread",id:thread.thread_id,thread}));}),ui("對話用量"),"detail:usage:"+detail.model));
  }else if(detail.kind==="file-group"){
    $("detail-title").textContent=ui("檔案讀寫明細");content.append(metadataList([[ui("檔案位置"),detail.path],[ui("操作次數"),fmt(detail.items.length)]]),table([ui("時間"),ui("對話"),ui("專案"),ui("操作"),ui("檔案位置"),ui("工具"),ui("紀錄方式"),ui("工具回傳時間"),ui("耗時 (ms)")],fileRows(detail.items),ui("檔案操作紀錄"),"detail:file-group"));
  }else if(detail.kind==="web-reference"){
    $("detail-title").textContent=ui("網路參考明細");const items=detail.items;
    content.append(metadataList([[ui("網址或網站"),detail.key],[ui("參考次數"),fmt(items.length)],[ui("涉及對話"),fmt(new Set(items.map(item=>item.thread_id)).size)],[ui("首次參考"),when(items.map(item=>item.timestamp).sort()[0])],[ui("最近參考"),when(items.map(item=>item.timestamp).sort().at(-1))]]));
    content.append(table([ui("時間"),ui("對話"),ui("參考網址"),ui("來源"),ui("操作")],items.map(item=>{const row=node("tr");cell(row,when(item.timestamp));eventThreadCell(row,item);const link=node("a",item.url,"mono");link.href=item.url;link.target="_blank";link.rel="noopener noreferrer";cell(row,null,"path-cell").append(link);cell(row,item.reference_source);cell(row,(item.metadata?.operations||[]).join(", ")||item.tool);return clickableRow(row,()=>openDetail({kind:"mcp",event:item}));}),ui("網路活動紀錄"),"detail:web-reference"));
  }else if(detail.kind==="error"){
    const e=detail.event;$("detail-title").textContent=errorReason(e);
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("等級"),severityTag(e.severity)],[ui("類型"),errorCategories[e.category]||e.category],[ui("來源"),errorSources[e.source]||e.source],[ui("代碼"),e.code||"--"],[ui("錯誤類型"),e.error_type||"--"],[ui("來源模組"),e.module||"--"],[ui("操作"),e.method||"--"],["HTTP status",fmt(e.http_status)],[ui("命令回傳代碼"),fmt(e.exit_code)],["Thread ID",e.thread_id||"--"],["Call ID",e.call_id||"--"]]));
    if(e.content_id){content.append(node("h4",ui("錯誤內容")),codePayload(detail.errorContent?.text??detail.errorContentError??ui(detail.errorLoaded?"來源內容已無法取得":"讀取內容中")));}
    const causeNames=editableLabels({rate_limit:"請求超過速率限制",quota_exceeded:"額度不足",authentication_failed:"認證失敗",permission_denied:"權限不足",connection_refused:"連線遭拒",timeout:"請求逾時",stream_interrupted:"回應串流中斷",invalid_response:"回應格式無效"});
    content.append(node("h4",ui("錯誤內容與追蹤")),metadataList([[ui("失敗原因"),causeNames[e.cause]||errorReason(e)],[ui("來源檔案"),e.file||"--"],[ui("紀錄 ID"),fmt(e.record_id)],["Request ID",e.request_id||"--"],["Trace ID",e.trace_id||"--"],[ui("重試次數"),fmt(e.attempt)],[ui("紀錄保留方式"),ui("最近 24 小時摘要")]]));
    const trace=Object.fromEntries(Object.entries(e).filter(([key])=>!["thread_name","project_name"].includes(key))),copy=button(ui("複製追蹤資料"),async()=>{try{await navigator.clipboard.writeText(JSON.stringify(trace,null,2));feedback("action-message",ui("追蹤資料已複製"));}catch{feedback("action-message",ui("剪貼簿無法寫入"),"error");}});content.append(copy);
    const logSource=(data.logs?.sources||[]).find(source=>source.source===e.source);if(logSource)content.append(button(ui("查看來源狀態"),()=>openDetail({kind:"log-source",source:logSource})));
    if(e.tool){const full=e.server?"mcp__"+e.server+"__"+e.tool:e.tool;content.append(node("h4",ui("工具用途")));purposeBlock(content,full);}
    const t=threadIndex.get(e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
    const mcp=(data.mcp?.events||[]).find(item=>item.thread_id===e.thread_id&&item.call_id===e.call_id&&item.server===e.server);if(mcp)content.append(button(ui("查看 MCP 操作"),()=>openDetail({kind:"mcp",event:mcp})));
  }else if(detail.kind==="mcp-source"){
    const source=(data.mcp?.servers||[]).find(item=>item.server===detail.server),events=(data.mcp?.events||[]).filter(event=>event.server===detail.server);$("detail-title").textContent=detail.server;
    if(source){
      const head=node("div",null,"source-detail-head"),tags=node("div",null,"tag-line");tags.append(...mcpTags(source,events),connectionBadge(source),tag(source.enabled?source.origin==="configured"?ui("已設定"):ui("有紀錄"):ui("已關閉"),source.enabled?"configured":"disabled"));
      head.append(tags,button(ui("本地文件與設定"),()=>{const server=detail.server;$("detail-dialog").close();mcpSection="files";navigateToMcpSource(server);},"link"),button(ui("查看來源頁面"),()=>{const server=detail.server;$("detail-dialog").close();navigateToMcpSource(server);},"source-open"));
      content.append(node("p",source.description||ui("來源尚未提供說明"),"tool-purpose"));content.append(metadataList([[ui("設定檔位置"),data.mcp.configuration?.location||"--"],[ui("說明檔案位置"),source.description_file||"--"]]));if(source.description_source)content.append(metadataList([[ui("說明來源"),source.description_source==="config"?ui("Codex 設定"):source.description_source]]));
      content.append(metadataList([[ui("工具呼叫"),fmt(source.calls)],[ui("exec 辨識"),fmt(source.recognized)],[ui("已提供結果"),fmt(source.known_status)],...source.known_status?[[ui("錯誤"),fmt(source.errors)]]:[]]),head);
      const times=[];if(source.connection?.last_response_at)times.push([ui("最後回應"),when(source.connection.last_response_at)]);if(source.connection?.pending_calls)times.push([ui("等待回應"),fmt(source.connection.pending_calls)]);if(times.length)content.append(metadataList(times));
    }
    if(data.mcp?.telemetry?.[detail.server])content.append(telemetryPanel(detail.server,data.mcp.telemetry[detail.server]));
    const metrics=mcpMetrics(events,"detail:source:"+detail.server);if(metrics)content.append(metrics);if(events.length)content.append(node("h4",ui("操作紀錄")),table([ui("時間"),ui("工具"),ui("結果"),ui("耗時 (ms)"),ui("操作摘要")],events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,event.tool,"mono");cell(row,labels.status[event.result?.status]||event.result?.status||"--");valueCell(row,event.duration_ms);cell(row,mcpBrief(event));return clickableRow(row,()=>openDetail({kind:"mcp",event}));}),ui("操作紀錄"),"detail:source:"+detail.server+":events"));
  }else if(detail.kind==="mcp-file"){
    renderMcpDocument(content,detail);
  }else if(detail.kind==="mcp"){
    const e=detail.event,full=e.server==="web"?"web__run":"mcp__"+e.server+"__"+e.tool;
    $("detail-title").textContent=e.server+" · "+e.tool;purposeBlock(content,full);
    content.append(metadataList([[ui("來源"),e.server],[ui("分類"),ui(data.mcp.categories[e.category])||e.category],[ui("紀錄方式"),e.nested?ui("exec 程式碼中的呼叫位置"):ui("工具呼叫")],[ui("時間"),when(e.timestamp)],[ui("回傳時間"),when(e.completed_at)],[ui("耗時 (ms)"),fmt(e.duration_ms)],[ui("exec 整次耗時 (ms)"),fmt(e.container_duration_ms)],[ui("結果"),labels.status[e.result?.status]||e.result?.status||ui("未知")],["Call ID",e.call_id]]));
    const source=(data.mcp?.servers||[]).find(item=>item.server===e.server);if(source){content.append(node("h4",ui("來源使用摘要")),metadataList([[ui("工具呼叫"),fmt(source.calls)],[ui("exec 辨識"),fmt(source.recognized)],[ui("工具已回傳"),fmt(source.returned)],[ui("已提供結果"),fmt(source.known_status)],...source.known_status?[[ui("錯誤"),fmt(source.errors)]]:[],...Number.isFinite(source.average_ms)?[[ui("平均耗時 (ms)"),fmt(source.average_ms)]]:[],...Number.isFinite(source.p99_ms)?[["P99 (ms)",fmt(source.p99_ms)]]:[],[ui("最近活動"),when(source.last_at)]]));const report=data.mcp.telemetry?.[e.server];if(report)content.append(telemetryPanel(e.server,report,"summary"));}
    const names={format:ui("文件格式"),ocr_mode:ui("OCR 模式"),write:ui("寫入設定"),write_output:ui("寫出文件"),max_files:ui("檔案數量上限"),written:ui("已寫入"),truncated:ui("內容截斷"),content_chars:ui("文字長度 (字元)"),ocr_status:ui("OCR 結果"),ocr_eligible_items:ui("待處理 OCR 項目"),ocr_processed_items:ui("已處理 OCR 項目"),ocr_omitted_items:ui("省略 OCR 項目"),ocr_errors:ui("OCR 錯誤數"),changed_files:ui("差異檔案數"),only_in_source_files:ui("只在來源的檔案數"),only_in_target_files:ui("只在目標的檔案數"),evidence_runs:ui("驗證紀錄數"),checks_passed:ui("檢查通過數"),checks_failed:ui("檢查失敗數"),checks_skipped:ui("略過檢查數"),findings:ui("發現項目數"),findings_critical:"Critical",findings_high:"High",findings_medium:"Medium",findings_low:"Low",retries:ui("重試次數"),http_attempts:ui("HTTP 嘗試次數"),input_tokens:"Input tokens",output_tokens:"Output tokens"};
    const selected={...e.metadata,...e.result},items=Object.entries(selected).filter(([key,v])=>!["references","resources","status"].includes(key)&&["string","boolean","number"].includes(typeof v)).map(([key,v])=>[names[key]||key,typeof v==="boolean"?(v?ui("是"):ui("否")):typeof v==="number"?fmt(v):labels.status[v]||v]);
    content.append(node("h4",ui("輸入與輸出")));if(detail.mcpError)content.append(node("p",detail.mcpError,"empty"));else if(!detail.mcpLoaded)content.append(node("p",ui("讀取內容中"),"empty"));else{content.append(structuredPayload(ui("送出內容"),detail.mcpContent?.request??ui("本機紀錄未提供可解析的輸入")),structuredPayload(ui(detail.mcpContent?.response_scope==="containing_tool_call"?"外層工具回覆":"MCP 回傳"),detail.mcpContent?.response??ui("本機紀錄未提供回傳內容")));if(detail.mcpContent?.response_scope==="containing_tool_call")content.append(node("p",ui("此回覆來自整次工具呼叫, 可能包含其他指令的輸出"),"muted"));if(detail.mcpContent?.truncated)content.append(node("p",ui("顯示內容已達長度上限"),"muted"));}
    if(items.length)content.append(node("h4",ui("操作摘要")),metadataList(items));
    const ids=Object.entries(e.metadata?.resources||{});if(ids.length)content.append(node("h4",ui("資源 ID")),metadataList(ids));
    const t=data.codex.threads.find(t=>t.thread_id===e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
    const urls=[...new Set([...(e.metadata?.references||[]),...(e.result?.references||[])])];if(urls.length){content.append(node("h4",ui("參考網址")+" ("+fmt(urls.length)+ui(" 個")+")"));for(const url of urls)content.append(referenceLink(url));}
  }
  if(detail.kind==="source-record"){const {server,record}=detail;$("detail-title").textContent=server+" · "+ui("來源紀錄");content.append(metadataList(telemetryFields(record).map(([key,value])=>[metricLabel(key),telemetryValue(value)])));for(const [key,value]of Object.entries(record))if(Array.isArray(value)&&value.length){const fields=[...new Set(value.flatMap(item=>telemetryFields(item).map(([key])=>key)))];content.append(node("h4",metricLabel(key)),table(fields.map(metricLabel),value.map(item=>{const row=node("tr");for(const field of fields)cell(row,telemetryValue(item[field]));return row;}),metricLabel(key),"source:"+server+":"+key));}}
  if(detail.kind==="log"){
    const e=detail.event;$("detail-title").textContent=logMessage(e);
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("等級"),severityTag(e.severity)],[ui("來源"),logSource(e.source)],[ui("模組"),e.module||"--"],[ui("代碼"),e.code||"--"],[ui("操作"),e.method||e.tool||ui("未知")],[ui("錯誤類型"),e.error_type||"--"],[ui("檔案"),e.file||"--"],[ui("紀錄 ID"),fmt(e.record_id)],["Thread ID",e.thread_id||"--"],["HTTP status",fmt(e.http_status)]]));
    if(e.thread_id)content.append(threadLink(threadIndex.get(e.thread_id)||{...e,metadata_only:true,event_only:true},ui("查看對話")));
    if(e.content_id)content.append(node("h4",ui("紀錄內容")),codePayload(detail.errorContent?.text??detail.errorContentError??ui(detail.errorLoaded?"來源內容已無法取得":"讀取內容中")));content.append(structuredPayload(ui("紀錄欄位"),e));
  }
  if(detail.kind==="sqlite"){
    const e=detail.event;$("detail-title").textContent=ui("SQL 操作")+" · "+e.statement;
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("SQL 操作"),e.statement],[ui("操作類型"),sqlLabels[e.operation]||e.operation],[ui("資料庫引擎"),e.engine],[ui("資料庫位置"),sqlLocation(e)||ui("未知")],[ui("工作目錄"),e.workdir||"--"],[ui("紀錄方式"),sqlMethod(e)],[ui("工具"),e.tool||ui("未知")],[ui("工具回傳狀態"),sqlResults[e.result]||e.result],[ui("工具整次耗時 (ms)"),fmt(e.container_duration_ms)],[ui("SQL 耗時 (ms)"),fmt(e.duration_ms)],[ui("影響列數"),fmt(e.rows_affected)],[ui("回傳列數"),fmt(e.rows_returned)],[ui("來源"),e.source?logSource(e.source):ui("對話紀錄")],[ui("模組"),e.module||"--"],["Call ID",e.call_id||"--"]]),node("h4",ui("SQL 內容")),codePayload(detail.sqlError||(!detail.sqlLoaded?ui("讀取內容中"):detail.sqlContent?.sql??"--"),"sql"));if(detail.sqlContent?.truncated)content.append(node("p",ui("顯示前 32,768 字元"),"muted"));
    if(e.thread_id)content.append(threadLink(threadIndex.get(e.thread_id)||{...e,event_only:true,metadata_only:true},ui("查看對話")));
  }
  if(detail.kind==="log-source"){
    const s=(logData?.sources||data.logs?.sources||[]).find(source=>source.source===detail.source.source)||detail.source;$("detail-title").textContent=logSource(s.source)+ui(" · 資料來源");
    const hints=editableLabels({missing:"目前未找到來源檔案, 來源產生紀錄後會自動讀取",config_missing:"尚未設定此來源, 請先確認來源紀錄已啟用",unsupported:"來源資料格式與目前可讀取的欄位不相符",unavailable:"目前無法存取來源, 請檢查檔案權限或資料庫使用狀態",partly_unavailable:"部分來源無法讀取, 其餘可讀取的紀錄會繼續更新",disabled:"此來源已停用, 可從 Tab 或總體設定啟用"});
    content.append(node("p",hints[s.health]||ui("來源已取得的欄位與目前讀取範圍如下")),metadataList([[ui("狀態"),logHealthLabels[s.health]||s.health],[ui("最近檢查"),when(s.checked_at)],[ui("錯誤類型"),s.error_type||ui("無")],[ui("已讀取 (行)"),fmt(s.read_lines)],[ui("已辨識 (行)"),fmt(s.parsed_lines)],[ui("未辨識 (行)"),fmt(s.unsupported_lines)],[ui("過長紀錄 (行)"),fmt(s.oversized_lines)],[ui("歷史回補"),s.backfill_pending==null?ui("不適用"):s.backfill_pending?ui("分批回補中"):ui("已完成目前來源回補")],[ui("每輪讀取上限"),s.read_limit==null?ui("未知"):fmt(s.read_limit)+" "+s.read_unit],[ui("保存大小 (bytes)"),fmt(s.bytes)],[ui("保存上限 (bytes)"),fmt(s.byte_limit)]]));
    if(s.files?.length)content.append(table([ui("檔案"),ui("已讀取位置 (bytes)"),ui("待完成片段 (bytes)"),ui("紀錄 ID")],s.files.map(file=>{const row=node("tr");cell(row,file.name,"mono");valueCell(row,file.offset);valueCell(row,file.pending_bytes);valueCell(row,file.record_id);return row;}),ui("Log 來源檔案")));
    if(s.source==="monitor")content.append(node("p",ui("程式事件保存於 CODEX_HOME/monitoring/local-activity-monitor.jsonl, 最多保留目前與上一份檔案. 效能樣本與本輪狀態事件仍保存在記憶體"),"muted"));
    if(s.source==="session"||s.source==="jev_telemetry")content.append(button(ui("查看完整操作紀錄"),()=>{$("detail-dialog").close();navigateToTab(s.source==="session"?"codex":"jev",s.source==="session"?"view-codex":"mcp-telemetry");},"link"));
  }
  for(const time of $("detail-dialog").querySelectorAll(".detail-time"))time.remove();
  const timestamp=detail.kind==="mcp-source"?(data.mcp?.servers||[]).find(source=>source.server===detail.server)?.last_at:detail.event?.timestamp||detail.thread?.updated_at||detail.record?.timestamp;
  if(timestamp){const time=node("time",ui(detail.kind==="mcp-source"||detail.kind==="thread"?"最近活動":"時間")+": "+when(timestamp),"detail-time");time.dateTime=timestamp;$("detail-title").after(time);}
  const descriptions=detail.kind==="thread"?[]:[...content.children].filter(child=>child.matches("p:not([role=status]),.tool-purpose,.purpose-block"));for(const paragraph of [...descriptions].reverse())content.prepend(paragraph);
  const actions=[...content.children].filter(child=>child.tagName==="BUTTON"||child.tagName==="A");
  if(actions.length&&detail.kind!=="mcp-file"){const row=node("div",null,"detail-actions");for(const action of actions)row.append(action);if(descriptions.length)descriptions.at(-1).after(row);else content.prepend(row);}
  if(detail.kind==="thread"){const children=(data.codex.threads||[]).filter(thread=>thread.execution?.parent_thread_id===detail.thread.thread_id);if(children.length)content.append(node("h4",ui("子代理程式")),node("p",ui("顯示已載入且有上層 Thread ID 的子代理程式, 狀態取自來源最近紀錄"),"muted"),table(subagentHeaders(),subagentRows(children),ui("子代理程式")));groupThreadDetails(content);}
  attachTables();
  if(restore){[...content.querySelectorAll("details")].forEach((el,index)=>{if(folds[index]!=null)el.open=folds[index];});[...content.querySelectorAll(".payload,.table-wrap")].forEach((el,index)=>{if(payloadScroll[index]){el.scrollTop=payloadScroll[index][0];el.scrollLeft=payloadScroll[index][1];}});if(focus>=0)content.querySelectorAll("details > summary")[focus]?.focus({preventScroll:true});content.scrollTop=scroll;}
}
async function loadErrorContent(selected){selected.errorLoading=true;try{selected.errorContent=await lazyContent("/api/codex/error",{id:selected.event.content_id});}catch{selected.errorContentError=ui("錯誤內容讀取失敗");}finally{selected.errorLoading=false;selected.errorLoaded=true;if(detail===selected)renderDetail();}}
async function loadProjectContent(selected){selected.projectLoading=true;try{const result=await request("/api/codex/project?project_id="+encodeURIComponent(selected.project.id),{cache:"no-store"});if(!result.ok)throw new Error();selected.projectContent=result.data;}catch{selected.projectError=ui("專案設定讀取失敗");}finally{selected.projectLoading=false;selected.projectLoaded=true;if(detail===selected)renderDetail();}}
async function loadFileMetadata(selected){selected.fileMetadataLoading=true;try{const query=new URLSearchParams({thread_id:selected.event.thread_id,call_id:selected.event.call_id,path:selected.event.path}),result=await request("/api/codex/file?"+query);if(!result.ok)throw new Error();selected.fileMetadata=result.data;}catch{selected.fileMetadataError=ui("無法讀取檔案資訊");}finally{selected.fileMetadataLoading=false;if(detail===selected)renderDetail();}}
async function loadInstructions(selected){selected.instructionsLoading=true;try{const query=new URLSearchParams({scope:selected.scope});if(selected.project)query.set("project_id",selected.project.id);const result=await request("/api/codex/instructions?"+query,{cache:"no-store"});if(!result.ok)throw new Error();selected.instructionsContent=result.data;}catch{selected.instructionsError=ui("指示文件讀取失敗");}finally{selected.instructionsLoading=false;selected.instructionsLoaded=true;if(detail===selected)renderDetail();}}
async function loadSqlContent(selected){
  selected.sqlLoading=true;const mask=preferences.sqlMasking!==false,token={};selected.sqlLoadToken=token;
  try{const result=await lazyContent("/api/codex/sql",{id:selected.event.id,mask:mask?"1":"0"});if(selected.sqlLoadToken!==token)return;selected.sqlContent=result;selected.sqlLoaded=true;}
  catch(error){if(selected.sqlLoadToken===token)selected.sqlError=error.message||ui("SQL 內容無法讀取");}
  finally{if(selected.sqlLoadToken!==token)return;selected.sqlLoading=false;if(detail===selected){const scroll=$("detail-content").scrollTop;renderDetail();$("detail-content").scrollTop=scroll;}}
}
const mcpDocumentDrafts=new Map();
async function loadMcpContent(selected){selected.mcpLoading=true;try{const event=selected.event;selected.mcpContent=await lazyContent("/api/codex/mcp",{thread_id:event.thread_id,call_id:event.call_id,index:event.index??0});selected.mcpLoaded=true;}catch{selected.mcpError=ui("MCP 輸入與輸出無法讀取");}finally{selected.mcpLoading=false;if(detail===selected){const scroll=$("detail-content").scrollTop;renderDetail();$("detail-content").scrollTop=scroll;}}}
async function loadMcpFiles(selected){selected.mcpFilesLoading=true;try{const response=await request("/api/mcp/files?"+new URLSearchParams({server:selected.server}),{cache:"no-store"});if(!response.ok)throw new Error();selected.mcpFiles=response.data.files;}catch{selected.mcpFilesError=ui("來源沒有可讀取的本地文件");}finally{selected.mcpFilesLoading=false;if(detail===selected)renderDetail();else if(mcpSource===selected.server&&mcpSection==="files")renderMcp();}}
function appendMcpFiles(content,selected){
  const list=node("div",null,"mcp-file-list");content.append(list);
  if(selected.mcpFilesError||!selected.mcpFiles){list.append(node("p",selected.mcpFilesError||ui("讀取內容中"),"muted"));return;}
  if(!selected.mcpFiles.length){list.append(node("p",ui("來源沒有可讀取的本地文件"),"muted"));return;}
  const head=node("div",null,"mcp-file-head");head.append(node("span",ui("檔案")),node("span",ui("最後更新時間")),node("span",ui("權限")));list.append(head);
  for(const file of selected.mcpFiles){const row=node("div",null,"detail-row"),time=node("time",when(file.modified_at));time.dateTime=file.modified_at;const label=node("div"),location=node("p",file.path,"mcp-file-path mono");bindHelp(location,file.path);label.append(button(file.name,()=>openDetail({kind:"mcp-file",server:selected.server,file}),"link"),location);row.append(label,time,tag(ui(file.editable?"可編輯":"唯讀"),file.editable?"configured":"observed"));list.append(row);}
}
async function loadMcpDocument(selected,discard=false){selected.documentLoading=true;selected.documentError=null;try{const response=await request("/api/mcp/files?"+new URLSearchParams({server:selected.server,document:selected.file.id}),{cache:"no-store"});if(!response.ok||!response.data||response.data.health==="unavailable")throw new Error();selected.documentContent=response.data;const key=selected.server+":"+selected.file.id;selected.draft=discard?response.data.text:mcpDocumentDrafts.get(key)??response.data.text;if(discard)mcpDocumentDrafts.delete(key);}catch{selected.documentError=ui("文件讀取失敗");}finally{selected.documentLoading=false;if(detail===selected)renderDetail();}}
function renderMcpDocument(content,selected){
  $("detail-title").textContent=selected.server+" · "+selected.file.name;const file=selected.documentContent;
  if(selected.documentError||!file){content.append(node("p",selected.documentError||ui("讀取內容中"),"empty"));return;}
  content.append(metadataList([[ui("檔案位置"),file.path],[ui("最後更新時間"),when(file.modified_at)],[ui("讀取大小 (bytes)"),fmt(file.bytes)]]));
  if(file.parsed_format){content.append(structuredPayload(ui("結構化預覽")+" · "+file.parsed_format,file.parsed));if(file.parsed_truncated)content.append(node("p",ui("顯示內容已達長度上限"),"muted"));}
  if(file.validation==="basic")content.append(node("p",ui("此環境未提供 YAML 解析器, 儲存時檢查基本縮排"),"muted"));
  if(!file.editable){content.append(documentPayload(file.text||"--",file.format));if(file.masked)content.append(node("p",ui("含有敏感欄位, 已遮蔽並以唯讀顯示"),"muted"));if(file.truncated)content.append(node("p",ui("顯示內容已達長度上限"),"muted"));return;}
  const editor=node("textarea",null,"mcp-document-editor"),actions=node("div",null,"settings-actions"),message=node("p");editor.value=selected.draft??file.text;editor.maxLength=32768;editor.spellcheck=false;const preview=structuredPayload(ui("內容預覽"),selected.draft??file.text);preview.addEventListener("toggle",()=>{if(preview.open){const body=preview.querySelector(".structured-payload-body");body.replaceChildren(documentPayload(editor.value,file.format));}});editor.setAttribute("aria-label",ui("檔案內容"));message.setAttribute("role","status");message.setAttribute("aria-live","polite");
  const save=button(ui("儲存修改"),async()=>{save.disabled=true;reload.disabled=true;try{const result=await post("/api/mcp/file",{server:selected.server,document:file.id,sha256:file.sha256,text:editor.value});selected.documentContent=result;selected.draft=result.text;mcpDocumentDrafts.delete(selected.server+":"+file.id);if(detail===selected)renderDetail();feedback("action-message",ui("檔案已儲存"));}catch(error){message.textContent=error.message;}finally{save.disabled=false;reload.disabled=false;}}),reload=button(ui("重新讀取"),()=>{const load=()=>loadMcpDocument(selected,true);if(editor.value!==file.text)confirmChange("重新讀取文件?","尚未儲存的修改會由檔案目前內容取代",load,ui("重新讀取"));else load();});
  editor.addEventListener("input",()=>{selected.draft=editor.value;mcpDocumentDrafts.set(selected.server+":"+file.id,editor.value);if(mcpDocumentDrafts.size>20)mcpDocumentDrafts.delete(mcpDocumentDrafts.keys().next().value);save.disabled=editor.value===file.text;message.textContent=editor.value===file.text?"":ui("有未儲存的修改");});save.disabled=editor.value===file.text;actions.append(save,reload);content.append(preview,editor,actions,message,node("p",ui("設定生效方式依來源 MCP, 部分來源需重新啟動"),"muted"));
}

async function loadGitContent(selected){
  selected.gitLoading=true;if(detail===selected)renderDetail();
  try{const event=selected.event;selected.gitContent=await lazyContent("/api/codex/git",{thread_id:event.thread_id,call_id:event.call_id,operation:event.operation});selected.gitLoaded=true;}
  catch(error){selected.gitError=error.message||ui("Git 明細無法讀取");}
  finally{selected.gitLoading=false;if(detail===selected){const scroll=$("detail-content").scrollTop;renderDetail();$("detail-content").scrollTop=scroll;}}
}
async function loadSkillDocuments(selected,path){
  const token={};selected.documentRequest=token;selected.documentLoading=true;selected.documentError=null;if(path)selected.documentPath=path;if(detail===selected)renderDetail();
  try{
    const response=await request("/api/codex/skill?"+new URLSearchParams({skill:selected.skill,...selected.event?{thread_id:selected.event.thread_id,call_id:selected.event.call_id}:{},...path?{file:path}:{}}),{cache:"no-store"});if(!response.ok)throw new Error(ui("技能文件檢查已關閉"));if(selected.documentRequest!==token)return;
    selected.files=response.data.files;selected.root=response.data.root;selected.filesTruncated=response.data.files_truncated;
    const documents=response.data.documents;selected.documents=path?[...(selected.documents||[]).filter(file=>file.relative_path!==path),...documents]:documents;
    if(!path)selected.documentPath=(documents.find(file=>file.name.toLowerCase()==="readme.md")||documents[0])?.relative_path;
    if(path&&!documents.length)selected.documentError=ui("文件已移動或無法讀取, 請重新開啟技能");
  }catch(error){if(selected.documentRequest===token)selected.documentError=error.message||ui("文件讀取失敗");}finally{if(selected.documentRequest===token){selected.documentLoading=false;if(detail===selected)renderDetail();}}
}
async function openJev(call,thread,remember=true,scroll=0){
  if(remember)rememberDetail();clearDetailTabs();updateDetailBack();
  const selected={kind:"jev",call,thread};detail=selected;$("detail-title").textContent="Jev "+call.operation+ui(" · 送出 / 回傳");$("detail-content").replaceChildren(node("p",ui("讀取內容中"),"empty"));
  if(!$("detail-dialog").open)openDialog($("detail-dialog"));
  try{
    const query=new URLSearchParams({thread:call.thread_id,call:call.call_id,index:call.index});
    const result=await lazyContent("/api/codex/jev",Object.fromEntries(query));if(detail!==selected)return;
    const content=$("detail-content");content.replaceChildren();
    const t=thread||data.codex.threads.find(item=>item.thread_id===call.thread_id);if(t){const actions=node("div",null,"detail-actions");actions.append(threadLink(t,ui("返回對話 · ")+title(t)));content.append(actions);}
    content.append(metadataList([[ui("呼叫時間"),when(call.timestamp)],[ui("回傳時間"),when(call.completed_at)],[ui("工具 Call ID"),call.call_id]]));
    content.append(structuredPayload(ui("送出至 Jev"),result.request==null?ui("本機紀錄未提供獨立的 Jev request"):result.request));
    content.append(structuredPayload(result.response_scope==="containing_tool_call"?ui("包含 Jev 的工具呼叫回傳"):ui("Jev 回傳"),result.response==null?ui("本機紀錄未提供回傳內容"):result.response));
  }catch(error){if(detail===selected)$("detail-content").replaceChildren(node("p",error.message||ui("內容讀取失敗"),"empty"));}
  finally{if(detail===selected)$("detail-content").scrollTop=scroll;}
}
document.addEventListener("visibilitychange",()=>{if(!document.hidden)refresh();});
function syncSessionTracking(){const all=!!data?.settings?.track_all;if(!settingsBusy)$("track-all").checked=all;const disabled=all||settingsBusy||$("track-all").disabled;$("session-count").disabled=disabled;$("apply-session-count").disabled=disabled;}
function syncIdleSettings(){if(document.activeElement!==$("activity-retention-days"))$("activity-retention-days").value=data?.settings?.activity_retention_days??7;const minutes=data?.settings?.idle_minutes??5;$("idle-pause").checked=minutes>0;$("idle-minutes").disabled=!minutes;$("apply-idle").disabled=!minutes;if(document.activeElement!==$("idle-minutes"))$("idle-minutes").value=minutes||5;}
function syncRefreshState(activity){idlePaused=activity?.paused===true;$("resume-refresh").hidden=!idlePaused;$("refresh-note").textContent=idlePaused?ui("Codex 閒置, 更新已暫停"):ui("每 ")+refreshSeconds+ui(" 秒更新");bindHelp($("refresh-note"),ui(idlePaused?"偵測到新的對話或活動後會自動恢復, 也可手動立即更新":"依設定的頻率更新監測資料"));}
function schedule(seconds){refreshSeconds=seconds;const delay=idlePaused?Math.min(30,Math.max(10,seconds)):seconds;if(refreshTimer&&scheduledSeconds===delay)return;scheduledSeconds=delay;if(refreshTimer)clearInterval(refreshTimer);refreshTimer=setInterval(()=>{if(!document.hidden)automaticRefresh();},delay*1000);syncRefreshState(data?.activity);}
async function automaticRefresh(){
  if(!idlePaused)return refresh();
  if(activityBusy||busy)return;activityBusy=true;
  try{const response=await request("/api/activity",{cache:"no-store"});if(!response.ok)throw new Error();const activity=response.data;if(data)data.activity=activity;syncRefreshState(activity);schedule(refreshSeconds);$("live").textContent=ui("已連線");renderConnectionStatus();if(!activity.paused||loadedRevision&&!loadedRevision.startsWith(activity.code_revision+"-"))await refresh();}
  catch{$("live").textContent=ui("連線中斷");renderConnectionStatus(true);}finally{activityBusy=false;}
}
async function request(url,options){const control=new AbortController(),timer=setTimeout(()=>control.abort(),15000);try{const response=await fetch(url,{...options,signal:control.signal}),body=await response.text();return {ok:response.ok,status:response.status,data:response.headers.get("Content-Type")?.includes("application/json")?JSON.parse(body):body};}finally{clearTimeout(timer);}}
function compatibleSettings(saved,current,categories){
  const next={};for(const [key,min,max]of [["interval",1,3600],["idle_minutes",0,1440],["activity_retention_days",1,365],["max_files",1,5000]])if(Number.isInteger(saved[key])&&saved[key]>=min&&saved[key]<=max)next[key]=saved[key];if(typeof saved.track_all==="boolean")next.track_all=saved.track_all;
  const observations=Object.fromEntries(Object.entries(saved.observations||{}).filter(([key,value])=>Object.hasOwn(current.observations,key)&&typeof value==="boolean"));if(Object.keys(observations).length)next.observations=observations;
  for(const key of ["mcp_sources","mcp_categories","tool_descriptions","mcp_descriptions","mcp_tags"]){const entries=Object.entries(saved[key]||{}).filter(([name,value])=>["tool_descriptions","mcp_descriptions"].includes(key)?/^[a-zA-Z0-9_.:-]{1,160}$/.test(name)&&typeof value==="string"&&value.length<=400:/^[a-zA-Z0-9_.-]{1,80}$/.test(name)&&(key==="mcp_sources"?typeof value==="boolean":key==="mcp_tags"?Array.isArray(value)&&value.length<=4&&value.every(text=>typeof text==="string"&&text.trim().length<=40&&text.trim()):typeof value==="string"&&Object.hasOwn(categories,value))).slice(0,64);if(entries.length)next[key]=Object.fromEntries(entries);}
  return next;
}
async function refresh(){
  if(busy){refreshQueued=true;return;}busy=true;const current=version;
  try{
    const url="/api/snapshot?window="+encodeURIComponent(activeSourceWindow());
    let response=await request(url,{cache:"no-store"});if(!response.ok)throw new Error();let next=response.data;
    if(loadedRevision&&next.revision&&loadedRevision!==next.revision){
      if(pendingRevision!==next.revision){pendingRevision=next.revision;$("live").textContent=ui("程式已更新, 準備重新載入");return;}
      const ready=await request("/",{cache:"no-store"});if(!ready.ok)throw new Error();
      saveView();location.reload();return;
    }
    loadedRevision=next.revision||loadedRevision;
    if(!preferencesApplied){
      if(preferences.settings){const saved=Object.fromEntries(Object.entries(compatibleSettings(preferences.settings,next.settings,next.mcp?.categories||{})).filter(([key,value])=>JSON.stringify(value)!==JSON.stringify(next.settings[key])));if(Object.keys(saved).length){try{await post("/api/settings",saved);}catch(error){if(![400,409].includes(error.status))throw error;preferences.settings=null;}response=await request(url,{cache:"no-store"});if(!response.ok)throw new Error();next=response.data;}}
      preferencesApplied=true;
    }
    if(current===version)render(next);
  }
  catch{$("live").textContent=ui("連線中斷");renderConnectionStatus(true);}finally{busy=false;if(refreshQueued){refreshQueued=false;refresh();}}
}
async function post(url,value){let response;try{response=await request(url,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(value)});}catch{throw new Error(ui("設定格式無效或連線中斷"));}if(!response.ok){const error=new Error(ui(response.data?.error||"設定更新失敗"));error.status=response.status;throw error;}return response.data;}
async function changeSettings(value){
  if(settingsBusy)throw new Error(ui("設定更新中"));
  settingsBusy=true;version++;feedback("settings-message",ui("正在套用設定"),"pending");
  try{const next=await post("/api/settings",value);data.settings=next;schedule(next.interval);$("refresh-interval").value=next.interval;syncSessionTracking();saveView();feedback("settings-message",value.interval!=null?ui("更新頻率已套用: ")+next.interval+ui(" 秒"):value.max_files!=null?ui("追蹤數量已套用: ")+next.max_files:ui("設定已套用"));}
  catch(error){feedback("settings-message",error.message,"error");throw error;}
  finally{settingsBusy=false;await refresh();}
}
async function applyFrequency(){
  if(settingsBusy)return;
  const input=$("refresh-interval");roundNumber(input);const seconds=Number(input.value);if(!input.checkValidity()||!Number.isInteger(seconds)){feedback("settings-message",ui("更新頻率請輸入 1 - 3600 秒的整數"),"error");input.value=refreshSeconds;return;}
  input.disabled=true;try{await changeSettings({interval:seconds});}catch{input.value=refreshSeconds;}finally{input.disabled=false;}
}
$("refresh-interval").addEventListener("change",applyFrequency);
$("refresh-interval").addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();applyFrequency();}});
$("apply-frequency").addEventListener("click",applyFrequency);
async function applyIdle(){
  if(settingsBusy||!data)return;const input=$("idle-minutes");roundNumber(input);if($("idle-pause").checked&&(!input.checkValidity()||!Number.isInteger(Number(input.value)))){feedback("settings-message",ui("閒置時間請輸入 1 - 1440 分鐘的整數"),"error");syncIdleSettings();return;}
  try{await changeSettings({idle_minutes:$("idle-pause").checked?Number(input.value):0});}catch{syncIdleSettings();}
}
$("idle-pause").addEventListener("change",applyIdle);
$("idle-minutes").addEventListener("change",applyIdle);
$("idle-minutes").addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();applyIdle();}});
$("apply-idle").addEventListener("click",applyIdle);
async function applyActivityRetention(){
  if(settingsBusy||!data)return;const input=$("activity-retention-days");roundNumber(input);
  if(!input.checkValidity()||!Number.isInteger(Number(input.value))){feedback("settings-message",ui("保留天數請輸入 1 - 365 的整數"),"error");syncIdleSettings();return;}
  try{await changeSettings({activity_retention_days:Number(input.value)});}catch{syncIdleSettings();}
}
$("apply-activity-retention").addEventListener("click",applyActivityRetention);
$("activity-retention-days").addEventListener("change",applyActivityRetention);
$("activity-retention-days").addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();applyActivityRetention();}});
$("resume-refresh").addEventListener("click",async()=>{const control=$("resume-refresh");control.disabled=true;try{await post("/api/refresh",{});await refresh();}catch{feedback("action-message",ui("資料更新失敗, 請稍後再試"),"error");}finally{control.disabled=false;}});
$("track-all").addEventListener("change",async()=>{const input=$("track-all"),value=input.checked;input.disabled=true;syncSessionTracking();try{await changeSettings({track_all:value});}catch{input.checked=!value;}finally{input.disabled=false;syncSessionTracking();}});
function renderObservationSettings(){
  const monitoring=$("observation-data"),mcp=$("observation-mcp"),recording=$("observation-recording");monitoring.replaceChildren();mcp.replaceChildren();recording.replaceChildren();
  const purposes={codex:"讀取對話狀態與 Token 紀錄",metadata:"補充對話名稱, 專案與執行設定",usage:"讀取帳戶額度與 Credits 快照",tool_events:"辨識工具呼叫與回傳時間",web:"整理網路操作與參考頁面",files:"辨識檔案讀寫操作",git:"辨識 Git 操作",skills:"保存技能讀取紀錄",checks:"辨識 test, build 與 lint 操作",sqlite:"辨識 SQL 操作與 SQLite 診斷",errors:"整理錯誤明細與最近 24 小時紀錄",logs:"讀取來源 Log 與執行紀錄",mcp:"辨識 MCP 操作與結果",jev:"讀取 Jev 用量與耗時",jev_calls:"讀取指定 Jev 呼叫的送出與回傳內容"};
  function observation(key,parent){if(data.settings.observations[key]==null||(key.startsWith("jev")&&!data.availability?.jev))return;const row=node("div",null,"setting-row"),input=node("input"),name=node("span",observationLabels[key]||key);input.setAttribute("aria-label",name.textContent);input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=!!data.settings.observations[key];input.name=key;bindHelp(name,ui(purposes[key]));row.append(name,input);input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({observations:{[key]:requested}});}catch{input.checked=!requested;}finally{input.disabled=false;}});parent.append(row);}
  const groups=node("div",null,"observation-groups");for(const [title,keys]of [["對話與用量",["codex","metadata","usage","tool_events"]],["專案活動",["files","git","skills","checks","web"]],["診斷紀錄",["errors","logs","sqlite"]]]){const card=node("section",null,"observation-group");card.append(node("h4",ui(title)));for(const key of keys)observation(key,card);groups.append(card);}monitoring.append(groups);
  for(const key of ["mcp"])observation(key,mcp);
  const searchLabel=node("label",ui("搜尋 MCP"),"document-label"),search=node("input"),sources=node("div",null,"source-settings-grid");search.type="search";search.placeholder=ui("來源名稱或標籤");searchLabel.append(search);mcp.append(searchLabel,sources);
  for(const source of data.mcp?.servers||[]){
    const card=node("section",null,"source-setting-card"),head=node("div",null,"setting-row"),input=node("input");input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=source.enabled;input.setAttribute("aria-label",source.server+ui(" 檢查開關"));head.append(node("strong",source.server,"mono"),input);card.append(head);input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({mcp_sources:{[source.server]:requested}});}catch{input.checked=!requested;}finally{input.disabled=false;}});
    const row=node("label",ui("分類"),"setting-row"),select=node("select");select.setAttribute("aria-label",source.server+ui(" 分類"));for(const [key,text]of Object.entries(data.mcp.categories)){const option=node("option",ui(text));option.value=key;select.append(option);}select.value=source.category;row.append(select);card.append(row);select.addEventListener("change",async()=>{select.disabled=true;try{await changeSettings({mcp_categories:{[source.server]:select.value}});}catch{select.value=source.category;}finally{select.disabled=false;}});
    const labels=node("label",ui("用途標籤"),"document-label"),tags=node("input"),defaultTags=mcpTags({...source,tags:null},data.mcp.events||[]).map(tag=>tag.textContent);tags.type="text";tags.maxLength=180;tags.value=(source.tags||defaultTags).join(", ");tags.setAttribute("aria-label",source.server+ui(" 用途標籤"));bindHelp(labels,ui("最多 4 個標籤, 每個 40 字, 以逗號分開"));labels.append(tags);card.append(labels);
    const description=node("textarea"),descriptionLabel=node("label",ui("用途說明"),"document-label");description.rows=3;description.maxLength=400;description.value=source.description||"";description.setAttribute("aria-label",source.server+ui(" 用途說明"));descriptionLabel.append(description);
    const actions=node("div",null,"settings-actions"),save=button(ui("儲存"),async()=>{const list=[...new Set(tags.value.split(/[,，]/).map(text=>text.trim()).filter(Boolean))];if(list.length>4||list.some(text=>text.length>40)){feedback("settings-message",ui("最多 4 個標籤, 每個 40 字, 以逗號分開"),"error");return;}save.disabled=true;try{await changeSettings({mcp_descriptions:{[source.server]:description.value.trim()===source.default_description?"":description.value.trim()},mcp_tags:{[source.server]:JSON.stringify(list)===JSON.stringify(defaultTags)?[]:list}});feedback("settings-message",ui("MCP 設定已儲存"));renderObservationSettings();}catch{}finally{save.disabled=false;}}),reset=button(ui("還原預設"),async()=>{try{await changeSettings({mcp_descriptions:{[source.server]:""},mcp_tags:{[source.server]:[]},mcp_categories:{[source.server]:source.default_category}});renderObservationSettings();}catch{}});reset.hidden=!["mcp_descriptions","mcp_tags","mcp_categories"].some(key=>Object.hasOwn(data.settings[key]||{},source.server));actions.append(save,reset);card.append(descriptionLabel,actions);card.dataset.sourceSearch=(source.server+" "+(source.tags||defaultTags).join(" ")).toLowerCase();sources.append(card);
  }
  search.addEventListener("input",()=>{const query=search.value.trim().toLowerCase();for(const card of sources.children)card.hidden=!card.dataset.sourceSearch.includes(query);});
  recording.append(node("p",ui("紀錄設定由各 MCP 來源管理"),"muted"));
  const recordingCards=node("div",null,"recording-cards");recording.append(recordingCards);
  for(const source of data.mcp?.servers||[]){
    if(source.server==="web")continue;
    const state=data.mcp.recording_status?.[source.server],report=data.mcp.telemetry?.[source.server],card=node("article",null,"recording-card"),head=node("header"),items=[],flags=Object.entries(state?.flags||{});
    head.append(node("strong",source.server,"mono"),tag(ui(data.mcp.categories[source.category])||source.category));card.append(head);
    if(flags.length)for(const [key,flag]of flags){const label=flags.length===1?ui("來源紀錄"):/telemetry/i.test(key)?ui("Telemetry 開關"):/recording/i.test(key)?ui("紀錄開關"):metricLabel(key);items.push([label,tag(ui(flag.enabled?"已啟用":"已停用"),flag.enabled?"enabled":"disabled"),ui("來源回報的紀錄或 Telemetry 開關")+". "+key]);}
    else items.push([ui("來源紀錄"),tag(state?.enabled==null?ui("未回報開關"):ui(state.enabled?"已啟用":"已停用"),state?.enabled==null?"unknown":state.enabled?"enabled":"disabled"),ui("來源回報的紀錄或 Telemetry 開關")]);
    const observed=source.calls>0||source.recognized>0||(report?.summary?.calls||0)>0||(report?.recent?.length||0)>0,active=source.enabled&&data.settings.observations.mcp&&(data.settings.observations.codex||report&&report.health!=="paused");
    items.push([ui("本機檢查"),tag(ui(!active?"檢查已暫停":observed?"已取得紀錄":"等待紀錄"),!active?"paused":observed?"observed":"waiting")]);card.append(metadataList(items));
    const meta=node("div",null,"recording-meta");if(state?.health)meta.append(tag(labels.status[state.health]||logHealthLabels[state.health]||state.health));if(state?.observed_at)meta.append(node("time",when(state.observed_at)));if(meta.childElementCount)card.append(meta);recordingCards.append(card);
  }
  if(!recordingCards.childElementCount)recording.append(node("p","--","empty"));syncHelp();
}
function openSettings(){
  $("language-select").value=locale;
  $("display-options").value=display.options.join(", ");fillDisplayOptions($("default-ranking"),display.ranking);fillDisplayOptions($("default-table"),display.table);$("default-lines").value=display.lines??displayDefaults.lines;for(const [id,key]of [["default-main-summary","mainSummary"],["default-sub-summary","subSummary"]])$(id).value=display[key]??displayDefaults[key];
  if(!data)return;applyAppearance();$("refresh-interval").value=data.settings.interval;$("session-count").value=data.settings.max_files;syncSessionTracking();syncIdleSettings();
  renderObservationSettings();
  docTools=[...new Set([...docTools,...Object.keys(data.codex.tools||{}),...Object.keys(data.codex.nested_tools||{}),...Object.keys(data.settings.tool_descriptions||{})])].sort();renderToolDocs();
  $("settings-message").textContent="";if(!$("settings-dialog").open)openDialog($("settings-dialog"));
}

const tableViews=new Map(),specialTables=new Set(["codex-rows","mcp-rows","docs-rows"]);
const tableStates=Object.fromEntries(Object.entries(preferences.tables||{}).filter(([key,value])=>key.length<=400&&value&&(value.size==="all"||Number.isInteger(value.size)&&value.size>=1&&value.size<=200)&&Number.isInteger(value.page)&&value.page>0).slice(0,500));
for(const state of Object.values(tableStates)){for(const key of ["columns","hidden"])if(Array.isArray(state[key]))state[key]=state[key].map(currentCopyKey);if(typeof state.sort?.header==="string")state.sort.header=currentCopyKey(state.sort.header);if(state.filters)state.filters=Object.fromEntries(Object.entries(state.filters).map(([key,value])=>[currentCopyKey(key),value]));}
if((preferences.tableSchema||1)<2&&tableStates["codex-rows"]?.sort?.column>=3)tableStates["codex-rows"].sort.column++;
if((preferences.tableSchema||1)<3){
  const mappings={"codex-rows":[0,3,6,8,9,10,11,12,13,14,15],"mcp-rows":[0,1,2,4,7,8],"file-rows":[0,1,2,3,4,6,7],"jev-rows":[0,1,3,4,5,7,8]};
  for(const [key,mapping]of Object.entries(mappings))if(tableStates[key]?.sort?.column>=0)tableStates[key].sort.column=mapping[tableStates[key].sort.column]??-1;
}
if((preferences.tableSchema||1)<4)for(const key of ["mcp-rows","file-rows","web-event-rows"])if(tableStates[key]?.sort?.column>=2)tableStates[key].sort.column++;
if((preferences.tableSchema||1)<5&&tableStates["web-event-rows"]?.sort?.column>=5)tableStates["web-event-rows"].sort.column=tableStates["web-event-rows"].sort.column===5?6:8;
if((preferences.tableSchema||1)<6){const state=tableStates["web-event-rows"];if(state?.sort){if(state.sort.header==="參考網址"){state.sort.column=-1;delete state.sort.header;}else if(state.sort.column>=6)state.sort.column--;}if(state?.hidden)state.hidden=state.hidden.filter(label=>!["參考網址","參考網址 (個)"].includes(label));}
if((preferences.tableSchema||1)<7&&tableStates["codex-rows"]?.sort?.column>=14)tableStates["codex-rows"].sort.column++;
preferences.tableSchema=7;
const defaultHiddenColumns={"sqlite-rows":["操作類型","資料庫引擎","紀錄方式","工具整次耗時 (ms)","Call ID","影響列數","回傳列數"],"log-rows":["專案","操作","錯誤類型","檔案","紀錄 ID"],"log-source-rows":["已讀取 (bytes)","已讀取 (行)","過長紀錄 (行)"],"error-rows":["專案","工具"],"codex-rows":["Thread ID","類型","位置","活動來源","Cached input","Reasoning output"],"mcp-rows":["分類","紀錄方式","操作摘要"],"file-rows":["紀錄方式"],"jev-rows":["HTTP 結果"],"web-event-rows":["紀錄方式"]};
let tableSettingsKey=null;

const sortCollator=new Intl.Collator("zh-TW",{numeric:true,sensitivity:"base"});
function comparable(value){if(value==null||value===""||value==="--"||value===ui("未知")||value===ui("--"))return null;if(typeof value==="number")return value;const text=String(value).trim();if(/^-?[\d,]+(?:\.\d+)?$/.test(text))return Number(text.replaceAll(",",""));const date=text.match(/^(\d{4})[/-](\d{1,2})[/-](\d{1,2})[ T](\d{1,2}):(\d{2})(?::(\d{2}))?/);if(date)return new Date(Number(date[1]),Number(date[2])-1,Number(date[3]),Number(date[4]),Number(date[5]),Number(date[6]||0)).getTime();return text;}
function compareValues(a,b,descending){a=comparable(a);b=comparable(b);if(a===null)return b===null?0:1;if(b===null)return -1;const result=typeof a==="number"&&typeof b==="number"?a-b:sortCollator.compare(String(a),String(b));return descending?-result:result;}
function sortableColumn(label){return !/^(?:操作|操作摘要|用途說明|工具用途|顯示文字|actions?|purpose|description|表示文字|用途説明)$/i.test(label);}
function tableSort(key){
  const saved=tableStates[key]?.sort,view=tableViews.get(key);if(saved&&(saved.column>=0||saved.header&&view?.headers.includes(saved.header))&&(!view||sortableColumn(view.headers[saved.header?view.headers.indexOf(saved.header):saved.column]||"")))return saved;if(!view)return {column:-1,descending:true};
  const timed=label=>/時間|日期|最近活動|最近檢查|time|date|last activity|最終|日時|更新/i.test(label)&&!/耗時|duration|latency|実行時間|処理時間/i.test(label),measured=label=>/次|數|量|大小|bytes|token|P\d+|平均|呼叫|count|calls|reads|size|total|input|output|耗時|duration|latency|件数|回数|サイズ|容量/i.test(label);let column=view.headers.findIndex(label=>sortableColumn(label)&&timed(label)),descending=true;
  if(column<0)column=view.headers.findIndex(label=>sortableColumn(label)&&measured(label));if(column<0){column=view.headers.findIndex(sortableColumn);descending=false;}return {column,descending};
}
function sortColumn(key,sort){const view=tableViews.get(key);return sort.header&&view?.headers.includes(sort.header)?view.headers.indexOf(sort.header):sort.column;}
function sortRecords(key,items,columns,time){const sort=tableSort(key),index=sortColumn(key,sort),get=index<0?time:columns[index]||time;return [...items].sort((a,b)=>compareValues(get(a),get(b),sort.descending));}
function sortRows(key,view,rows=view.rows){const sort=tableSort(key),column=sortColumn(key,sort),index=column<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):column;const get=row=>index<0?row.querySelector("[data-sort-time]")?.dataset.sortTime:rowCells(row)[index]?.dataset.sortValue??rowValue(row,index);return [...rows].sort((a,b)=>compareValues(get(a),get(b),sort.descending));}
function changeTableSort(key,sort){if(sort.column>=0)sort.header=tableViews.get(key)?.headers[sort.column];const size=key==="codex-rows"?$("page-size").value:tableStates[key]?.size||display.table;tableStates[key]={...tableStates[key],size:size==="all"?"all":Number(size),page:1,sort};if(key==="codex-rows"){page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);persistTables();attachTables();if($("table-dialog").open)renderTableSort(key);}
function renderTableSort(key){const view=tableViews.get(key),sort=tableSort(key);$("table-sort-column").replaceChildren(...[[-1,ui("自動排序 (預設)")],...view.headers.map((label,index)=>[index,ui(label)]).filter(([,label])=>sortableColumn(label))].map(([value,label])=>{const option=node("option",label);option.value=value;return option;}));$("table-sort-column").value=String(sortColumn(key,sort));$("table-sort-direction").value=sort.descending?"desc":"asc";}
$("table-sort-column").addEventListener("change",()=>{changeTableSort(tableSettingsKey,{column:Number($("table-sort-column").value),descending:$("table-sort-direction").value==="desc"});feedback("table-settings-message",ui("排序已更新"));});
$("table-sort-direction").addEventListener("change",()=>{changeTableSort(tableSettingsKey,{column:Number($("table-sort-column").value),descending:$("table-sort-direction").value==="desc"});feedback("table-settings-message",ui("排序已更新"));});

const originalCells=new WeakMap();
function rowCells(row){let cells=originalCells.get(row);if(!cells){cells=[...row.children].sort((a,b)=>Number(a.dataset.columnIndex)-Number(b.dataset.columnIndex));originalCells.set(row,cells);}return cells;}
function rowValue(row,index){const cell=rowCells(row)[index];return cell?.querySelector(".tool")?.getAttribute("aria-label")||cell?.textContent.trim()||"--";}
function matchingRows(key,rows){const view=tableViews.get(key),filters=tableStates[key]?.filters||{},query=(filters.search||"").toLowerCase();return rows.filter(row=>(!query||row.textContent.toLowerCase().includes(query))&&view.headers.every((header,index)=>!filters[header]||filters[header]===rowValue(row,index)));}
function updateFilterSummary(section){const controls=section.querySelector(".filters"),summary=section.querySelector("summary"),active=[...controls.querySelectorAll("input,select")].filter(control=>!control.disabled&&!control.closest("label")?.hidden&&String(control.value).trim()!==(control.dataset.filterDefault||""));summary.textContent=ui("篩選")+(active.length?" · "+ui("已套用 ")+fmt(active.length)+ui(" 項"):"");}
function attachFilterSections(){
  const editor=document.querySelector(".document-toolbar");if(editor&&!editor.querySelector(".filters")){const controls=node("div",null,"filters compact-filters");for(const label of editor.querySelectorAll(":scope>label"))controls.append(label);editor.prepend(controls);}
  for(const controls of document.querySelectorAll(".filters")){
    for(const input of controls.querySelectorAll("input,select"))if(input.dataset.filterDefault==null)input.dataset.filterDefault=input.tagName==="SELECT"?([...input.options].find(option=>option.defaultSelected)||input.options[0])?.value||"":input.defaultValue||"";
    let section=controls.parentElement;
    if(!section.classList.contains("filter-section")){
      const key=controls.dataset.filterKey||controls.querySelector("[id]")?.id;if(!key)continue;
      section=node("details",null,"filter-section");section.dataset.filterKey=key;section.open=preferences.filterCollapsed?.[key]===false;const summary=node("summary",ui("篩選"));bindHelp(summary,ui("收合只隱藏篩選區, 已套用的條件仍會保留"));controls.before(section);section.append(summary,controls);
      section.addEventListener("toggle",()=>{if((preferences.filterCollapsed?.[key]!==false)===!section.open)return;preferences.filterCollapsed={...preferences.filterCollapsed,[key]:!section.open};saveView();});for(const event of ["input","change"])controls.addEventListener(event,()=>updateFilterSummary(section));
    }
    updateFilterSummary(section);
  }
}
function renderTableFilters(key){
  const view=tableViews.get(key);if(view.table.closest("dialog")||specialTables.has(key)||key==="file-rows")return;
  if(!view.filters){
    const wrap=view.table.closest(".table-wrap"),panel=wrap.closest(".panel"),existingControls=panel?.querySelectorAll("table").length===1?panel.querySelector(".filters"):null,controls=existingControls||node("div",null,"filters table-filters"),fields=new Map(),existing=[...controls.querySelectorAll("label")].map(label=>label.firstChild?.textContent.trim());
    const search=node("input"),label=node("label",ui("搜尋紀錄"));search.type="search";search.value=tableStates[key]?.filters?.search||"";label.append(search);controls.append(label);search.addEventListener("input",()=>update("search",search.value));
    for(const [index,header]of view.headers.entries())if(/對話|專案|來源|工具$|操作$|結果$|Skill$|類型$|等級$|事件$|Model$|紀錄方式|目錄名稱/.test(header)&&!existing.includes(ui(header))){const select=node("select"),label=node("label",ui(header));select.setAttribute("aria-label",view.title+" · "+ui(header));select.addEventListener("change",()=>update(header,select.value));fields.set(header,{index,select});label.append(select);controls.append(label);}
    function update(header,value){tableStates[key]={...tableStates[key],page:1,filters:{...tableStates[key]?.filters,[header]:value}};paginateTable(key);persistTables();}
    controls.dataset.filterKey=key;if(!existingControls)wrap.before(controls);view.filters={controls,fields,search};
  }
  for(const [header,{index,select}]of view.filters.fields){const values=[...new Set(view.rows.map(row=>rowValue(row,index)))].sort(sortCollator.compare),signature=JSON.stringify(values),selected=tableStates[key]?.filters?.[header]||"";if(select.dataset.signature!==signature){select.replaceChildren(...["",...values].map(value=>{const option=node("option",value||ui("全部"));option.value=value;return option;}));select.dataset.signature=signature;}if(!values.includes(selected)&&selected){tableStates[key].filters[header]="";select.value="";}else select.value=selected;}
}
function adaptTable(view){
  if(view.table.closest("[hidden]")||view.table.closest("dialog")&&!view.table.closest("dialog").open)return;
  const wrap=view.table.closest(".table-wrap"),width=wrap.clientWidth,visible=view.headerCells.filter(cell=>!cell.hidden).length,font=Number(document.documentElement.dataset.font)||14,stacked=width<760,minimum=visible*110*font/14,scroll=!stacked&&minimum>width;
  view.table.classList.add("table-fluid");view.table.classList.toggle("table-stacked",stacked);wrap.classList.toggle("table-scroll",scroll);view.table.style.minWidth=scroll?minimum+"px":"";wrap.tabIndex=scroll?0:-1;
  if(scroll){wrap.setAttribute("role","region");wrap.setAttribute("aria-label",view.title+ui(" · 左右捲動查看欄位"));}else{wrap.removeAttribute("role");wrap.removeAttribute("aria-label");}
  for(const th of view.headerCells)if(th.querySelector("button"))th.querySelector("button").tabIndex=stacked?-1:0;
  syncTableHead(view);
}
function syncTableHead(view){if(view.table.tHead)view.table.tHead.style.transform="";}
const defaultColumnHeaders=["最近活動時間","時間","呼叫時間","對話名稱","對話","專案","技能","操作","SQL 操作","檔案位置","資料庫位置","工具","來源","Model","Reasoning 等級","狀態","結果","工具回傳狀態","錯誤說明","等級","操作類型","資料庫引擎","類型","位置","活動來源","Input","Cached input","Output","Reasoning output","Total","工具 (次)","工作總耗時 (秒)","耗時 (ms)","工具耗時 (ms)","工具整次耗時 (ms)","工具回傳時間","紀錄方式","代碼","Thread ID","Call ID"];
function columnOrder(key){const view=tableViews.get(key),saved=tableStates[key]?.columns,defaults=key==="docs-rows"?view.headers:defaultColumnHeaders.filter(header=>view.headers.includes(header)).concat(view.headers);return [...new Set((Array.isArray(saved)?saved:[]).filter(header=>view.headers.includes(header)).concat(defaults))].map(header=>view.headers.indexOf(header));}
function setGlobalHeatmap(enabled){const previous=display.heatmap===true;display={...display,heatmap:enabled};for(const [key,state]of Object.entries(tableStates)){if(state.heatmapCustom!==true&&state.heatmap===previous)delete state.heatmap;if(tableViews.has(key))applyTableHeatmap(key);}for(const key of tableViews.keys())if(!tableStates[key])applyTableHeatmap(key);}
function applyTableHeatmap(key){
  const view=tableViews.get(key),enabled=(tableStates[key]?.heatmap??display.heatmap)===true;if(!enabled&&!view.heatmapActive)return;view.heatmapActive=enabled;const rows=[...view.body.children],maximum=[];
  for(const row of rows)for(const [index,cell]of rowCells(row).entries())if(cell.dataset.heatValue!=null)maximum[index]=Math.max(maximum[index]||0,Number(cell.dataset.heatValue));
  for(const row of specialTables.has(key)?view.body.children:view.rows)for(const [index,cell]of rowCells(row).entries()){const value=Number(cell.dataset.heatValue),active=enabled&&cell.dataset.heatValue!=null&&maximum[index]>0&&value>0;cell.classList.toggle("heat-cell",active);if(active)cell.style.setProperty("--heat-opacity",String(.04+.2*Math.min(1,value/maximum[index])));else cell.style.removeProperty("--heat-opacity");}
}
function applyTableColumns(key){
  const view=tableViews.get(key);if(!view)return;const hidden=Array.isArray(tableStates[key]?.hidden)?tableStates[key].hidden:defaultHiddenColumns[key]||[],order=columnOrder(key),signature=order.join(",");
  for(const [index,th]of view.headerCells.entries())th.hidden=hidden.includes(view.headers[index]);
  if(view.orderSignature!==signature){view.headerCells[0]?.parentElement.append(...order.map(index=>view.headerCells[index]));view.orderSignature=signature;}
  for(const row of specialTables.has(key)?view.body.children:view.rows){const cells=rowCells(row);for(const [index,td]of cells.entries()){td.hidden=hidden.includes(view.headers[index]);td.dataset.label=ui(view.headers[index]||"");}if(row.dataset.columnOrder!==signature){row.append(...order.map(index=>cells[index]).filter(Boolean));row.dataset.columnOrder=signature;}}
  applyTableHeatmap(key);adaptTable(view);
}
function moveColumn(key,header,target,after=false){
  if(header===target)return;const view=tableViews.get(key),columns=columnOrder(key).map(index=>view.headers[index]).filter(name=>name!==header),index=columns.indexOf(target);if(index<0)return;columns.splice(index+(after?1:0),0,header);tableStates[key]={...tableStates[key],columns};applyTableColumns(key);persistTables();if($("table-dialog").open&&tableSettingsKey===key){renderTableColumns(key);feedback("table-settings-message",ui("欄位順序已更新"));}feedback("action-message",ui("欄位順序已更新"));
}
function edgeScroll(container,position,horizontal,changed){
  let frame;function tick(){const point=position();if(!point)return;const rect=container.getBoundingClientRect(),coordinate=horizontal?point.clientX:point.clientY,start=horizontal?rect.left:rect.top,end=horizontal?rect.right:rect.bottom,speed=coordinate<start+40?-12:coordinate>end-40?12:0,before=horizontal?container.scrollLeft:container.scrollTop;if(speed)container.scrollBy(horizontal?speed:0,horizontal?0:speed);if((horizontal?container.scrollLeft:container.scrollTop)!==before)changed();frame=requestAnimationFrame(tick);}frame=requestAnimationFrame(tick);return ()=>cancelAnimationFrame(frame);
}
function dragColumn(handle,key,header,isHeader=false){
  if(handle.closest("dialog"))return;
  bindDragHelp(handle,isHeader?"拖曳調整順序, 或用 Alt + 方向鍵移動":"拖曳排序, 或用方向鍵移動");
  let drag=null,blockClick=false,ghost=null,stopScroll=()=>{};const scope=isHeader?tableViews.get(key).table.querySelector("thead"):$("table-columns"),clear=()=>{for(const el of scope.querySelectorAll("[data-column-key]"))el.classList.remove("column-dragging","column-drop-before","column-drop-after");};
  const stop=()=>{stopScroll();ghost?.remove();ghost=null;window.removeEventListener("pointermove",move);window.removeEventListener("pointerup",finish);window.removeEventListener("pointercancel",cancel);};
  function cancel(){drag=null;clear();stop();}
  function move(event){if(!drag||drag.pointer!==event.pointerId)return;if(!dragAllowed(handle)){drag=null;clear();stop();return;}drag.event=event;if(!drag.moved&&Math.hypot(event.clientX-drag.x,event.clientY-drag.y)<5)return;if(!drag.moved){drag.moved=true;handle.setPointerCapture(event.pointerId);ghost=node("div",null,"column-drag-preview");ghost.setAttribute("aria-hidden","true");(handle.closest("dialog")||document.body).append(ghost);const container=isHeader?scope.closest(".table-wrap"):scope.closest(".settings-content");stopScroll=edgeScroll(container,()=>drag?.event,isHeader,()=>move(drag.event));}event.preventDefault();clear();handle.closest("[data-column-key]").classList.add("column-dragging");const el=document.elementFromPoint(event.clientX,event.clientY)?.closest("[data-column-key]");drag.target=el&&scope.contains(el)&&el.dataset.columnKey!==header?el.dataset.columnKey:null;if(drag.target){const rect=el.getBoundingClientRect();drag.after=isHeader?event.clientX>rect.left+rect.width/2:event.clientY>rect.top+rect.height/2;el.classList.add(drag.after?"column-drop-after":"column-drop-before");}ghost.textContent=ui(header)+(drag.target?ui(" → 放在 ")+ui(drag.target)+ui(drag.after?" 後面":" 前面"):ui(" · 選擇插入位置"));ghost.style.left=Math.max(8,Math.min(event.clientX+18,innerWidth-ghost.offsetWidth-8))+"px";ghost.style.top=Math.max(8,Math.min(event.clientY+18,innerHeight-ghost.offsetHeight-8))+"px";}
  function finish(event){if(!drag||drag.pointer!==event.pointerId)return;const current=drag;drag=null;clear();stop();blockClick=current.moved;if(dragAllowed(handle)&&current.moved&&current.target)moveColumn(key,header,current.target,current.after);}
  handle.addEventListener("pointerdown",event=>{if(!dragAllowed(handle)||event.button!==0||event.target.closest("input"))return;drag={pointer:event.pointerId,x:event.clientX,y:event.clientY,moved:false,target:null,after:false};window.addEventListener("pointermove",move);window.addEventListener("pointerup",finish);window.addEventListener("pointercancel",cancel);});
  handle.addEventListener("lostpointercapture",()=>{if(drag)cancel();});
  handle.addEventListener("click",event=>{if(blockClick){event.preventDefault();event.stopPropagation();blockClick=false;}},true);
  handle.addEventListener("keydown",event=>{if(!dragAllowed(handle)||event.target.closest("input")||isHeader&&!event.altKey||!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const view=tableViews.get(key),columns=columnOrder(key).map(index=>view.headers[index]),index=columns.indexOf(header),after=["ArrowRight","ArrowDown","End"].includes(event.key),target=event.key==="Home"?columns[0]:event.key==="End"?columns.at(-1):columns[index+(after?1:-1)];if(target){moveColumn(key,header,target,after);if(isHeader)view.headerCells[view.headers.indexOf(header)].querySelector("button").focus();else [...$("table-columns").querySelectorAll("[data-column-key]")].find(el=>el.dataset.columnKey===header)?.focus();}});
}
function renderTableColumns(key){
  const view=tableViews.get(key),hidden=Array.isArray(tableStates[key]?.hidden)?tableStates[key].hidden:defaultHiddenColumns[key]||[];
  $("table-columns").replaceChildren(...columnOrder(key).map(index=>{const header=view.headers[index],option=node("div",null,"column-option"),label=node("label"),input=node("input");option.dataset.columnKey=header;input.type="checkbox";input.checked=!hidden.includes(header);input.addEventListener("change",()=>{const next=new Set(tableStates[key]?.hidden||defaultHiddenColumns[key]||[]);if(input.checked)next.delete(header);else next.add(header);if(view.headers.every(name=>next.has(name))){input.checked=true;feedback("table-settings-message",ui("至少保留一個欄位"),"error");return;}tableStates[key]={...tableStates[key],hidden:[...next]};applyTableColumns(key);persistTables();feedback("table-settings-message",ui("顯示欄位已更新"));});label.append(input,node("span",ui(header)));option.append(label);return option;}));
}
function tableSize(key,fallback=display.table){const size=tableStates[key]?.size??fallback;return size==="all"?Number.MAX_SAFE_INTEGER:size;}
function replaceRows(id,...rows){const body=$(id);body.replaceChildren(...rows);const view=tableViews.get(id);if(view){view.sourceChanged=true;if(specialTables.has(id))applyTableColumns(id);}}
function collapseTable(view,key){
  const wrap=view.table.closest(".table-wrap");if(wrap.closest(".table-disclosure"))return;
  const disclosure=node("details",null,"table-disclosure"),summary=node("summary",view.title),panel=wrap.closest(".panel"),heading=panel?.querySelector(":scope>h3,:scope>h4,:scope>.panel-head"),whole=panel&&panel.querySelectorAll("table").length===1&&heading;
  disclosure.open=tableStates[key]?.open!==false;disclosure.dataset.tableDisclosure=key;
  if(panel){panel.dataset.fullWidth="true";panel.classList.add("table-panel");}
  if(whole){const content=[...panel.children].slice([...panel.children].indexOf(heading)+1);heading.before(disclosure);if(heading.classList.contains("panel-head")){heading.querySelector("h3,h4")?.remove();summary.append(...heading.childNodes);}heading.remove();disclosure.append(summary,...content);}else{const toolbar=wrap.previousElementSibling;let previous=toolbar.previousElementSibling;const description=[];while(previous&&!previous.matches("h3,h4,details,.table-wrap")){description.unshift(previous);previous=previous.previousElementSibling;}toolbar.before(disclosure);if(previous?.matches("h3,h4")&&previous.textContent.trim()===view.title){previous.remove();disclosure.append(...description);}disclosure.prepend(summary);disclosure.append(toolbar,wrap);if(view.footer)disclosure.append(view.footer);}
  for(const title of disclosure.querySelectorAll("h3,h4"))if(title.textContent.trim()===view.title)title.remove();
  const toolbar=disclosure.querySelector(".table-toolbar");if(toolbar){summary.append(toolbar);for(const control of toolbar.querySelectorAll("button"))control.addEventListener("click",event=>{event.preventDefault();event.stopPropagation();});}
  view.disclosureSummary=node("span",null,"table-fold-summary");summary.append(view.disclosureSummary);
  disclosure.addEventListener("toggle",()=>{if(tableStates[key]?.open===disclosure.open)return;tableStates[key]={...tableStates[key],size:tableStates[key]?.size||display.table,page:tableStates[key]?.page||1,open:disclosure.open};persistTables();if(disclosure.open)adaptTable(view);arrangeContentPanels();});
}
function attachTables(){
  for(const el of document.querySelectorAll("table")){
    const body=el.querySelector("tbody"),key=el.dataset.tableKey||body?.id;if(!body||!key)continue;
    let view=tableViews.get(key);
    if(!view||view.table!==el){
      const wrap=el.closest(".table-wrap"),previous=wrap.previousElementSibling,caption=el.dataset.tableTitle||(/^H[1-4]$/.test(previous?.tagName)?previous.textContent:previous?.querySelector("h3,h4")?.textContent||wrap.closest(".panel,dialog")?.querySelector("h3,h4")?.textContent||ui("表格")),toolbar=node("div",null,"table-toolbar"),gear=button("⚙",()=>openTableSettings(key),"chart-setting-button");gear.setAttribute("aria-label",caption.trim()+ui(" 表格設定"));gear.title=ui("表格設定");toolbar.append(gear);wrap.before(toolbar);
      const headerCells=[...el.querySelectorAll("th")];view={table:el,body,rows:[],first:null,title:caption.trim(),headerCells,headers:headerCells.map(th=>th.dataset.copyBase||th.textContent)};tableViews.set(key,view);
      if(!specialTables.has(key)){const footer=node("div",null,"pagination table-pagination"),summary=node("span"),controls=node("div"),prev=button("←",()=>moveTablePage(key,-1)),next=button("→",()=>moveTablePage(key,1)),label=node("label",ui("第 ")),input=node("input"),total=node("span");prev.setAttribute("aria-label",ui("上一頁"));next.setAttribute("aria-label",ui("下一頁"));input.type="number";input.min=1;input.className="table-page-input";input.setAttribute("aria-label",view.title+ui(" 頁碼"));pageInput(input,()=>{tableStates[key]={...tableStates[key],size:tableStates[key]?.size||display.table,page:Number(input.value)};paginateTable(key);persistTables();});label.append(input,node("span",ui(" 頁 ")));controls.append(prev,label,total,next);footer.append(summary,controls);wrap.after(footer);Object.assign(view,{footer,summary,prev,next,input,total,label});}
    }
    collapseTable(view,key);
    if(!specialTables.has(key)){if(view.sourceChanged||body.firstChild!==view.first){view.rows=[...body.children];view.sourceChanged=false;}renderTableFilters(key);paginateTable(key);}
    applyTableColumns(key);
    const sort=tableSort(key),column=sortColumn(key,sort),active=column<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):column;
    for(const [index,th]of view.headerCells.entries()){if(!sortableColumn(view.headers[index])){th.replaceChildren(document.createTextNode(ui(view.headers[index])));th.removeAttribute("aria-sort");continue;}th.setAttribute("aria-sort",index===active?sort.descending?"descending":"ascending":"none");if(!th.querySelector("button")){const control=button("",()=>{const current=tableSort(key),selected=sortColumn(key,current),column=selected<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):selected;changeTableSort(key,{column:index,descending:column===index?!current.descending:false});},"sort-header");th.replaceChildren(control);th.dataset.columnKey=view.headers[index];bindHelp(th,(fieldDescription(view.headers[index])?fieldDescription(view.headers[index])+". ":"")+ui("點擊升降冪排序"));dragColumn(th,key,view.headers[index],true);}th.querySelector("button").textContent=ui(view.headers[index])+(index===active?sort.descending?" ↓":" ↑":" ↕");}
    if(specialTables.has(key))updateTableFoldSummary(view,key);adaptTable(view);

  }
  attachFilterSections();syncHelp();
}
function paginateTable(key){
  const view=tableViews.get(key);if(!view)return;const state=tableStates[key]||{size:display.table,page:1},size=tableSize(key),rows=matchingRows(key,view.rows),pages=Math.max(1,Math.ceil(rows.length/size));state.page=Math.max(1,Math.min(state.page,pages));tableStates[key]=state;
  view.prev.setAttribute("aria-label",ui("上一頁"));view.next.setAttribute("aria-label",ui("下一頁"));view.input.setAttribute("aria-label",ui(view.title)+ui(" 頁碼"));view.input.title=ui("輸入頁碼後按 Enter 套用");view.label.firstChild.textContent=ui("第 ");view.label.lastChild.textContent=ui(" 頁 ");const start=(state.page-1)*size;view.body.replaceChildren(...sortRows(key,view,rows).slice(start,start+size));view.first=view.body.firstChild;syncPageInput(view.input,state.page,pages);view.total.textContent="/ "+pages;view.summary.textContent=ui("共 ")+fmt(rows.length)+ui(" 筆")+(rows.length!==view.rows.length?ui(" · 全部 ")+fmt(view.rows.length)+ui(" 筆"):"");view.prev.disabled=state.page===1;view.next.disabled=state.page===pages;view.footer.hidden=!view.rows.length;applyTableColumns(key);updateTableFoldSummary(view,key,rows);
}
function updateTableFoldSummary(view,key,selected){
  if(!view.disclosureSummary)return;
  const rows=selected||(specialTables.has(key)?[...view.body.children]:matchingRows(key,view.rows)),filtered=key==="codex-rows"?filteredThreads().length:key==="mcp-rows"?Number(view.body.dataset.recordCount||0):key==="docs-rows"?visibleDocTools().length:rows.length;
  view.disclosureSummary.textContent=ui("共 ")+fmt(filtered)+ui(" 筆");
  if(specialTables.has(key))return;
  const columns=view.headers.flatMap((label,index)=>/時間|日期|最近|time|date/i.test(label)?[index]:[]),dates=rows.flatMap(row=>{const cells=rowCells(row);return columns.flatMap(index=>{const value=cells[index]?.textContent.trim()||"",time=Date.parse(value);return /(?:^|\D)\d{4}(?:\D|$)/.test(value)&&Number.isFinite(time)?[{value,time}]:[];});}).sort((a,b)=>a.time-b.time);
  if(dates.length)view.disclosureSummary.textContent+=ui(" · 最近 ")+dates.at(-1).value;
}
function persistTables(){preferences.tables=tableStates;saveView();}
function moveTablePage(key,delta){const state=tableStates[key]||{size:display.table,page:1};state.page+=delta;tableStates[key]=state;paginateTable(key);persistTables();}
function openTableSettings(key){tableSettingsKey=key;const view=tableViews.get(key);$("table-settings-title").textContent=(view?.title||ui("表格"))+ui(" · 表格設定");$("table-size").value=key==="codex-rows"?$("page-size").value:String(tableStates[key]?.size||display.table);$("table-settings-message").textContent="";$("table-heatmap").checked=(tableStates[key]?.heatmap??display.heatmap)===true;$("table-heatmap").disabled=!Array.from(view.body.querySelectorAll("[data-heat-value]")).length;fillDisplayOptions($("table-size"),$("table-size").value);renderTableSort(key);renderTableColumns(key);if(!$("table-dialog").open)openDialog($("table-dialog"));}
$("table-size").addEventListener("change",()=>{const key=tableSettingsKey,value=$("table-size").value;tableStates[key]={...tableStates[key],size:value==="all"?"all":Number(value),page:1};if(key==="codex-rows"){$("page-size").value=value;page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);persistTables();feedback("table-settings-message",ui("顯示數量已更新"));});
$("table-heatmap").addEventListener("change",()=>{const key=tableSettingsKey;tableStates[key]={...tableStates[key],heatmap:$("table-heatmap").checked,heatmapCustom:true};applyTableHeatmap(key);persistTables();feedback("table-settings-message",ui("數值熱度已更新"));});
$("table-settings-close").addEventListener("click",()=>$("table-dialog").close());

let docsMode="tools",docsBusy=false,confirmAction=null;
function openToolDocs(tool){
  if(!data)return;docsMode=tool?"tools":docsMode;docTools=[...new Set([tool,...Object.keys(data.codex.tools||{}),...Object.keys(data.codex.nested_tools||{}),...Object.keys(data.settings.tool_descriptions||{})].filter(Boolean))].sort();
  $("docs-search").value=tool||"";docsPage=1;renderToolDocs();$("docs-message").textContent="";
  if($("detail-dialog").open)$("detail-dialog").close();openSettings();document.querySelector('[data-modal-key="介面與說明"]').click();
}
function docGroup(key){if(docsMode==="tools"){const match=key.match(/^mcp__(.+)__/);return match?"MCP · "+match[1]:key.split(/[._]/)[0];}if(docsMode==="help"){if(/Token|快取|模型|推理/.test(key))return ui("模型與 Token");if(/CPU|平均|百分位|樣本|耗時/.test(key))return ui("效能與統計");if(/來源|紀錄|session|回補|讀取|資料庫/.test(key))return ui("來源與紀錄");return ui("操作與設定");}const target=copyTargets.find(item=>item.base===key),tag=target?.text?.parentElement?.tagName;if(/^(?:H[1-4]|TH)$/.test(tag))return ui("標題與欄位");if(target?.attribute||["BUTTON","LABEL","OPTION"].includes(tag))return ui("操作與設定");return ui("狀態與提示");}
function visibleDocTools(){
  const query=$("docs-search").value.trim().toLowerCase(),items=docsMode==="tools"?docTools:docsMode==="help"?[...helpCatalog]:[...copyCatalog].filter(key=>!helpCatalog.has(key)),select=$("docs-filter"),selected=select.value,groups=[...new Set(items.map(docGroup))].sort(),signature=docsMode+groups.join("|");
  if(select.dataset.signature!==signature){select.replaceChildren(...["all",...groups].map(value=>{const option=node("option",value==="all"?ui("全部分類"):value);option.value=value;return option;}));select.value=groups.includes(selected)?selected:"all";select.dataset.signature=signature;}
  const times=new Map();if(docsMode==="tools")for(const thread of data.codex.threads||[])for(const event of thread.tool_events||[])for(const tool of [event.tool,...Object.keys(event.nested_tools||{})])if(event.timestamp>(times.get(tool)||""))times.set(tool,event.timestamp);
  const filtered=items.filter(key=>(key+" "+(docsMode!=="tools"?ui(key):purpose(key))).toLowerCase().includes(query)&&(select.value==="all"||docGroup(key)===select.value));return sortRecords("docs-rows",filtered,[key=>key,key=>docsMode!=="tools"?ui(key):purpose(key)],key=>times.get(key));
}
function docKey(key){return docsMode+":"+key;}
function hasDocOverride(key){return docsMode==="tools"?Object.hasOwn(data.settings.tool_descriptions||{},key)&&data.settings.tool_descriptions[key]!==defaultPurpose(key):Object.hasOwn(preferences.copy,key)&&preferences.copy[key]!==preset(key);}
function updateDocsReset(){$("docs-reset-all").hidden=!Object.entries(data.settings.tool_descriptions||{}).some(([key,value])=>value!==defaultPurpose(key))&&!Object.entries(preferences.copy).some(([key,value])=>value!==preset(key))&&![...docDrafts].some(([key,value])=>value!==(key.startsWith("tools:")?defaultPurpose(key.slice(6)):preset(key.slice(key.indexOf(":")+1))));}
function renderToolDocs(){
  updateDocsReset();
  const tools=visibleDocTools(),size=tableSize("docs-rows"),pages=Math.max(1,Math.ceil(tools.length/size));docsPage=Math.max(1,Math.min(docsPage,pages));
  for(const [mode,id]of [["tools","docs-tools-mode"],["copy","docs-copy-mode"],["help","docs-help-mode"]]){const selected=docsMode===mode;$(id).setAttribute("aria-selected",String(selected));$(id).tabIndex=selected?0:-1;}$("docs-panel").setAttribute("aria-labelledby","docs-"+(docsMode==="tools"?"tools":docsMode==="help"?"help":"copy")+"-mode");
  $("docs-column-key").textContent=ui(docsMode==="tools"?"工具":"原始文案");$("docs-column-text").textContent=ui(docsMode==="tools"?"用途說明":"顯示文字");$("docs-search").placeholder=ui(docsMode==="tools"?"工具名稱":"原始文案或顯示文字");
  $("docs-column-key").dataset.copyBase=docsMode==="tools"?"工具":"原始文案";$("docs-column-text").dataset.copyBase=docsMode==="tools"?"用途說明":"顯示文字";const view=tableViews.get("docs-rows");if(view){view.headers[0]=$("docs-column-key").dataset.copyBase;view.headers[1]=$("docs-column-text").dataset.copyBase;}
  replaceRows("docs-rows",...tools.slice((docsPage-1)*size,docsPage*size).map(key=>{const row=node("tr");cell(row,docsMode!=="tools"?preset(key):key,"path-cell"+(docsMode==="tools"?" mono":""));const input=node("textarea"),draft=docKey(key);input.rows=3;input.maxLength=400;input.value=docDrafts.has(draft)?docDrafts.get(draft):docsMode==="tools"?purpose(key):ui(key);input.setAttribute("aria-label",key+ui(docsMode==="tools"?" 用途說明":" 顯示文字"));const reset=button(ui("還原預設"),()=>{docDrafts.delete(draft);saveDocuments({[key]:""});},"link"),base=docsMode==="tools"?defaultPurpose(key):preset(key);reset.hidden=input.value===base;input.addEventListener("input",()=>{docDrafts.set(draft,input.value);reset.hidden=input.value!==base?false:!hasDocOverride(key);updateDocsReset();});cell(row).append(input);const actions=cell(row,null,"doc-actions");reset.hidden=reset.hidden&&!hasDocOverride(key);actions.append(button(ui("儲存"),()=>saveDocuments({[key]:input.value.trim()})),reset);return row;}));
  $("docs-page-summary").textContent=ui("第 ")+docsPage+" / "+pages+ui(" 頁 · ")+tools.length+(docsMode==="tools"?" 個工具":" 項介面文字");$("docs-prev").disabled=docsPage===1;$("docs-next").disabled=docsPage===pages;syncPageInput($("docs-page-number"),docsPage,pages);attachTables();const current=tableViews.get("docs-rows"),caption=ui(docsMode==="tools"?"工具說明":docsMode==="help"?"指標說明":"介面設定");current.title=caption;const summary=current.table.closest(".table-disclosure").querySelector(":scope>summary");summary.firstChild.textContent=caption;summary.querySelector(".chart-setting-button").setAttribute("aria-label",caption+ui(" 表格設定"));
}
function setDocsBusy(value){docsBusy=value;for(const el of $("view-interface").querySelectorAll("button,input,textarea"))el.disabled=value;}
async function saveDocuments(changes){
  if(docsBusy)return;changes=Object.fromEntries(Object.entries(changes).map(([key,value])=>[key,value===(docsMode==="tools"?defaultPurpose(key):preset(key))?"":value]));setDocsBusy(true);feedback("docs-message",ui("正在儲存"),"pending");const mode=docsMode;
  try{
    if(mode==="tools")await changeSettings({tool_descriptions:changes});
    else{const next={...preferences.copy,...changes};for(const [key,value]of Object.entries(next))if(!value.trim())delete next[key];if(Object.keys(next).length>500)throw new Error(ui("自訂文案最多 500 項"));preferences.copy=next;saveView();applyCopy();render(data);if($("settings-dialog").open)openSettings();}
    for(const key of Object.keys(changes))docDrafts.delete(mode+":"+key);feedback("docs-message",ui(mode==="tools"?"工具說明已儲存":"介面設定已儲存"));
  }catch(error){feedback("docs-message",error.message,"error");}finally{setDocsBusy(false);renderToolDocs();}
}
function confirmChange(title,text,action,accept=ui("確認還原")){
  $("confirm-accept").textContent=accept;$("confirm-title").textContent=ui(title);$("confirm-description").textContent=ui(text);confirmAction=action;openDialog($("confirm-dialog"));}
async function resetDocuments(){
  if(docsBusy)return;setDocsBusy(true);feedback("docs-message",ui("正在還原預設"),"pending");
  try{const changes=Object.fromEntries(Object.keys(data.settings.tool_descriptions||{}).map(key=>[key,""]));if(Object.keys(changes).length)await changeSettings({tool_descriptions:changes});preferences.copy={};docDrafts.clear();saveView();applyCopy();render(data);if($("settings-dialog").open)openSettings();feedback("docs-message",ui("介面文字與工具說明已全部還原預設"));}
  catch(error){feedback("docs-message",error.message,"error");}finally{setDocsBusy(false);renderToolDocs();}
}
$("docs-reset-all").addEventListener("click",()=>confirmChange("還原全部預設?","所有自訂介面文字與工具說明都會還原, 尚未儲存的修改也會清除",resetDocuments));
$("confirm-accept").addEventListener("click",()=>{const action=confirmAction;$("confirm-dialog").close();if(action)action();});for(const id of ["confirm-cancel","confirm-close"])$(id).addEventListener("click",()=>$("confirm-dialog").close());$("confirm-dialog").addEventListener("close",()=>{confirmAction=null;});
$("docs-filter").addEventListener("change",()=>{docsPage=1;renderToolDocs();});$("docs-search").addEventListener("input",()=>{docsPage=1;renderToolDocs();});$("docs-save-page").addEventListener("click",()=>{const size=tableSize("docs-rows"),tools=visibleDocTools().slice((docsPage-1)*size,docsPage*size),changes=Object.fromEntries(tools.filter(key=>docDrafts.has(docKey(key))).map(key=>[key,docDrafts.get(docKey(key)).trim()]));if(Object.keys(changes).length)saveDocuments(changes);else feedback("docs-message",ui("本頁沒有修改"));});
for(const [mode,id]of [["tools","docs-tools-mode"],["copy","docs-copy-mode"],["help","docs-help-mode"]]){$(id).addEventListener("click",()=>{docsMode=mode;docsPage=1;$("docs-search").value="";$("docs-filter").value="all";$("docs-message").textContent="";renderToolDocs();});$(id).addEventListener("keydown",event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const modes=["docs-tools-mode","docs-copy-mode","docs-help-mode"],index=modes.indexOf(id),next=event.key==="Home"?modes[0]:event.key==="End"?modes.at(-1):modes[(index+(event.key==="ArrowRight"?1:2))%3];$(next).click();$(next).focus();});}
for(const [id,delta]of [["docs-prev",-1],["docs-next",1]])$(id).addEventListener("click",()=>{docsPage+=delta;renderToolDocs();});

function fillDisplayOptions(select,value,ranking=false){
  const current=value==="all"||String(value)==="0"?"all":String(value),values=[...display.options.map(String),"all"];
  if(!values.includes(current)&&/^\d+$/.test(current))values.splice(-1,0,current);
  select.replaceChildren(...values.map(item=>{const option=node("option",item==="all"?ui("全部"):item+(ranking?ui(" 項"):ui(" 筆")));option.value=ranking&&item==="all"?"0":item;return option;}));select.value=ranking&&current==="all"?"0":current;
}
function redrawTable(key){if(key==="codex-rows"){page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);}
$("display-options").addEventListener("change",()=>{const options=[...new Set($("display-options").value.split(/[,\s]+/).filter(Boolean).map(value=>Math.round(Number(value))))];if(options.length&&options.length<=8&&options.every(n=>Number.isInteger(n)&&n>=1&&n<=200)&&new Set(options).size===options.length){for(const id of ["default-ranking","default-table"]){const previous=$(id).value;$(id).replaceChildren(...[...options,"all"].map(value=>{const option=node("option",value==="all"?ui("全部"):String(value));option.value=value;return option;}));$(id).value=[...options,"all"].map(String).includes(previous)?previous:String(options[0]);}}});
$("apply-display").addEventListener("click",()=>{
  const options=[...new Set($("display-options").value.split(/[,\s]+/).filter(Boolean).map(value=>Math.round(Number(value))))],ranking=$("default-ranking").value,table=$("default-table").value,next={options,ranking:ranking==="all"?"all":Number(ranking),table:table==="all"?"all":Number(table),heatmap:$("global-table-heatmap").checked,lines:Number($("default-lines").value),mainSummary:Number($("default-main-summary").value),subSummary:Number($("default-sub-summary").value)};
  if(!validDisplay(next)){feedback("display-message",ui("請輸入 1 到 200 的整數, 最多 8 個選項, 以逗號分開"),"error");return;}
  const charts=[...chartViews.values()].filter(view=>(view.fields.top&&view.settings.top!==(display.ranking==="all"?0:display.ranking)&&view.settings.top!==(next.ranking==="all"?0:next.ranking)||view.fields.lines&&view.settings.lines!==(display.lines??3)&&view.settings.lines!==next.lines)),tables=[...new Set([...tableViews.keys(),...Object.keys(tableStates)])].filter(key=>{const state=tableStates[key]||{};return state.size!=null&&state.size!==display.table&&state.size!==next.table||state.heatmap!=null&&state.heatmap!==display.heatmap&&state.heatmap!==next.heatmap;});
  const apply=()=>{display=next;redrawSummaries();$("global-table-heatmap").checked=!!display.heatmap;$("display-options").value=options.join(", ");for(const view of chartViews.values()){if(view.fields.top){view.settings.top=display.ranking==="all"?0:display.ranking;fillDisplayOptions(view.fields.top,display.ranking,true);}if(view.fields.lines){view.settings.lines=display.lines;view.fields.lines.value=display.lines;}}fillDisplayOptions($("page-size"),display.table);for(const key of new Set([...tableViews.keys(),...Object.keys(tableStates)])){tableStates[key]={...tableStates[key],size:display.table,page:1};delete tableStates[key].heatmap;delete tableStates[key].heatmapCustom;if(tableViews.has(key))redrawTable(key);}persistTables();renderCharts();attachTables();feedback("display-message",ui("顯示數量已套用到所有排行榜與表格"));};
  if(charts.length||tables.length)confirmChange("覆蓋個別設定?",ui("將覆蓋個別設定")+": "+ui("圖表")+" "+fmt(charts.length)+", "+ui("表格")+" "+fmt(tables.length)+". "+ui("其他排序與顯示欄位保留"),apply,ui("確認覆蓋"));else apply();
});

const tabDescriptions=editableLabels({summary:"查看對話活動與執行狀態",tools:"查看工具呼叫與耗時",files:"查看檔案讀寫操作",mcp:"查看 MCP 來源與操作",errors:"查看錯誤與來源 Log",subagents:"顯示已載入且有上層 Thread ID 的子代理程式, 狀態取自來源最近紀錄",projects:"查看專案與所屬對話",dots:"查看 My dots 產出與對話",logs:"查看錯誤與來源 Log",git:"查看 Git 操作與活動趨勢",checks:"查看版本控制與驗證操作"});
function setupTabDescriptions(){for(const tab of document.querySelectorAll('[role="tab"]:not([data-tab])')){const panel=$(tab.getAttribute("aria-controls")),description=panel?.querySelector(".tab-description");if(description)bindHelp(tab,description.textContent);else{const key=tab.dataset.modalKey||tab.id.replace(/^(?:codex-|tab-|diagnostic-)/,"");bindHelp(tab,tabDescriptions[key]||ui("查看此分類的資料與設定"));}}}
function modalTabs(container,groups,id,selected,onSelect){
  const nav=node("div",null,"document-modes modal-tabs"),entries=[];nav.setAttribute("role","tablist");nav.setAttribute("aria-label",ui("明細分類"));
  for(const [index,[key,label,panel]]of groups.entries()){const tab=button(label,()=>select(key)),tabId=id+"-tab-"+index,panelId=id+"-panel-"+index;tab.id=tabId;tab.dataset.modalKey=key;tab.dataset.copyBase=label;tab.setAttribute("role","tab");bindHelp(tab,tabDescriptions[key]||ui("查看此分類的資料與設定"));tab.setAttribute("aria-controls",panelId);panel.id=panelId;panel.classList.add("modal-tab-content");const descriptions=[...panel.children].filter(child=>child.matches("p:not([role=status]),.tool-purpose,.purpose-block"));for(const description of [...descriptions].reverse())panel.prepend(description);panel.setAttribute("role","tabpanel");panel.setAttribute("aria-labelledby",tabId);entries.push({key,tab,panel});nav.append(tab);}
  function select(key){container.scrollTop=0;const dialog=container.closest("dialog");if(dialog)dialog.scrollTop=0;const scroller=container.closest(".settings-content,#detail-content");if(scroller)scroller.scrollTop=0;for(const entry of entries){const active=entry.key===key;entry.tab.setAttribute("aria-selected",String(active));entry.tab.tabIndex=active?0:-1;entry.panel.hidden=!active;}onSelect?.(key);}
  for(const [index,entry]of entries.entries())entry.tab.addEventListener("keydown",event=>{if(event.altKey)return;if(event.altKey||!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?entries.length-1:(index+(["ArrowRight","ArrowDown"].includes(event.key)?1:entries.length-1))%entries.length;select(entries[next].key);entries[next].tab.focus();});
  const previous=container.previousElementSibling;if(previous?.classList.contains("modal-tabs"))previous.remove();container.before(nav);container.replaceChildren(...entries.map(entry=>entry.panel));select(entries.some(entry=>entry.key===selected)?selected:entries[0].key);
}
function groupThreadDetails(content){
  const groups=new Map(),headingGroup=new Map([[ui("工具使用 (次)"),"tools"],[ui("exec 內辨識到的工具"),"tools"],[ui("工具呼叫紀錄"),"tools"],[ui("MCP 檢查"),"mcp"],[ui("檔案讀寫紀錄"),"files"],[ui("MCP / Web 操作"),"mcp"],[ui("子代理程式"),"subagents"],[ui("網路參考"),"mcp"],[ui("錯誤與警告"),"errors"]]),names={summary:ui("對話資訊"),tools:ui("工具"),files:ui("檔案"),mcp:"MCP",subagents:ui("子代理程式"),errors:ui("錯誤紀錄")};let key="summary";
  for(const child of [...content.children]){if(child.tagName==="H4"&&headingGroup.has(child.textContent))key=headingGroup.get(child.textContent);if(!groups.has(key))groups.set(key,node("section"));groups.get(key).append(child);}
  modalTabs(content,[...groups].map(([id,panel])=>[id,names[id]||id,panel]),"thread-detail",detail.section,key=>detail.section=key);
}
function groupSettings(){
  const content=$("settings-dialog").querySelector(".settings-content"),status=$("settings-message"),groups=[];status.remove();let panel;
  for(const child of [...content.children]){if(child.dataset.modalSection){if(child.dataset.modalSection==="資料來源"){panel=null;continue;}panel=node("section");groups.push([child.dataset.modalSection,ui(child.dataset.modalSection),panel]);}panel?.append(child);}
  const position=groups.findIndex(([key])=>key==="設定檔");for(const [index,[key,id]]of [["監測項目","observation-data"],["MCP 來源","observation-mcp"],["來源紀錄","observation-recording"]].entries()){const section=node("section");section.append(node("h4",ui(key)));const target=node("div");target.id=id;section.append(target);groups.splice(position+index,0,[key,ui(key),section]);}
  modalTabs(content,groups,"settings-sections","外觀");const nav=content.previousElementSibling,layout=node("div",null,"settings-layout");nav.classList.add("settings-navigation");nav.setAttribute("aria-label",ui("設定分類"));nav.setAttribute("aria-orientation","vertical");content.before(layout);layout.append(nav,content);status.className="settings-feedback";layout.before(status);
}
if(preferences.tab==="codex"&&preferences.conversationSource==="usage")preferences.tab="usage";
let conversationSource=["conversations","subagents","projects","dots"].includes(preferences.conversationSource)?preferences.conversationSource:"conversations";
function selectConversationSource(key){if(key==="archived")key="conversations";conversationSource=key;preferences.conversationSource=key;for(const item of ["conversations","subagents","projects","dots"]){const selected=item===key,tab=$("codex-"+item);tab.setAttribute("aria-selected",String(selected));tab.tabIndex=selected?0:-1;$(item==="conversations"?"conversation-content":item+"-content").hidden=!selected;}if(data){attachTables();syncSourceWindow();renderCharts();renderSources();saveView();if(data.codex?.activity_scope?.window!==activeSourceWindow()){version++;refresh();}}}
for(const [index,key]of ["conversations","subagents","projects","dots"].entries()){const tab=$("codex-"+key);tab.addEventListener("click",()=>selectConversationSource(key));tab.addEventListener("keydown",event=>{if(event.altKey)return;if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const items=["conversations","subagents","projects","dots"],next=event.key==="Home"?0:event.key==="End"?items.length-1:(index+(event.key==="ArrowRight"?1:items.length-1))%items.length;selectConversationSource(items[next]);$("codex-"+items[next]).focus();});}selectConversationSource(conversationSource);dragSubTabs($("conversation-tabs"),"codex",tab=>tab.id);
function usageWindow(minutes){return minutes==null?"--":minutes%1440===0?fmt(minutes/1440)+ui(" 天"):minutes%60===0?fmt(minutes/60)+ui(" 小時"):fmt(minutes)+ui(" 分鐘");}
let usageModelCounts={},usageModelSegments={};
function renderAllowance(id,usage){
  const target=$(id);target.replaceChildren(...(usage?.limits||[]).map(item=>{const panel=node("section",null,"allowance-window"),expired=item.resets_at&&new Date(item.resets_at)<new Date(),progress=node("progress");panel.append(node("h4",usageWindow(item.window_minutes)),metadataList([[ui("剩餘比例"),item.remaining_percent==null?"--":fmt(item.remaining_percent)+"%"],[ui("已使用"),item.used_percent==null?"--":fmt(item.used_percent)+"%"],[ui("重設時間"),when(item.resets_at)],[ui("資料狀態"),ui(expired?"等待來源更新":"來源最後回報")]]));if(item.remaining_percent!=null){progress.max=100;progress.value=item.remaining_percent;progress.setAttribute("aria-label",ui("剩餘比例")+" "+fmt(item.remaining_percent)+"%");panel.append(progress);}panel.append(node("p",ui("最近更新時間")+": "+when(usage.updated_at),"snapshot-at"));return panel;}));if(!usage?.limits?.length)target.append(node("p","--","empty"));
}
function renderUsage(){
  const usage=data.codex.usage,threads=data.codex.threads||[],groups=new Map();
  $("usage-observed").hidden=true;
  const knownTokens=threads.map(thread=>thread.tokens?.total_tokens).filter(Number.isFinite);
  cards("usage-cards",[[ui("方案"),usage?.plan_type||"--",usage?.limit_id||""],[ui("Credits 餘額"),fmt(usage?.credits?.balance),""],[ui("Token 合計"),fmt(knownTokens.length?knownTokens.reduce((sum,value)=>sum+value,0):null),ui("已載入對話")],[ui("最後檢查"),when(usage?.updated_at),""]]);
  for(const thread of threads){const model=thread.model||"unknown";if(!groups.has(model))groups.set(model,{model,threads:0,known:0,complete:true,tokens:{}});const group=groups.get(model);group.threads++;const t=thread.tokens||{};group.complete&&=[t.input_tokens,t.cached_input_tokens,t.output_tokens,t.total_tokens].every(Number.isFinite)&&t.input_tokens>=t.cached_input_tokens&&t.input_tokens+t.output_tokens===t.total_tokens;if(Object.values(thread.tokens||{}).some(Number.isFinite))group.known++;for(const [key,value]of Object.entries(thread.tokens||{}))if(Number.isFinite(value))group.tokens[key]=(group.tokens[key]||0)+value;}
  replaceRows("usage-model-rows",...[...groups.values()].map(group=>{const row=node("tr");cell(row,group.model==="unknown"?"--":group.model,"mono");valueCell(row,group.threads);valueCell(row,group.known);for(const key of ["input_tokens","cached_input_tokens","output_tokens","reasoning_output_tokens","total_tokens"])valueCell(row,group.tokens[key]);valueCell(row,group.tokens.input_tokens>0&&group.tokens.cached_input_tokens!=null&&group.tokens.cached_input_tokens<=group.tokens.input_tokens?Math.round(group.tokens.cached_input_tokens/group.tokens.input_tokens*10000)/100:null);return clickableRow(row,()=>openDetail({kind:"usage-model",model:group.model}));}));
  usageModelCounts=Object.fromEntries([...groups.values()].filter(group=>group.tokens.total_tokens!=null).map(group=>[group.model,group.tokens.total_tokens]));usageModelSegments=Object.fromEntries([...groups.values()].map(group=>{const t=group.tokens,complete=group.complete&&[t.input_tokens,t.cached_input_tokens,t.output_tokens,t.total_tokens].every(Number.isFinite)&&t.input_tokens>=t.cached_input_tokens&&t.input_tokens+t.output_tokens===t.total_tokens;return [group.model,complete?[["一般輸入",t.input_tokens-t.cached_input_tokens],["快取輸入",t.cached_input_tokens],["輸出",t.output_tokens]]:[["未分類",t.total_tokens]]];}));
  const account=telemetryFields({plan_type:usage?.plan_type,credits_balance:usage?.credits?.balance,has_credits:usage?.credits?.has_credits,unlimited:usage?.credits?.unlimited}).filter(([,value])=>value!=null);for(const id of ["overview-billing","overview-account-card"])$(id).replaceChildren(account.length?metadataList(account.map(([key,value])=>[metricLabel(key),telemetryValue(value)])):node("p","--","empty"));

  $("usage-note").textContent=ui("依模型分析對話 Token 用量與快取比例");
}
function renderDots(){const dots=data.codex.dots||{},events=dots.events||[];cards("dot-cards",[[ui("產出紀錄"),fmt(Number.isFinite(dots.total)?dots.total:Array.isArray(dots.events)?events.length:null),ui("已載入的產出 metadata")],[ui("關聯對話"),fmt(Array.isArray(dots.events)?new Set(events.map(event=>event.thread_id)).size:null),ui("可從列表開啟對話")],[ui("活動快照項目"),fmt(dots.activity_items),ui("本機快照中的項目數")],[ui("最近活動"),when(events.map(event=>event.timestamp).filter(Boolean).sort().at(-1)),""]]);replaceRows("dot-rows",...events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row,event.artifact_type||"--");cell(row,event.thread_id,"mono");return row;}));$("dot-note").textContent=ui("查看 My dots 產出時間, 類型與關聯對話");}

let currentTabSettings=null;
function contentCards(scope){
  return [...scope.querySelectorAll(".panel")].filter(panel=>!panel.closest(".overview-panels")&&!panel.matches(".source-reference")&&!panel.parentElement.closest(".panel")).map(panel=>{
    const heading=panel.querySelector("h3,h4,.table-disclosure>summary"),key=panel.dataset.visibilityKey||panel.querySelector(".chart,.bar-chart,tbody[id]")?.id||panel.id||panel.querySelector("[id]")?.id;if(!heading||!key)return null;panel.dataset.visibilityKey=key;const label=heading.dataset.copyBase||heading.childNodes[0]?.textContent||heading.textContent;return {panel,key,label,ranking:/排行|ranking|ランキング/i.test(label)};
  }).filter(Boolean);
}
const defaultContentCharts=new Set(["tool-time-chart","tool-breakdown-chart","tool-duration-chart","skill-time-chart","skill-breakdown-chart","skill-thread-chart","check-time-chart","check-breakdown-chart","check-thread-chart","git-time-chart","git-breakdown-chart","git-thread-chart","web-time-chart","web-url-time-chart","web-site-time-chart","file-time-chart","file-path-time-chart","file-folder-time-chart","mcp-time-chart","mcp-breakdown-chart","mcp-retained-chart","sqlite-time-chart","sqlite-breakdown-chart","sqlite-retained-chart","error-time-chart","error-source-chart","error-level-chart","log-time-chart","log-level-chart","log-source-chart","conversation-time-chart","conversation-model-chart","conversation-environment-chart","usage-model-chart","monitor-refresh-chart","monitor-read-chart"]);
function defaultCardVisible({key,ranking}){return key.endsWith("-statistics")?defaultContentCharts.has(key.slice(0,-11)):chartViews.has(key)?defaultContentCharts.has(key):!ranking;}
function applyContentVisibility(){for(const item of contentCards(document.querySelector("main")))item.panel.classList.toggle("layout-hidden",!(preferences.cardVisibility?.[item.key]??defaultCardVisible(item)));}

function redrawSummaries(){for(const {root,items}of [...summaryLibrary.values()])cards(root,items);}
function renderSummarySettings(scope){
  const list=$("overview-layout-rows"),owner=$("view-"+currentTabSettings)||scope.closest("[id^='view-']")||scope,groups=[...summaryLibrary.values()].filter(({root})=>scope.contains(root)||owner===root.closest("[id^='view-']")&&!root.closest("[hidden]"));
  for(const {root,items,sub}of groups){const section=node("section",null,"card-library-group summary-library"),caption=node("h4",ui(sub?"子 Tab 摘要卡":"主 Tab 摘要卡")),size=node("select"),state=preferences.summaries?.[root.id]||{},order=[...new Set((state.order||[]).concat(items.map((_,index)=>index)))].filter(index=>index<items.length);size.setAttribute("aria-label",ui("摘要卡數量"));for(const value of ["inherit",1,2,3,4,5,6,7,8]){const option=node("option",value==="inherit"?ui("沿用全域")+" ("+(display[sub?"subSummary":"mainSummary"]??(sub?3:4))+")":String(value));option.value=value;size.append(option);}size.value=state.count??"inherit";
    const save=next=>{preferences.summaries={...preferences.summaries,[root.id]:next};cards(root,summaryLibrary.get(root.id).items);saveView();};size.addEventListener("change",()=>{const next={...preferences.summaries?.[root.id]};if(size.value==="inherit")delete next.count;else next.count=Number(size.value);save(next);});const header=node("div",null,"setting-row");header.append(caption,size);section.append(header);
    for(const [position,index]of order.entries()){const row=node("div",null,"summary-editor-row"),shown=node("input"),title=node("input"),actions=node("div",null,"chart-actions");shown.type="checkbox";shown.checked=!state.hidden?.includes(index);shown.setAttribute("aria-label",ui("顯示")+" "+items[index][0]);title.value=state.titles?.[index]||items[index][0];title.type="text";title.maxLength=80;title.setAttribute("aria-label",ui("摘要卡名稱")+" "+items[index][0]);shown.addEventListener("change",()=>{const next={...preferences.summaries?.[root.id]},hidden=new Set(next.hidden||[]);if(shown.checked)hidden.delete(index);else hidden.add(index);next.hidden=[...hidden];save(next);});title.addEventListener("change",()=>{const next={...preferences.summaries?.[root.id],titles:{...preferences.summaries?.[root.id]?.titles}};if(title.value.trim())next.titles[index]=title.value.trim();else delete next.titles[index];save(next);});
      for(const [delta,label]of [[-1,"上移"],[1,"下移"]]){const move=button(delta<0?"↑":"↓",()=>{const next=[...order],target=position+delta;[next[position],next[target]]=[next[target],next[position]];save({...preferences.summaries?.[root.id],order:next});if(currentTabSettings==="overview")renderOverviewSettings();else renderContentSettings(currentTabSettings);});move.setAttribute("aria-label",ui(label)+" "+items[index][0]);move.disabled=position+delta<0||position+delta>=order.length;actions.append(move);}row.append(shown,title,actions);section.append(row);
    }list.append(section);
  }
}
for(const [id,key]of [["default-main-summary","mainSummary"],["default-sub-summary","subSummary"]])$(id).addEventListener("change",()=>{display={...display,[key]:Number($(id).value)};redrawSummaries();saveView();feedback("display-message",ui("摘要卡預設已更新"));});

function renderContentSettings(name){renderCardLibrary($("overview-layout-rows"),contentCards(tabScope(name)).map(({panel,key,label,ranking})=>({id:key,panel,label,checked:preferences.cardVisibility?.[key]??defaultCardVisible({key,ranking}),change:shown=>{preferences.cardVisibility={...preferences.cardVisibility,[key]:shown};applyContentVisibility();if(data)renderCharts();saveView();feedback("tab-settings-message",ui("卡片顯示已更新"));}})));renderSummarySettings(tabScope(name));}

function tabScope(name){return name==="workflow"?$("view-"+activitySource):name==="errors"?$(diagnosticSource==="logs"?"view-logs":"error-content"):name==="codex"?$(conversationSource==="conversations"?"conversation-content":conversationSource+"-content"):$("view-"+name);}
function visibleInScope(scope,element){return scope.contains(element)&&!element.closest("[hidden],.layout-hidden");}
function openTabSettings(name){
  currentTabSettings=name;windowOptions($("tab-source-window"),true);$("tab-source-window").value=sourceWindows.tabs[sourceWindowKey(name)]||"inherit";$("overview-layout-controls").hidden=false;if(name==="overview")renderOverviewSettings();else renderContentSettings(name);$("overview-layout-controls").querySelector("h4").textContent=ui("卡片庫");$("overview-layout-controls").querySelector("p").textContent=ui("依來源選擇卡片, 查看支援的圖表形式並個別設定");$("overview-layout-reset").textContent=ui(name==="overview"?"還原圖表配置":"還原卡片顯示");const scope=tabScope(name),charts=[...chartViews].filter(([id])=>visibleInScope(scope,$(id))),tables=[...tableViews].filter(([,view])=>visibleInScope(scope,view.table));
  $("sql-masking-controls").hidden=name!=="sqlite";$("sql-masking").checked=preferences.sqlMasking!==false;
  $("tab-settings-title").textContent=$("tab-"+name).textContent+ui(" · Tab 設定");$("tab-chart-controls").hidden=!charts.length;$("tab-table-controls").hidden=!tables.length;fillDisplayOptions($("tab-table-size"),display.table);$("tab-settings-message").textContent="";
  const keys={overview:[],usage:["usage"],codex:["codex","metadata"],tools:["tool_events"],mcp:["mcp"],web:["web"],files:["files"],sqlite:["sqlite"],jev:["jev","jev_calls"],git:["git"],workflow:["git","checks"],skills:["skills"],checks:["checks"],monitor:[],errors:["errors","logs"],logs:["logs","codex"]}[name]||[];
  $("tab-observations").replaceChildren(...keys.filter(key=>data.settings.observations[key]!=null).map(key=>{const row=node("div",null,"setting-row"),input=node("input");input.setAttribute("aria-label",observationLabels[key]||key);input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=data.settings.observations[key];input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({observations:{[key]:requested}});feedback("tab-settings-message",ui("檢查設定已套用"));}catch(error){input.checked=!requested;feedback("tab-settings-message",error.message,"error");}finally{input.disabled=false;}});row.title=ui("啟用或停用此檢查項目, 不會修改來源紀錄");row.append(node("span",observationLabels[key]||key),input);return row;}));
  $("tab-observations-heading").hidden=!keys.length;openDialog($("tab-dialog"));
}
$("apply-tab-charts").addEventListener("click",()=>{roundNumber($("tab-chart-length"));const name=currentTabSettings,scope=tabScope(name),range=$("tab-chart-range").value,length=Number($("tab-chart-length").value),unit=Number($("tab-chart-unit").value);if(!Number.isInteger(length)||length<1||length>365){feedback("tab-settings-message",ui("請輸入 1 到 365 的整數"),"error");return;}for(const [id,view]of chartViews)if(visibleInScope(scope,$(id))){Object.assign(view.settings,{range,length,unit});for(const key of ["range","length","unit"])view.fields[key].value=view.settings[key];view.fields.length.parentElement.hidden=view.fields.unit.parentElement.hidden=range!=="recent";view.fields.start.parentElement.hidden=view.fields.end.parentElement.hidden=true;}saveView();renderCharts();feedback("tab-settings-message",ui("時間範圍已套用到此 Tab 的圖表"));});
$("apply-tab-tables").addEventListener("click",()=>{const scope=tabScope(currentTabSettings),value=$("tab-table-size").value;for(const [key,view]of tableViews)if(visibleInScope(scope,view.table)){tableStates[key]={...tableStates[key],size:value==="all"?"all":Number(value),page:1};if(key==="codex-rows")fillDisplayOptions($("page-size"),value);redrawTable(key);}persistTables();attachTables();feedback("tab-settings-message",ui("每頁筆數已套用到此 Tab 的表格"));});
$("sql-masking").addEventListener("change",()=>{preferences.sqlMasking=$("sql-masking").checked;for(const selected of [detail,...detailHistory.map(item=>item.view)])if(selected?.kind==="sqlite"){selected.sqlLoadToken=null;delete selected.sqlContent;delete selected.sqlError;selected.sqlLoaded=false;selected.sqlLoading=false;}saveView();if(detail?.kind==="sqlite"){renderDetail();loadSqlContent(detail);}});
$("tab-settings-close").addEventListener("click",()=>$("tab-dialog").close());
function addTabSettings(){for(const tab of document.querySelectorAll("[data-tab]")){const head=$("view-"+tab.dataset.tab).querySelector(".section-head"),gear=button("⚙",()=>openTabSettings(tab.dataset.tab),"chart-setting-button tab-settings-button");gear.setAttribute("aria-label",tab.textContent+ui(" Tab 設定"));gear.title=ui("Tab 設定");const actions=head.querySelector(".section-actions")||node("div",null,"section-actions");actions.append(gear);if(!actions.parentElement)head.append(actions);}}


function configuration(){
  saveView();preferences.tables=tableStates;preferences.tabOrder=tabOrder;const keys=["tab","inputs","page","appearance","display","charts","tables","tableSchema","tabOrder","copy","settings","locale","mcpSource","overview","highlightOrder","subOrders","cardOrders","diagnosticSource","activitySource","conversationSource","filterCollapsed","sectionCollapsed","chartDefaultsVersion","sourceWindows","cardVisibility","sqlMasking","summaries"];
  return {application:"local-activity-monitor",kind:"settings",version:1,exported_at:new Date().toISOString(),preferences:Object.fromEntries(keys.filter(key=>preferences[key]!=null).map(key=>[key,preferences[key]]))};
}
function readConfiguration(value){
  const object=item=>item&&typeof item==="object"&&!Array.isArray(item),bounded=(items,max,check)=>object(items)&&Object.keys(items).length<=max&&Object.entries(items).every(([key,item])=>key.length<=400&&!["__proto__","constructor","prototype"].includes(key)&&check(item,key));
  if(!object(value)||value.application!=="local-activity-monitor"||value.kind!=="settings"||value.version!==1||!object(value.preferences)||(value.recording!=null&&typeof value.recording!=="boolean"))throw new Error(ui("設定檔格式或版本不支援"));
  const p=value.preferences,a=p.appearance,s=p.settings;
  if(p.sourceWindows!=null&&(!object(p.sourceWindows)||!activityWindows.includes(p.sourceWindows.global)||!bounded(p.sourceWindows.tabs||{},64,(item,key)=>/^[a-z][a-z:-]{0,79}$/.test(key)&&activityWindows.includes(item))))throw new Error(ui("來源紀錄範圍設定格式無效"));
  if(p.locale!=null&&!["zh-TW","en","ja"].includes(p.locale))throw new Error(ui("設定檔格式或版本不支援"));
  if(!object(a)||!["auto","light","dark"].includes(a.mode)||!["slate","neutral"].includes(a.theme)||!["green","blue","orange"].includes(a.accent)||!Number.isInteger(a.font)||a.font<12||a.font>18||!validDisplay(p.display)||!object(s)||!Number.isInteger(s.interval)||s.interval<1||s.interval>3600||!Number.isInteger(s.max_files)||s.max_files<1||s.max_files>5000||typeof s.track_all!=="boolean"||s.idle_minutes!=null&&(!Number.isInteger(s.idle_minutes)||s.idle_minutes<0||s.idle_minutes>1440)||s.activity_retention_days!=null&&(!Number.isInteger(s.activity_retention_days)||s.activity_retention_days<1||s.activity_retention_days>365))throw new Error(ui("外觀, 更新頻率或顯示數量不在可用範圍"));
  if(!bounded(s.observations,64,item=>typeof item==="boolean")||!bounded(s.mcp_sources||{},64,item=>typeof item==="boolean")||!bounded(s.mcp_categories||{},64,item=>typeof item==="string"&&item.length<=80)||!bounded(s.tool_descriptions||{},64,item=>typeof item==="string"&&item.length<=400)||!bounded(s.mcp_descriptions||{},64,item=>typeof item==="string"&&item.length<=400)||!bounded(s.mcp_tags||{},64,item=>Array.isArray(item)&&item.length<=4&&item.every(text=>typeof text==="string"&&text.trim()&&text.length<=40))||!bounded(p.copy||{},500,item=>typeof item==="string"&&item.length<=400)||!bounded(p.inputs||{},100,item=>typeof item==="string"&&item.length<=2000))throw new Error(ui("檢查設定或介面文字格式無效"));
  if(!bounded(p.charts||{},256,item=>object(item)&&["all","recent","custom"].includes(item.range)&&Number.isInteger(item.length)&&item.length>=1&&item.length<=365&&[60000,3600000,86400000].includes(item.unit)&&[60000,300000,900000,3600000,21600000,86400000].includes(item.interval)&&Number.isInteger(item.top)&&item.top>=0&&item.top<=200&&Number.isFinite(item.maximum)&&item.maximum>=0&&item.maximum<=1e9&&(item.lines==null||[3,5,10].includes(item.lines))&&(item.statistics==null||[0,1].includes(item.statistics))&&["bar","line","pie","donut","column","stacked"].includes(item.shape)&&typeof item.start==="string"&&typeof item.end==="string"&&Number.isFinite(new Date(item.start).getTime())&&Number.isFinite(new Date(item.end).getTime())&&(item.range!=="custom"||new Date(item.start)<new Date(item.end)))||!bounded(p.tables||{},500,item=>object(item)&&(item.size==="all"||Number.isInteger(item.size)&&item.size>=1&&item.size<=200)&&Number.isInteger(item.page)&&item.page>=1&&(item.heatmap==null||typeof item.heatmap==="boolean")&&(item.heatmapCustom==null||typeof item.heatmapCustom==="boolean")&&(item.open==null||typeof item.open==="boolean")&&(!item.hidden||Array.isArray(item.hidden)&&item.hidden.length<=100&&item.hidden.every(label=>typeof label==="string"&&label.length<=400))&&(!item.columns||Array.isArray(item.columns)&&item.columns.length<=100&&item.columns.every(label=>typeof label==="string"&&label.length<=400))&&(!item.filters||bounded(item.filters,100,value=>typeof value==="string"&&value.length<=2000))&&(!item.sort||object(item.sort)&&Number.isInteger(item.sort.column)&&item.sort.column>=-1&&item.sort.column<=100&&typeof item.sort.descending==="boolean")))throw new Error(ui("圖表或表格設定格式無效"));
  if(typeof p.tab!=="string"||p.tab.length>80||!Number.isInteger(p.page)||p.page<1||!Array.isArray(p.tabOrder)||p.tabOrder.length>100||!p.tabOrder.every(key=>typeof key==="string"&&key.length<=80)||!Number.isInteger(p.tableSchema||1)||(p.tableSchema||1)>7)throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.diagnosticSource!=null&&!["errors","logs"].includes(p.diagnosticSource)||p.activitySource!=null&&!["git","skills","checks"].includes(p.activitySource))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.mcpSource!=null&&(typeof p.mcpSource!=="string"||!/^[a-zA-Z0-9_.-]{1,80}$/.test(p.mcpSource)))throw new Error(ui("MCP 來源設定格式無效"));
  if(p.conversationSource!=null&&!["conversations","subagents","projects","archived","dots"].includes(p.conversationSource))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.highlightOrder!=null&&(!Array.isArray(p.highlightOrder)||p.highlightOrder.length>100||!p.highlightOrder.every(id=>typeof id==="string"&&id.length<=80))||p.subOrders!=null&&!bounded(p.subOrders,100,items=>Array.isArray(items)&&items.length<=100&&items.every(id=>typeof id==="string"&&id.length<=80)))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.sqlMasking!=null&&typeof p.sqlMasking!=="boolean")throw new Error(ui("SQL 遮蔽設定格式無效"));
  if(p.summaries!=null&&!bounded(p.summaries,100,item=>object(item)&&(item.count==null||Number.isInteger(item.count)&&item.count>=1&&item.count<=8)&&["order","hidden"].every(key=>item[key]==null||Array.isArray(item[key])&&item[key].length<=32&&new Set(item[key]).size===item[key].length&&item[key].every(index=>Number.isInteger(index)&&index>=0&&index<32))&&(item.titles==null||bounded(item.titles,32,title=>typeof title==="string"&&title.length<=80))))throw new Error(ui("摘要卡設定格式無效"));
  if(p.cardVisibility!=null&&!bounded(p.cardVisibility,500,item=>typeof item==="boolean"))throw new Error(ui("卡片顯示設定格式無效"));
  if(p.cardOrders!=null&&!bounded(p.cardOrders,100,items=>Array.isArray(items)&&items.length<=100&&items.every(id=>typeof id==="string"&&id.length<=80)))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.filterCollapsed!=null&&!bounded(p.filterCollapsed,500,item=>typeof item==="boolean"))throw new Error(ui("篩選設定格式無效"));
  if(p.sectionCollapsed!=null&&!bounded(p.sectionCollapsed,100,item=>typeof item==="boolean"))throw new Error(ui("設定檔格式或版本不支援"));
  if(p.chartDefaultsVersion!=null&&p.chartDefaultsVersion!==1)throw new Error(ui("設定檔格式或版本不支援"));
  if(p.overview!=null&&(!object(p.overview)||!["order","hidden"].every(key=>Array.isArray(p.overview[key])&&p.overview[key].length<=500&&p.overview[key].every(id=>typeof id==="string"&&id.length<=80))))throw new Error(ui("總覽圖表設定格式無效"));
  const backend=compatibleSettings(s,data.settings,data.mcp?.categories||{});backend.mcp_sources??={};backend.mcp_categories??={};backend.tool_descriptions??={};backend.mcp_descriptions??={};backend.mcp_tags??={};backend.replace_customizations=true;
  return {preferences:JSON.parse(JSON.stringify(p)),backend,recording:value.recording};
}
$("export-configuration").addEventListener("click",()=>{
  if(!data)return;const text=JSON.stringify(configuration(),null,2)+"\n";$("configuration-text").value=text;const url=URL.createObjectURL(new Blob([text],{type:"application/json"})),link=node("a");link.href=url;link.download="local-activity-monitor-settings-"+new Date().toISOString().slice(0,10)+".json";document.body.append(link);link.click();link.remove();setTimeout(()=>URL.revokeObjectURL(url),1000);feedback("configuration-message",ui("設定檔已準備下載, 下方也可複製 JSON 內容"));
});
$("configuration-file").addEventListener("change",async()=>{
  const file=$("configuration-file").files[0];if(!file)return;
  if(file.size>2097152){feedback("configuration-message",ui("設定檔上限 2 MiB"),"error");return;}
  try{$("configuration-text").value=await file.text();feedback("configuration-message",ui("設定檔已讀取, 請預覽並確認匯入"));}catch{feedback("configuration-message",ui("設定檔無法讀取"),"error");}
});
$("import-configuration").addEventListener("click",()=>{
  try{const text=$("configuration-text").value;if(new TextEncoder().encode(text).length>2097152)throw new Error(ui("設定檔上限 2 MiB"));const imported=readConfiguration(JSON.parse(text)),p=imported.preferences;
    confirmChange("匯入設定?",ui("將套用外觀, 檢查開關, 顯示數量, 圖表, 表格與介面文字")+" · "+Object.keys(p.charts||{}).length+ui(" 個圖表 · ")+Object.keys(p.tables||{}).length+ui(" 個表格"),async()=>{
      const control=$("import-configuration");control.disabled=true;feedback("configuration-message",ui("正在匯入設定"),"pending");
      try{await post("/api/settings",imported.backend);const preferencesText=JSON.stringify(imported.preferences);localStorage.setItem(preferenceKey,preferencesText);feedback("configuration-message",ui("設定已匯入, 正在重新載入"));location.reload();}
      catch(error){feedback("configuration-message",error.message,"error");}finally{control.disabled=false;}
    },ui("確認匯入"));
  }catch(error){feedback("configuration-message",error instanceof SyntaxError?ui("設定內容不是有效的 JSON"):error.message,"error");}
});


const overviewGrid=document.querySelector(".overview-panels"),overviewPanels=new Map([...overviewGrid.children].map(panel=>{const chart=panel.querySelector(".chart,.bar-chart");panel.dataset.tabOrder=panel.dataset.cardKey||chart.id;panel.classList.add("tab-order-row");return [panel.dataset.tabOrder,panel];})),overviewPriority=["overview-account-card","overview-account-quota","overview-token-stack","model-chart","activity-chart","overview-tools-chart","source-chart","overview-web-chart","overview-file-chart","overview-sql-chart","overview-git-chart","overview-check-chart","overview-skill-chart","overview-dot-chart","overview-error-chart","overview-error-time-chart","overview-cpu-chart","overview-refresh-chart","overview-read-chart","environment-chart","trigger-chart","overview-source-column","overview-model-ring","overview-model-pie","overview-token-chart"],overviewDefault=[...new Set(overviewPriority.filter(id=>overviewPanels.has(id)).concat([...overviewPanels.keys()]))],overviewDefaultVisible=new Set(["overview-account-card","overview-account-quota","activity-chart","model-chart","overview-token-stack"]),priorOverviewDefaultHidden=overviewDefault.filter(id=>!overviewPriority.slice(0,16).includes(id)),overviewDefaultHidden=overviewDefault.filter(id=>!overviewDefaultVisible.has(id)||/排行/.test(overviewPanels.get(id).querySelector("h3").textContent)),overviewLabel=id=>ui(chartViews.get(id)?.title||overviewPanels.get(id)?.querySelector("h3")?.textContent||id);
const legacyOverviewPriority=["activity-chart","overview-billing","overview-quota-chart","overview-token-stack","overview-error-chart","source-chart","overview-tools-chart","model-chart","overview-file-chart","overview-git-chart","overview-check-chart","overview-skill-chart","overview-sql-chart","overview-cpu-chart","overview-refresh-chart","overview-read-chart","overview-web-chart","overview-dot-chart","environment-chart","trigger-chart","overview-error-time-chart","overview-source-column","overview-model-ring","overview-model-pie","overview-token-chart"],legacyOverviewDefault=legacyOverviewPriority.filter(id=>overviewPanels.has(id)),legacyOverviewHidden=legacyOverviewDefault.filter(id=>!legacyOverviewPriority.slice(0,12).includes(id)),sameOrder=(a,b)=>Array.isArray(a)&&a.length===b.length&&a.every((id,index)=>id===b[index]);
if(sameOrder(preferences.overview?.order,legacyOverviewDefault))preferences.overview.order=overviewDefault;
if(sameOrder(preferences.overview?.hidden,priorOverviewDefaultHidden)||sameOrder(preferences.overview?.hidden,overviewDefault.filter(id=>!overviewPriority.slice(0,16).includes(id)||/(?:error|cpu|refresh|read|environment|trigger)/.test(id)))||sameOrder(preferences.overview?.hidden,legacyOverviewHidden)||sameOrder(preferences.overview?.hidden,overviewDefault.filter(id=>!["overview-account-card","overview-account-quota","activity-chart"].includes(id))))preferences.overview.hidden=overviewDefaultHidden;
let overviewOrder=[...new Set((Array.isArray(preferences.overview?.order)?preferences.overview.order:[]).filter(id=>overviewPanels.has(id)).concat(overviewDefault))],overviewHidden=new Set((Array.isArray(preferences.overview?.hidden)?preferences.overview.hidden:overviewDefaultHidden).concat(overviewDefault.filter(id=>id.endsWith("-statistics")&&!preferences.overview?.order?.includes(id))).filter(id=>overviewPanels.has(id)));
function overviewNeeds(owner){return !$("view-overview")?.hidden&&overviewOrder.some(id=>!overviewHidden.has(id)&&cardLibrary.get(id)?.owner===owner);}
function applyOverview(){for(const id of overviewOrder){const panel=overviewPanels.get(id);panel.hidden=overviewHidden.has(id);overviewGrid.append(panel);}preferences.overview={order:overviewOrder,hidden:[...overviewHidden]};arrangeOverview();}
function moveOverview(id,target,after=false){if(id===target)return;const next=overviewOrder.filter(key=>key!==id),index=next.indexOf(target);if(index<0)return;next.splice(index+(after?1:0),0,id);overviewOrder=next;applyOverview();saveView();renderOverviewSettings();feedback("tab-settings-message",ui("總覽圖表順序已更新"));}

function cardOwner(element){return element.closest("#conversation-content,#projects-content,#subagents-content,#error-content,[id^='view-']")?.id||"view-overview";}
function cardOwnerLabel(owner){const names={"conversation-content":"對話","projects-content":"專案","subagents-content":"子代理程式","error-content":"錯誤紀錄","view-logs":"來源 Log","view-git":"Git","view-checks":"驗證操作"};return ui(names[owner]||$("tab-"+owner.replace(/^view-/,""))?.textContent||owner);}
function setupOverviewCopies(){
  const meterId="overview-copy-device-usage",meter=node("section",null,"panel"),body=node("div"),head=node("div",null,"panel-head device-usage-head"),updated=node("span",null,"card-updated");body.id=meterId;updated.id=meterId+"-updated";head.append(node("h3",ui("裝置使用量")),updated);meter.dataset.tabOrder=meterId;meter.classList.add("tab-order-row");meter.append(head,body);overviewGrid.append(meter);overviewPanels.set(meterId,meter);overviewDefault.push(meterId);overviewDefaultHidden.push(meterId);overviewOrder.push(meterId);if(!preferences.overview?.order?.includes(meterId)||preferences.overview?.hidden?.includes(meterId))overviewHidden.add(meterId);cardLibrary.set(meterId,{id:meterId,baseId:meterId,owner:"view-monitor",title:"裝置使用量",kind:"使用量",types:[],meter:true});

  for(const [sourceId,source]of [...cardLibrary]){
    if(source.owner==="view-overview"||source.chartId||source.meter)continue;
    const id="overview-copy-"+sourceId,panel=node("section",null,"panel"),heading=node("h3",ui(source.title)),graph=source.trend?svgNode("svg",{viewBox:"0 0 740 240",role:"img","aria-label":ui(source.title),class:"chart"}):node("div",null,"bar-chart");graph.id=id;panel.dataset.tabOrder=id;panel.classList.add("tab-order-row");panel.append(heading,graph);if(source.trend){const note=node("p",null,"muted");note.id=id+"-note";panel.append(note);}overviewGrid.append(panel);overviewPanels.set(id,panel);overviewDefault.push(id);overviewOrder.push(id);overviewCopies.set(id,sourceId);
    const visible=["conversation-content","error-content","view-usage"].includes(source.owner)&&source.kind!=="排行";if(!visible)overviewDefaultHidden.push(id);if(preferences.overview?.hidden?.includes(id)||!preferences.overview?.order?.includes(id)&&!visible)overviewHidden.add(id);
    chartControls(id,source.trend,source.series,sourceId);
  }
}
function renderOverviewCopies(){
  const meter=$("overview-copy-device-usage"),m=data.monitor||{},total=m.physical_memory_bytes,available=m.available_memory_bytes,used=Number.isFinite(total)&&Number.isFinite(available)&&total>0&&available>=0&&available<=total?total-available:null;$("overview-copy-device-usage-updated").textContent=ui("更新時間")+": "+when(m.device_checked_at);meter.replaceChildren(metadataList([[ui("CPU 使用率"),usageMeter(m.cpu_usage_percent,ui("CPU"))],[ui("已使用記憶體"),usageMeter(used==null?null:Math.round(used/total*1000)/10,memorySize(used)+" / "+memorySize(total)),ui("可用記憶體")+": "+memorySize(available)]]));

  for(const [id,sourceId]of overviewCopies){if(overviewHidden.has(id)&&(!overviewPanels.has(id+"-statistics")||overviewHidden.has(id+"-statistics")))continue;const recipe=chartViews.get(sourceId)?.renderData;if(!recipe)continue;const note=id+"-note",args=recipe.args;
    if(recipe.kind==="rank")renderRankingCard(id,...args);else if(recipe.kind==="bars")bars(id,...args);else if(recipe.kind==="timeline")timeline(id,note,...args);else if(recipe.kind==="category")categoryTimeline(id,note,...args);else if(recipe.kind==="threads")activeThreadsTimeline(id,note,...args);
  }
}
function cardSettingRow(id,label,checked,change){
  const row=node("div",null,"setting-row card-library-row"),field=node("div",null,"card-library-field"),check=node("input"),caption=node("span",ui(label));check.type="checkbox";check.checked=checked;check.setAttribute("aria-label",ui(label));check.addEventListener("change",()=>change(check.checked));field.append(check,caption);const card=cardLibrary.get(id),actions=node("div",null,"card-library-meta");if(card){actions.append(node("span",ui(card.kind),"tag"));if(card.types.length)actions.append(node("span",card.types.map(([,label])=>ui(label)).join(" / "),"muted"));if(!card.meter)actions.append(button(ui("設定"),()=>openChartSettings(card.chartId||id)));}row.append(field,actions);return row;
}
function renderCardLibrary(list,items){
  list.replaceChildren();const groups=new Map();for(const item of items){const owner=cardLibrary.get(item.id)?.owner||cardOwner(item.panel);if(!groups.has(owner)){const section=node("section",null,"card-library-group");section.append(node("h4",cardOwnerLabel(owner)));groups.set(owner,section);list.append(section);}groups.get(owner).append(cardSettingRow(item.id,item.label,item.checked,item.change));}
}
function renderOverviewSettings(){renderCardLibrary($("overview-layout-rows"),overviewOrder.map(id=>({id,panel:overviewPanels.get(id),label:overviewLabel(id),checked:!overviewHidden.has(id),change:shown=>{if(shown)overviewHidden.delete(id);else overviewHidden.add(id);applyOverview();if(data){renderCharts();if(overviewNeeds("view-logs")&&!logBusy)loadLogs();if(overviewNeeds("view-files")&&!fileSnapshots.has(activeSourceWindow())&&!fileSnapshotRequests.has(activeSourceWindow())&&!fileSummaryBusy)loadFileSizes();}saveView();feedback("tab-settings-message",ui("總覽圖表顯示已更新"));}})));renderSummarySettings($("view-overview"));}

function setupOverview(){for(const heading of document.querySelectorAll(".overview-group-title")){heading.tabIndex=0;bindHelp(heading,ui(heading.textContent==="活動圖表"?"依來源範圍呈現活動趨勢與分布, 各圖表可個別設定":"彙整目前載入的活動數量, 點選卡片可查看相關紀錄"));}for(const [id,panel]of overviewPanels){const heading=panel.querySelector("h3");heading.tabIndex=0;heading.classList.add("direct-drag");heading.setAttribute("aria-label",overviewLabel(id));bindHelp(heading,ui("查看活動隨時間的變化或不同類型的分布"));dragTab(heading,id,{root:overviewGrid,order:()=>overviewOrder.filter(key=>!overviewHidden.has(key)),label:overviewLabel,move:moveOverview});}for(const heading of document.querySelectorAll(".overview-group-title,.overview-panels h3")){const mark=node("span"," ⓘ","overview-help-mark");mark.setAttribute("aria-hidden","true");mark.dataset.copy="ignore";heading.append(mark);}applyOverview();}
$("overview-layout-reset").addEventListener("click",()=>{if(currentTabSettings!=="overview"){confirmChange("還原卡片顯示?","此分頁的卡片將回到預設顯示",()=>{for(const {key}of contentCards(tabScope(currentTabSettings)))delete preferences.cardVisibility?.[key];for(const {root}of summaryLibrary.values())if(tabScope(currentTabSettings).contains(root))delete preferences.summaries?.[root.id];redrawSummaries();applyContentVisibility();renderContentSettings(currentTabSettings);if(data)renderCharts();saveView();feedback("tab-settings-message",ui("卡片顯示已更新"));});return;}confirmChange("還原總覽圖表?","圖表的顯示項目與順序將回到預設",()=>{overviewOrder=[...overviewDefault];overviewHidden=new Set(overviewDefaultHidden);delete preferences.summaries?.["overview-cards"];redrawSummaries();applyOverview();if(data)renderCharts();saveView();renderOverviewSettings();});});

const defaultTabOrder=[...document.querySelectorAll("[data-tab]")].map(tab=>tab.dataset.tab);
let tabOrder=[...new Set((Array.isArray(preferences.tabOrder)?preferences.tabOrder:[]).filter(id=>defaultTabOrder.includes(id)).concat(defaultTabOrder))];
function applyTabOrder(){const nav=document.querySelector(".tabs");for(const id of tabOrder)nav.append($("tab-"+id));}
function moveTab(id,target,after=false){
  if(id===target)return;const next=tabOrder.filter(key=>key!==id),index=next.indexOf(target);if(index<0)return;next.splice(index+(after?1:0),0,id);tabOrder=next;preferences.tabOrder=tabOrder;applyTabOrder();saveView();feedback("settings-message",ui("Tab 順序已更新"));
}
function dragTab(handle,id,config){
  if(handle.closest("dialog")||config.root.closest("dialog"))return;
  bindDragHelp(handle,config.horizontal?"拖曳調整順序, 或用 Alt + 方向鍵移動":"拖曳調整順序, 或用方向鍵上下移動");
  let drag=null,blockClick=false,ghost=null,stopScroll=()=>{};
  const clear=()=>{for(const row of config.root.children)row.classList.remove("dragging","drop-before","drop-after");};
  const stop=()=>{stopScroll();ghost?.remove();ghost=null;};
  function move(event){
    if(!drag||drag.pointer!==event.pointerId)return;if(!dragAllowed(handle)){drag=null;clear();stop();return;}drag.event=event;if(!drag.moved&&Math.hypot(event.clientX-drag.x,event.clientY-drag.y)<5)return;
    if(!drag.moved){drag.moved=true;ghost=node("div",null,"column-drag-preview");ghost.setAttribute("aria-hidden","true");(handle.closest("dialog")||document.body).append(ghost);stopScroll=edgeScroll(config.horizontal?config.root:handle.closest(".settings-content")||document.scrollingElement,()=>drag?.event,!!config.horizontal,()=>move(drag.event));}
    event.preventDefault();clear();handle.closest(".tab-order-row").classList.add("dragging");const row=document.elementFromPoint(event.clientX,event.clientY)?.closest(".tab-order-row");drag.target=row&&config.root.contains(row)&&row.dataset.tabOrder!==id?row.dataset.tabOrder:null;
    if(drag.target){const rect=row.getBoundingClientRect();drag.after=config.grid&&Math.abs(event.clientY-rect.top-rect.height/2)<rect.height*.25?event.clientX>rect.left+rect.width/2:config.horizontal?event.clientX>rect.left+rect.width/2:event.clientY>rect.top+rect.height/2;row.classList.add(drag.after?"drop-after":"drop-before");}
    ghost.textContent=config.label(id)+(drag.target?ui(" → 放在 ")+config.label(drag.target)+ui(drag.after?" 後面":" 前面"):ui(" · 選擇插入位置"));ghost.style.left=Math.max(8,Math.min(event.clientX+18,innerWidth-ghost.offsetWidth-8))+"px";ghost.style.top=Math.max(8,Math.min(event.clientY+18,innerHeight-ghost.offsetHeight-8))+"px";
  }
  handle.addEventListener("pointerdown",event=>{if(!dragAllowed(handle)||event.button!==0)return;const control=event.target.closest("button,a,input,select,textarea,[role=button]");if(control&&control!==handle)return;drag={pointer:event.pointerId,x:event.clientX,y:event.clientY,target:null,after:false,moved:false};handle.setPointerCapture(event.pointerId);handle.focus();});
  handle.addEventListener("pointermove",move);
  handle.addEventListener("pointerup",event=>{if(!drag||drag.pointer!==event.pointerId)return;const current=drag;drag=null;clear();stop();blockClick=current.moved;if(dragAllowed(handle)&&current.moved&&current.target)config.move(id,current.target,current.after);});
  for(const event of ["pointercancel","lostpointercapture"])handle.addEventListener(event,()=>{drag=null;clear();stop();});
  handle.addEventListener("click",event=>{if(blockClick){event.preventDefault();event.stopPropagation();blockClick=false;}},true);
  handle.addEventListener("keydown",event=>{if(!dragAllowed(handle)||event.target!==handle||config.horizontal&&!event.altKey||!["ArrowUp","ArrowDown","ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const visible=config.order(),index=visible.indexOf(id),target=event.key==="Home"?visible[0]:event.key==="End"?visible.at(-1):visible[index+(["ArrowUp","ArrowLeft"].includes(event.key)?-1:1)];if(target){config.move(id,target,["ArrowDown","ArrowRight","End"].includes(event.key));const row=config.root.querySelector('[data-tab-order="'+id+'"]');(row.tagName==="BUTTON"?row:row.querySelector("h3")||row).focus();}});
}

$("layout-drag-toggle").addEventListener("change",()=>{dragEnabled=$("layout-drag-toggle").checked;document.documentElement.dataset.layoutEdit=String(dragEnabled);hideHelp();hideChartTip();});document.documentElement.dataset.layoutEdit="false";
$("reset-all-configuration").addEventListener("click",()=>confirmChange("全部設定還原預設?","外觀, 語言, 顯示數量, 排序, 圖表, 表格, 介面文字與檢查設定都會還原",async()=>{const control=$("reset-all-configuration");control.disabled=true;try{const defaults=data.default_settings;if(!defaults)throw new Error(ui("無法取得預設設定, 請重新啟動程式"));await post("/api/settings",{...defaults,replace_customizations:true});localStorage.setItem(preferenceKey,JSON.stringify({settings:defaults}));location.reload();}catch(error){feedback("configuration-message",error.message,"error");}finally{control.disabled=false;}},ui("確認還原")));

$("observation-settings").addEventListener("click",openSettings);
$("language-select").addEventListener("change",()=>{if(!["zh-TW","en","ja"].includes($("language-select").value))return;locale=$("language-select").value;applyLanguage();saveView();if($("settings-dialog").open)openSettings();feedback("settings-message",ui("語言已切換"));});
$("settings-close").addEventListener("click",()=>$("settings-dialog").close());
$("dark-toggle").addEventListener("click",()=>{appearance.mode=document.documentElement.dataset.mode==="dark"?"light":"dark";applyAppearance();saveView();});
for(const el of document.querySelectorAll("#mode-options [data-mode]"))el.addEventListener("click",()=>{appearance.mode=el.dataset.mode;applyAppearance();saveView();});
for(const [id,key]of [["theme-select","theme"],["accent-select","accent"]])$(id).addEventListener("change",()=>{appearance[key]=$(id).value;applyAppearance();saveView();feedback("settings-message",ui("外觀已更新"));});
$("font-size").addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();$("apply-font").click();}});
$("apply-font").addEventListener("click",()=>{const input=$("font-size");if(!roundNumber(input)||!input.checkValidity()){feedback("settings-message",ui("請輸入有效數值"),"error");return;}appearance.font=Number(input.value);applyAppearance();saveView();feedback("settings-message",ui("字體大小已套用: ")+appearance.font+" px");});
systemDark.addEventListener("change",applyAppearance);
async function applySessionCount(){if(settingsBusy||!data)return;const input=$("session-count");roundNumber(input);if(!input.checkValidity()||!Number.isInteger(Number(input.value))){input.value=data.settings.max_files;feedback("settings-message",ui("session 數量請輸入 1 - 5000 的整數"),"error");return;}input.disabled=true;try{await changeSettings({max_files:Number(input.value)});}catch{input.value=data.settings.max_files;}finally{input.disabled=data.settings.track_all;}}
$("session-count").addEventListener("change",applySessionCount);$("apply-session-count").addEventListener("click",applySessionCount);
$("detail-close").addEventListener("click",()=>$("detail-dialog").close());
$("detail-dialog").addEventListener("close",()=>{detail=null;detailHistory.length=0;const nav=$("detail-content").previousElementSibling;if(nav?.classList.contains("modal-tabs"))nav.remove();clearPayloadObservers($("detail-content"));$("detail-content").replaceChildren();for(const key of tableViews.keys())if(key.startsWith("detail:"))tableViews.delete(key);updateDetailBack();});
$("detail-back").addEventListener("click",async()=>{const previous=detailHistory.pop();if(!previous)return;if(previous.view.kind==="jev")await openJev(previous.view.call,previous.view.thread,false,previous.scroll);else{openDetail(previous.view,false);$("detail-content").scrollTop=previous.scroll;}updateDetailBack();});
for(const dialog of document.querySelectorAll("dialog"))dialog.addEventListener("click",event=>{if(event.target!==dialog)return;const rect=dialog.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom)dialog.close();});
for(const tab of document.querySelectorAll("[data-tab]")){
  tab.classList.add("tab-order-row");tab.dataset.tabOrder=tab.dataset.tab;dragTab(tab,tab.dataset.tab,{root:document.querySelector(".tabs"),order:()=>tabOrder.filter(key=>!$("tab-"+key).hidden),label:key=>$("tab-"+key).textContent,move:moveTab,horizontal:true});
  tab.addEventListener("click",()=>switchTab(tab.dataset.tab));
  tab.addEventListener("keydown",event=>{if(event.altKey)return;if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const tabs=[...document.querySelectorAll("[data-tab]")].filter(t=>!t.hidden),index=tabs.indexOf(tab),next=event.key==="Home"?0:event.key==="End"?tabs.length-1:(index+(event.key==="ArrowRight"?1:tabs.length-1))%tabs.length;switchTab(tabs[next].dataset.tab);tabs[next].focus();});
}
for(const id of ["filter-type","filter-environment","filter-project","filter-trigger","filter-status","filter-reasoning","filter-model","page-size"])$(id).addEventListener("change",()=>{page=1;lazyLimit=50;if(data){renderCodex();attachTables();}saveView();});
$("thread-search").addEventListener("input",()=>{page=1;lazyLimit=50;if(data){renderCodex();attachTables();}saveView();});
$("filter-git").addEventListener("change",()=>{if(data){renderGit();renderCharts();attachTables();}saveView();});
for(const id of ["filter-file-operation","filter-file-method","filter-file-project","filter-file-tool"])$(id).addEventListener("change",()=>{if(data){renderFiles();renderCharts();attachTables();}saveView();});
$("file-search").addEventListener("input",()=>{if(data){renderFiles();renderCharts();attachTables();}saveView();});
for(const id of ["filter-mcp-category","filter-mcp-server","filter-mcp-result"])$(id).addEventListener("change",()=>{mcpPage=1;if(id==="filter-mcp-server"){mcpSource=$(id).value;preferences.mcpSource=mcpSource;}if(data){renderMcp();renderCharts();}attachTables();saveView();});
for(const [id,change]of [["mcp-prev",-1],["mcp-next",1]])$(id).addEventListener("click",()=>{mcpPage+=change;renderMcp();});
function loadMore(){if(!data||$("load-more").hidden)return;lazyLimit+=50;renderCodex();}
$("load-more").addEventListener("click",loadMore);
const lazyObserver=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting))loadMore();},{rootMargin:"150px"});lazyObserver.observe($("load-more"));
pageInput($("page-number"),()=>{page=Number($("page-number").value);if(data){renderCodex();attachTables();}saveView();});
for(const [id,action]of [["page-first",()=>1],["page-prev",()=>page-1],["page-next",()=>page+1],["page-last",()=>pageCount]])$(id).addEventListener("click",()=>{page=action();renderCodex();saveView();});
$("window").addEventListener("change",()=>{if($("window").value==="inherit")delete sourceWindows.tabs.mcp;else sourceWindows.tabs.mcp=$("window").value;applySourceWindow();});
for(const id of viewInputs){const value=preferences.inputs?.[id],input=$(id);if(typeof value==="string"&&!['filter-project','filter-git'].includes(id)&&(input.tagName!=="SELECT"||[...input.options].some(option=>option.value===value)))input.value=value;}
page=Number.isInteger(preferences.page)?preferences.page:1;
if(preferences.tab==="jev")preferences.tab="mcp";if(preferences.tab==="logs")preferences.tab="errors";if(["git","checks"].includes(preferences.tab))preferences.tab="workflow";
dragSubTabs($("diagnostic-tabs"),"diagnostics",tab=>tab.id);dragSubTabs($("activity-tabs"),"activity",tab=>tab.id);
bindSubPages("errors","diagnostic-tabs",diagnosticPages,diagnosticSource);bindSubPages("workflow","activity-tabs",activityPages,activitySource);
if([...document.querySelectorAll("[data-tab]")].some(tab=>tab.dataset.tab===preferences.tab))switchTab(preferences.tab);
discoverCopy();applyCopy();applyTabOrder();groupSettings();setupSourceWindowSettings();setupTabDescriptions();setupCollapsibleSections();fillDisplayOptions($("page-size"),preferences.inputs?.["page-size"]||display.table);
for(const id of ["activity-chart","git-time-chart","web-time-chart","file-time-chart","monitor-refresh-chart","monitor-cpu-chart","monitor-read-chart","sqlite-retained-chart","web-retained-chart","mcp-retained-chart","error-time-chart","skill-time-chart","tool-time-chart","check-time-chart","mcp-time-chart","sqlite-time-chart","tool-duration-chart","skill-thread-chart","git-thread-chart","check-thread-chart","log-time-chart","conversation-time-chart"])chartControls(id,true,id==="monitor-read-chart");
for(const id of ["top-tools","source-chart","model-chart","environment-chart","trigger-chart","mcp-action-chart","mcp-source-chart","git-chart","skill-chart","check-chart","file-operation-chart","error-source-chart","web-url-rank","web-site-rank","file-count-rank","folder-count-rank","file-read-count-rank","file-write-count-rank","file-read-rank","file-write-rank","file-size-rank"])chartControls(id);
for(const id of ["tool-breakdown-chart","skill-breakdown-chart","check-breakdown-chart","file-breakdown-chart","mcp-breakdown-chart","git-breakdown-chart","sqlite-breakdown-chart","web-url-time-chart","web-site-time-chart","file-path-time-chart","file-folder-time-chart","error-category-chart","error-level-chart","log-level-chart"])chartControls(id,true,true);
chartControls("conversation-model-chart");chartControls("conversation-environment-chart");chartControls("log-source-chart");chartControls("sqlite-operation-chart");chartControls("usage-model-chart");for(const id of ["overview-tools-chart","overview-token-chart","overview-error-chart","overview-file-chart","overview-sql-chart","overview-source-column","overview-model-ring","overview-model-pie","overview-token-stack"])chartControls(id);chartControls("overview-cpu-chart",true);for(const id of ["overview-git-chart","overview-skill-chart","overview-check-chart" ,"overview-dot-chart"])chartControls(id);for(const id of ["overview-web-chart","overview-error-time-chart","overview-refresh-chart","overview-read-chart"])chartControls(id,true);setupOverviewCopies();setupOverview();
pageInput($("mcp-page-number"),()=>{mcpPage=Number($("mcp-page-number").value);renderMcp();});
pageInput($("docs-page-number"),()=>{docsPage=Number($("docs-page-number").value);renderToolDocs();});
addTabSettings();
applyAppearance();loadLocales().then(()=>{applyLanguage();schedule(10);refresh();});
if(typeof ResizeObserver==="function"){const overviewObserver=new ResizeObserver(arrangeOverview);for(const panel of document.querySelector(".overview-panels").children)overviewObserver.observe(panel);}
if(typeof ResizeObserver==="function"){const contentObserver=new ResizeObserver(arrangeContentPanels);for(const grid of document.querySelectorAll("main .panels:not(.overview-panels),#mcp-dashboard-panels"))if(!grid.closest("dialog"))contentObserver.observe(grid);}
let chartResizeTimer;addEventListener("resize",()=>{clearTimeout(chartResizeTimer);chartResizeTimer=setTimeout(()=>{if(data)renderCharts();},150);});

for(const [id,value]of [["mcp-activity-tab","activity"],["mcp-files-tab","files"]]){$(id).addEventListener("click",()=>selectMcpSection(value));$(id).addEventListener("keydown",event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?"mcp-activity-tab":event.key==="End"?"mcp-files-tab":id==="mcp-files-tab"?"mcp-activity-tab":"mcp-files-tab";$(next).click();$(next).focus();});}
$("mcp-files-reload").addEventListener("click",()=>{const selected=mcpFileLists.get(mcpSource);if(selected&&!selected.mcpFilesLoading){selected.mcpFiles=null;selected.mcpFilesError=null;loadMcpFiles(selected);renderMcp();}});

async function loadToolContent(selected){selected.toolLoading=true;try{const event=selected.event;selected.toolContent=await lazyContent("/api/codex/tool",{thread_id:event.thread_id,call_id:event.call_id});selected.toolLoaded=true;}catch{selected.toolError=ui("工具執行內容無法讀取");}finally{selected.toolLoading=false;if(detail===selected)renderDetail();}}

$("global-table-heatmap").checked=!!display.heatmap;
$("global-table-heatmap").addEventListener("change",()=>{setGlobalHeatmap($("global-table-heatmap").checked);persistTables();feedback("display-message",ui("數值熱度已更新"));});

async function loadFileSizes(){if(fileSummaryBusy)return;const selected=activeSourceWindow();fileSnapshotRequests.add(selected);fileSummaryBusy=true;$("load-file-sizes").disabled=true;try{const response=await request("/api/codex/file-summary?"+new URLSearchParams({window:selected}),{cache:"no-store"});if(!response.ok)throw new Error();fileSnapshots.set(selected,response.data);renderFiles();renderCharts();attachTables();}catch{feedback("action-message",ui("無法讀取檔案資訊"),"error");}finally{fileSummaryBusy=false;$("load-file-sizes").disabled=false;}}
$("load-file-sizes").addEventListener("click",loadFileSizes);
