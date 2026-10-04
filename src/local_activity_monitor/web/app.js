"use strict";
const preferenceKey="local-activity-monitor.preferences.v1";
let dragEnabled=false;
let preferences={};try{preferences=JSON.parse(localStorage.getItem(preferenceKey)||"{}");if(!preferences||typeof preferences!=="object"||Array.isArray(preferences))preferences={};}catch{}
function editableCopy(value){return typeof value==="string"&&value===value.trim()&&value.length>=2&&value.length<=400&&/[A-Za-z\u3400-\u9fff]/.test(value)&&!/^(?:ms|px|bytes|tokens|分鐘|小時|秒鐘|\d+\s*(?:筆|項|秒|分鐘|小時|天))$/i.test(value);}
preferences.copy=Object.fromEntries(Object.entries(preferences.copy||{}).filter(([key,value])=>editableCopy(key)&&typeof value==="string"&&value.length<=400).slice(0,500));
function validDisplay(value){return value&&Array.isArray(value.options)&&value.options.length>=1&&value.options.length<=8&&value.options.every(n=>Number.isInteger(n)&&n>=1&&n<=200)&&new Set(value.options).size===value.options.length&&[...value.options,"all"].includes(value.ranking)&&[...value.options,"all"].includes(value.table);}
let display=validDisplay(preferences.display)?preferences.display:{options:[5,10,20],ranking:10,table:20};
const copyCatalog=new Set(),copyTargets=[];
let locale=["zh-TW","en","ja"].includes(preferences.locale)?preferences.locale:"zh-TW",translations={},translatedValues=new Set();
const defaultCopy={"資料保留與讀取":"監測資料與保存狀況","工作觀察台":"工作監測台","對話, 工具與專案操作":"對話與工具活動監測","程式狀態":"監測程式","Log 紀錄":"來源 Log","工具使用情況":"工具活動","Log 觀察與程式事件保存":"來源 Log 讀取與歷史回補","整理狀態":"資料更新狀態","本輪整理耗時":"本次資料更新耗時","每輪讀取上限":"每次讀取上限","保留效能樣本":"效能樣本數","保留狀態事件":"執行事件數","待完成的紀錄片段 (bytes)":"待解析資料 (bytes)","片段保留上限 (bytes)":"資料暫存上限 (bytes)","累計移除的舊工具紀錄":"已清理工具紀錄","累計 session 讀取量 (bytes)":"累計讀取量 (bytes)","損壞紀錄 (行)":"無法解析的紀錄 (行)","上次資料大小 (bytes)":"最近回應大小 (bytes)","上次傳輸大小 (bytes)":"最近傳輸大小 (bytes)","Model":"模型","Model 設定更新時間":"模型設定時間","Model 分布":"模型分布","Model Token 分布":"模型 Token 分布","Model 用量分析":"模型用量分析","Reasoning 等級":"推理等級","Input":"輸入 Token","Cached input":"快取輸入 Token","Output":"輸出 Token","Reasoning output":"推理輸出 Token","Total":"Token 合計","手動對話":"一般對話","exec 辨識":"程式碼辨識","exec 內辨識到的工具":"程式碼中辨識到的工具","原始文案":"預設文字","顯示文字":"自訂文字","用途說明":"工具用途","位置":"執行位置","過長紀錄 (行)":"超過長度上限 (行)","工具回傳狀態":"紀錄狀態","Session 讀取量 (bytes)":"檔案讀取量 (bytes)","Session 讀取量趨勢":"檔案讀取量趨勢"};
function preset(value){const base=defaultCopy[value]||value;return translations[locale]?.[base]||translations[locale]?.[value]||base;}
function ui(value){if(editableCopy(value)&&!translatedValues.has(value))copyCatalog.add(value);return Object.hasOwn(preferences.copy,value)?preferences.copy[value]:preset(value);}
const fieldDescriptions={"資料狀態":"額度取自最後讀取的來源紀錄, 不會隨時間推估用量. 到達重設時間後等待新的來源回報","Low 1% (P1)":"第 1 百分位數, 約有 1% 樣本不超過此值. 使用線性內插, 搭配平均值與 P99 觀察分布","統計範圍":"平均值, P99 與 Low 1% 使用的資料範圍, 同時列出實際參與統計的樣本數","Credits 餘額":"來源最近回報的 credits 餘額","已使用":"來源回報的額度使用百分比, 保留原始視窗長度與觀測時間","剩餘比例":"以 100 減去來源回報的使用百分比, 快照超過重設時間後需等來源更新","重設時間":"來源提供的額度視窗重設時間, 依目前語系與本機時區顯示","快取命中比例 (%)":"已載入對話的快取輸入除以輸入 Token","來源檔案":"可用來對照原始紀錄的檔名, 搭配時間與紀錄 ID 追查","Request ID":"來源提供的請求識別碼, 用來對照相同請求的診斷事件","Trace ID":"來源提供的追蹤或關聯識別碼, 用來查找同一操作的事件","Model":"目前來源提供的模型名稱","Reasoning 等級":"來源提供的推理強度設定","Input":"對話最新累計輸入 Token, 包含來源計入的快取輸入","Cached input":"已包含在輸入 Token 中的快取部分, 不再額外加總","Output":"來源提供的累計輸出 Token","Reasoning output":"來源提供的推理輸出 Token, 是否包含在輸出量中依來源定義","Total":"來源提供的最新累計 Token, 不加總歷次更新數值","狀態":"來源提供的對話狀態. 未提供時不推測是否仍在執行","工作總耗時 (秒)":"已觀察到開始與結束的工作時間加總. 尚在執行的工作持續計時","SQL 耗時 (ms)":"只顯示 SQL 診斷來源提供的個別操作耗時. 不以外層工具時間代替","影響列數":"SQL 診斷來源提供的 rows_affected. 不從查詢內容推算","回傳列數":"SQL 診斷來源提供的 rows_returned","工具整次耗時 (ms)":"整次外層工具呼叫的時間, 不代表其中單一 SQL 或內層操作的耗時","工具回傳狀態":"工具紀錄顯示外層回傳狀態. 診斷紀錄顯示來源是否回報錯誤, 不推測 SQL 是否成功","紀錄方式":"區分直接工具呼叫與程式碼中辨識的操作. 程式碼辨識不代表已執行成功","CPU 時間":"此監測程式更新資料時使用的 CPU 時間","平均值":"目前圖表區間內有效樣本的算術平均. 活動次數以每個時間格計算, 包含零活動格","P99":"第 99 百分位數, 約有 99% 樣本不超過此值. 使用線性內插, 少量樣本時需搭配樣本數判讀","樣本數":"目前統計區間內有效的資料筆數或時間格數","資料庫位置":"紀錄中能確認的 SQLite 檔案位置","SQL 操作":"從工具參數或 literal 程式碼辨識的 SQL 指令, 不顯示 SQL 內容或資料值","Log 等級":"沿用來源提供的 error, warn, info, debug, trace 等等級. 未知的新等級保留原值","已讀取 (bytes)":"此來源自啟動至今累計讀取的位元組數, 包含歷史回補","未辨識 (行)":"來源格式與目前解析方式不相符的行數, 可用來判斷是否需要更新解析器","歷史回補":"分批讀取程式啟動前的來源紀錄, 補上最近 24 小時可取得的錯誤摘要","資料來源與讀取範圍":"列出此頁使用的實際來源位置, 讀取方式與涵蓋範圍","每頁筆數":"每頁顯示的資料列數. 修改後立即套用並保存在此瀏覽器","觀察項目":"啟用或停用指定資料的讀取與辨識, 不會修改來源紀錄"};
for(const [key,text]of Object.entries(fieldDescriptions)){ui(key);ui(text);}
function fieldDescription(label){const entry=Object.entries(fieldDescriptions).find(([key])=>key===label||ui(key)===label);return entry?ui(entry[1]):null;}
function editableLabels(values){return Object.defineProperties({},Object.fromEntries(Object.entries(values).map(([key,text])=>{ui(text);return [key,{enumerable:true,get:()=>ui(text)}];})));}
function discoverCopy(){
  const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);let text;
  while(text=walker.nextNode()){if(text.parentElement.closest("script,style,[data-copy=ignore]"))continue;const original=text.textContent,base=original.trim();if(!editableCopy(base))continue;const start=original.indexOf(base);copyTargets.push({text,base,before:original.slice(0,start),after:original.slice(start+base.length)});ui(base);if(/^(?:H[1-4]|TH)$/.test(text.parentElement.tagName))text.parentElement.dataset.copyBase=base;}
  for(const el of document.querySelectorAll("[placeholder],[aria-label],[title]"))for(const attribute of ["placeholder","aria-label","title"]){const base=el.getAttribute(attribute);if(!el.closest("[data-copy=ignore]")&&editableCopy(base)){copyTargets.push({el,attribute,base});ui(base);}}
}
function applyCopy(){for(const target of copyTargets)if(target.text)target.text.textContent=target.before+ui(target.base)+target.after;else target.el.setAttribute(target.attribute,ui(target.base));}
async function loadLocales(){try{const response=await request("/locales.json",{cache:"no-store"});if(!response.ok||!response.data?.en||!response.data?.ja)throw new Error();translations=response.data;translatedValues=new Set(Object.values(translations).flatMap(Object.values));}catch{locale="zh-TW";$("action-message").textContent="語言檔無法讀取, 使用繁體中文";}}
function applyLanguage(){preferences.locale=locale;document.documentElement.lang=locale;numberFormat=new Intl.NumberFormat(locale);dateFormat=new Intl.DateTimeFormat(locale,{year:"numeric",month:"numeric",day:"numeric",hour:"2-digit",minute:"2-digit",second:"2-digit",hour12:false});applyCopy();$("language-select").value=locale;for(const tab of document.querySelectorAll(".modal-tabs button[data-copy-base]"))tab.textContent=ui(tab.dataset.copyBase);if(data)render(data);syncHelp();}
const $ = id => document.getElementById(id);
let numberFormat=new Intl.NumberFormat(locale),dateFormat=new Intl.DateTimeFormat(locale,{year:"numeric",month:"numeric",day:"numeric",hour:"2-digit",minute:"2-digit",second:"2-digit",hour12:false});
const fmt = value => value == null ? "--" : numberFormat.format(value);
function when(value){if(!value)return "--";const date=new Date(value);return Number.isFinite(date.getTime())?dateFormat.format(date):ui("未知");}
const labels = {
  status:editableLabels({ok:"完成",success:"完成",ready:"就緒",passed:"通過",failed:"失敗",error:"錯誤",timeout:"逾時",cancelled:"已取消",canceled:"已取消",partial:"部分完成",extracted:"已取得內容",written:"已寫入",preview:"預覽",unchanged:"未變更",fallback:"備用結果",skipped:"略過",dry_run:"試跑",running:"執行中",completed:"已完成",observed:"未知",pending:"等待中",queued:"排隊中",missing_dependencies:"缺少相依",used:"已執行",not_needed:"無需執行",disabled:"已停用"}),
  type:editableLabels({codex:"Codex",work:"ChatGPT Work",chat:"ChatGPT 對話",unknown:"未知"}),
  environment:editableLabels({local:"本機",cloud:"雲端",remote:"遠端",unknown:"未知"}),
  trigger:editableLabels({user:"手動對話",dot:"Dot",orbit:"Dot",schedule:"排程",automation:"排程",heartbeat:"Heartbeat",subagent:"Subagent",guardian_review:"自動審查",unknown:"未知"})
};
const tokenLabels = editableLabels({input_tokens:"Input",cached_input_tokens:"Cached input",output_tokens:"Output",reasoning_output_tokens:"Reasoning output",total_tokens:"Total"});
const observationLabels = editableLabels({codex:"對話紀錄",jev:"Jev 用量",metadata:"對話名稱與分類",usage:"用量與額度快照",git:"Git 操作",jev_calls:"Jev 送出與回傳內容",skills:"Skills 讀取",checks:"test / build / lint 操作",tool_events:"工具呼叫明細",mcp:"MCP 操作與結果",web:"網路工具與參考網址",files:"檔案讀寫紀錄",errors:"對話與工具錯誤",logs:"Log 觀察與程式事件保存",sqlite:"SQL / SQLite 操作"});
const actionLabels=editableLabels({read:"讀取",write:"寫入",change:"變更",deployment:"部署",operation:"操作"}),fileLabels=editableLabels({added:"新增",modified:"修改",deleted:"刪除",moved:"移動",write:"寫入",read:"讀取"});
const viewInputs=["thread-search","filter-type","filter-environment","filter-project","filter-trigger","filter-status","filter-reasoning","page-size","filter-git","window","filter-mcp-category","filter-mcp-server","filter-mcp-result","file-search","filter-file-operation","filter-file-method","filter-file-project","filter-file-tool"];
let pendingStatus=preferences.inputs?.["filter-status"];
let pendingFileProject=preferences.inputs?.["filter-file-project"],pendingFileTool=preferences.inputs?.["filter-file-tool"];
let loadedRevision=null,pendingRevision=null,preferencesApplied=false,pendingProject=preferences.inputs?.["filter-project"],pendingGit=preferences.inputs?.["filter-git"],pendingReasoning=preferences.inputs?.["filter-reasoning"];
let threadIndex=new Map();
let logData=null,logBusy=false,logQueued=false;
let data = null, page = 1, pageCount = 1, refreshTimer = null, refreshSeconds = 10;
let busy = false, refreshQueued = false, settingsBusy = false, version = 0, detail = null;
let lazyLimit=50,mcpPage=1,docsPage=1,docTools=[],chartSelection=null;
const docDrafts=new Map();
const chartViews=new Map(),systemDark=matchMedia("(prefers-color-scheme: dark)");
let appearance={mode:"dark",theme:"slate",accent:"green",font:14,...preferences.appearance};

function node(tag,text,cls){const el=document.createElement(tag);if(text!=null)el.textContent=text;if(cls)el.className=cls;return el;}
function button(text,action,cls){const el=node("button",text,cls);el.type="button";el.addEventListener("click",action);return el;}
function cell(row,text,cls){const el=node("td",text==null?null:text===ui("未知")||text===ui("--")?"--":text,cls);el.dataset.columnIndex=row.children.length;row.append(el);return el;}
function valueCell(row,value,cls){const el=cell(row,value==null?"--":fmt(value),cls);if(Number.isFinite(value))el.dataset.heatValue=String(value);el.dataset.sortValue=value==null?"":String(value);return el;}
function tag(text,cls=""){return node("span",text===ui("未知")?"--":text,"badge "+cls);}
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
  if(overviewFrame)return;overviewFrame=requestAnimationFrame(()=>{overviewFrame=0;const grid=document.querySelector(".overview-panels"),panels=[...grid.children].filter(panel=>!panel.hidden),heights=panels.map(panel=>panel.getBoundingClientRect().height);if(!heights.some(height=>height>0))return;grid.classList.add("masonry");const style=getComputedStyle(grid),gap=parseFloat(style.rowGap),step=parseFloat(style.gridAutoRows)+gap;for(let i=0;i<panels.length;i++)if(heights[i]>0)panels[i].style.gridRowEnd="span "+Math.ceil((heights[i]+gap)/step);});
}
function cards(id,items){(typeof id==="string"?$(id):id).replaceChildren(...items.map(([label,value,info])=>{const el=node("div",null,"card"),amount=node("div",value,"value"),caption=node("div",label,"label"),help=fieldDescription(label),parts=typeof value==="string"?value.match(/^([\d,.]+)\s+(.+)$/):null;if(parts)amount.replaceChildren(document.createTextNode(parts[1]+" "),node("span",parts[2],"value-unit"));if(help){caption.title=help;caption.tabIndex=0;caption.append(node("span"," ⓘ","help-mark"));}el.append(caption,amount,node("div",info,"detail"));return el;}));}
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
function dragAllowed(handle){const dialog=handle.closest("dialog");return dialog?dialog.dataset.layoutEdit==="true":dragEnabled;}
function syncModalDrag(dialog,reset=false){
  const enabled=!!dialog.querySelector(".direct-drag,.column-option,.tab-order-row,th[data-column-key]"),head=dialog.querySelector(".dialog-head");if(!head)return;
  let label=head.querySelector(".modal-layout-edit");if(enabled&&!label){label=node("label",null,"layout-edit-control modal-layout-edit");const caption=node("span",ui("拖曳排序")),control=node("input");control.type="checkbox";control.className="switch";control.setAttribute("role","switch");control.setAttribute("aria-label",ui("視窗內拖曳排序"));label.append(caption,control);head.insertBefore(label,head.lastElementChild);dialog.dataset.layoutEdit="false";control.addEventListener("change",()=>{dialog.dataset.layoutEdit=String(control.checked);hideHelp();hideChartTip();});}
  if(label){label.hidden=!enabled;label.querySelector("span").textContent=ui("拖曳排序");label.querySelector("input").setAttribute("aria-label",ui("視窗內拖曳排序"));if(reset){dialog.dataset.layoutEdit="false";label.querySelector("input").checked=false;}}
}
function openDialog(dialog){if(document.activeElement instanceof HTMLElement)tooltipSuppressed.add(document.activeElement);hideHelp();hideChartTip();syncModalDrag(dialog,true);dialog.showModal();syncHelp();}
function bindHelp(el,text){
  const base=[...copyCatalog].find(key=>key===text||ui(key)===text)||text;if(copyCatalog.has(base)&&editableCopy(base)&&base.length>30)helpCatalog.add(base);el.dataset.help=text;el.removeAttribute("title");el.setAttribute("aria-describedby",helpTip.id);if(el.dataset.helpBound)return;el.dataset.helpBound="true";
  function show(event){if(tooltipSuppressed.has(el)||el.classList.contains("help-dismissed")||el.closest("dialog")&&(event.type==="focusin"||el.matches("button,input,select")))return;hideChartTip();helpTarget=el;helpTip.textContent=el.dataset.help;(el.closest("dialog")||document.body).append(helpTip);helpTip.hidden=false;const rect=el.getBoundingClientRect(),x=event.type==="focusin"?rect.left:event.clientX,y=event.type==="focusin"?rect.bottom:event.clientY;placeTooltip(helpTip,x,y,el);}
  for(const event of ["pointermove","focusin"])el.addEventListener(event,show);for(const event of ["pointerleave","focusout"])el.addEventListener(event,()=>{if(helpTarget===el)hideHelp();});
  for(const event of ["pointerleave","focusout"])el.addEventListener(event,()=>el.classList.remove("help-dismissed"));
}
function syncHelp(){for(const el of document.querySelectorAll("[title]"))bindHelp(el,el.getAttribute("title"));for(const title of document.querySelectorAll("svg title"))title.remove();if(helpTarget&&!helpTarget.isConnected)hideHelp();}
document.addEventListener("keydown",event=>{if(event.key==="Escape"&&!helpTip.hidden){event.preventDefault();event.stopPropagation();helpTarget?.classList.add("help-dismissed");hideHelp();}},true);addEventListener("scroll",hideHelp,true);addEventListener("blur",hideHelp);
document.addEventListener("pointerdown",()=>{hideHelp();hideChartTip();},true);
document.addEventListener("pointermove",event=>{const el=event.target.closest?.("[data-help]");if(el)tooltipSuppressed.delete(el);},true);document.addEventListener("keydown",event=>{if(event.key==="Tab")tooltipSuppressed=new WeakSet();},true);for(const dialog of document.querySelectorAll("dialog"))dialog.addEventListener("close",()=>{if(document.activeElement instanceof HTMLElement)tooltipSuppressed.add(document.activeElement);hideHelp();hideChartTip();});
function hideChartTip(){chartTip.hidden=true;}
function showChartTip(heading,value,event,target){
  hideHelp();
  (target.closest("dialog")||document.body).append(chartTip);
  chartTip.replaceChildren(node("div",heading),node("strong",value));chartTip.hidden=false;
  const rect=target.getBoundingClientRect(),x=event?.clientX??rect.left+rect.width/2,y=event?.clientY??rect.top;
  placeTooltip(chartTip,x,y,target);
}
addEventListener("scroll",hideChartTip,true);addEventListener("blur",hideChartTip);
function bounds(id,items=[]){const s=chartViews.get(id)?.settings||{},end=s.range==="custom"?new Date(s.end).getTime():Date.now(),start=s.range==="all"?Math.min(...items.map(e=>new Date(e.timestamp||e.time||e.updated_at).getTime()).filter(Number.isFinite),end-86400000):s.range==="custom"?new Date(s.start).getTime():end-s.length*s.unit;return {start,end};}
function ranged(id,items){const {start,end}=bounds(id,items);return items.filter(item=>{const time=new Date(item.timestamp||item.time||item.updated_at).getTime();return Number.isFinite(time)&&time>=start&&time<=end;});}
function niceMax(value){if(value<=4)return Math.max(4,Math.ceil(value));const power=10**Math.floor(Math.log10(value/4)),step=Math.ceil(value/4/power)*power;return step*4;}
function statistics(values){const samples=values.filter(Number.isFinite).sort((a,b)=>a-b),size=samples.length;if(!size)return null;const percentile=ratio=>{const position=(size-1)*ratio,lower=Math.floor(position);return samples[lower]+(samples[Math.ceil(position)]-samples[lower])*(position-lower);};return {size,mean:samples.reduce((sum,value)=>sum+value,0)/size,p99:percentile(.99),p1:percentile(.01)};}
function measure(value,unit){if(unit==="bytes"){const scale=value>=1024**3?1024**3:value>=1024**2?1024**2:value>=1024?1024:1;return fmt(Math.round(value/scale*100)/100)+" "+({1:"B",1024:"KiB",1048576:"MiB",1073741824:"GiB"}[scale]);}return fmt(Math.round(value*100)/100)+" "+unit;}
function statisticLabels(stats,unit,basis){const text=node("span",null,"chart-statistics");for(const [label,value]of [["平均值",stats?.mean],["P99",stats?.p99],["Low 1% (P1)",stats?.p1],["統計範圍",basis?basis+" · "+fmt(stats?.size??0)+ui(" 筆樣本"):"--"]]){const item=node("span",null,"statistic-item");item.append(node("span",ui(label),"statistic-label"),node("b",label==="統計範圍"?value:Number.isFinite(value)?measure(value,unit):"--","statistic-value"));const help=fieldDescription(label);if(help){bindHelp(item,help);item.tabIndex=0;}text.append(item);}return text;}
function bars(id,counts,names,onClick,unit=ui("次"),segments){
  const s=chartViews.get(id)?.settings||{},entries=sorted(counts).slice(0,s.top===0?undefined:s.top||Number(display.ranking)||10),peak=Math.max(...entries.map(item=>item[1]),1),max=s.maximum||peak;
  if(["column","stacked"].includes(s.shape)){columnChart(id,entries,names,onClick,unit,s,segments);return;}
  if(["donut","pie"].includes(s.shape)){shareChart(id,counts,entries,names,onClick,unit,s.shape);return;}
  $(id).replaceChildren(...entries.map(([key,value])=>{
    const row=node("div",null,"bar-row"),label=names?.[key]||key;
    row.append(onClick?button(label,()=>onClick(key),"bar-label link"):node("span",label,"bar-label"),node("b",fmt(value)));
    const svg=svgNode("svg",{viewBox:"0 0 640 10",preserveAspectRatio:"none",role:"img","aria-label":label+" "+fmt(value)+" "+unit});
    svg.append(svgNode("rect",{width:640,height:10,rx:5,class:"bar-track"}),svgNode("rect",{width:640*Math.min(value,max)/max,height:10,rx:5,class:"bar-fill"}));
    const text=fmt(value)+" "+unit;row.tabIndex=onClick?-1:0;row.setAttribute("aria-label",label+": "+text);
    for(const event of ["pointerenter","pointermove","focusin"])row.addEventListener(event,e=>showChartTip(label,text,e.type==="focusin"?null:e,row));
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
    for(const [part,[name,amount]]of segments.entries()){if(!Number.isFinite(amount))continue;const shown=Math.min(amount,Math.max(0,max-offset)),rect=svgNode("rect",{x,y:240-(offset+shown)/max*188,width:slot*.6,height:shown/max*188,rx:2,class:"column-segment palette-"+(settings.shape==="stacked"?colors.get(name):index)%8});const text=(settings.shape==="stacked"?ui(name)+": ":"")+fmt(amount)+" "+unit;rect.tabIndex=0;rect.setAttribute("role",onClick?"button":"img");rect.setAttribute("aria-label",label+": "+text);for(const event of ["pointerenter","pointermove","focusin"])rect.addEventListener(event,e=>showChartTip(label,text,e.type==="focusin"?null:e,rect));for(const event of ["pointerleave","focusout"])rect.addEventListener(event,hideChartTip);if(onClick){rect.addEventListener("click",()=>onClick(key));rect.addEventListener("keydown",event=>{if(["Enter"," "].includes(event.key)){event.preventDefault();onClick(key);}});}svg.append(rect);offset+=amount;}
    const size=textSize,center=x+slot*.3,limit=slant?Math.min(100,(center-8)/.82):slot-8;let caption=label;while(caption.length>1&&size(caption+(caption===label?"":"…"))>limit)caption=caption.slice(0,-1);if(caption!==label)caption+="…";const tick=svgNode("text",{x:center,y:slant?260:270,"text-anchor":slant?"end":"middle",...(slant?{transform:"rotate(-35 "+center+" 260)"}:{}),class:"column-label"},caption);tick.tabIndex=0;tick.setAttribute("aria-label",label);for(const event of ["pointerenter","pointermove","focusin"])tick.addEventListener(event,e=>showChartTip(label,fmt(value)+" "+unit,e.type==="focusin"?null:e,tick));for(const event of ["pointerleave","focusout"])tick.addEventListener(event,hideChartTip);svg.append(svgNode("line",{x1:center,x2:center,y1:240,y2:245,class:"axis-line"}));if(index%labelStep===0){svg.append(tick);const valueText=textSize(fmt(value))<=slot-4?fmt(value):compact.format(value);if(textSize(valueText)<=slot*labelStep-4)svg.append(svgNode("text",{x:center,y:240-Math.min(value,max)/max*188-8,"text-anchor":"middle",class:"column-value"},valueText));}
  }
  for(const tick of svg.querySelectorAll(".column-label"))if(slant)labelBottom=Math.max(labelBottom,260+textSize(tick.textContent)*Math.sin(35*Math.PI/180)+font);
  const fittedHeight=Math.ceil(labelBottom+12);svg.setAttribute("viewBox","0 0 "+width+" "+fittedHeight);svg.style.height=fittedHeight+"px";
  wrap.append(svg);root.append(wrap);if(settings.shape==="stacked"){const legend=node("div",null,"column-legend");for(const label of categories){const item=node("span");item.append(node("span",null,"pie-swatch palette-"+colors.get(label)),node("span",ui(label)));legend.append(item);}bindHelp(legend,ui("缺少完整分類時以未分類總量顯示, 不補估 Token"));root.append(legend);}
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
    for(const event of ["pointerenter","pointermove","focusin"])segment.addEventListener(event,e=>showChartTip(item.label,info,e.type==="focusin"?null:e,segment));for(const event of ["pointerleave","focusout"])segment.addEventListener(event,hideChartTip);
    if(onClick&&item.key){segment.addEventListener("click",()=>onClick(item.key));segment.addEventListener("keydown",event=>{if(["Enter"," "].includes(event.key)){event.preventDefault();onClick(item.key);}});}
    svg.append(segment);angle=end;const row=node("div",null,"pie-legend-row"),swatch=node("span",null,"pie-swatch "+color);row.append(swatch,onClick&&item.key?button(item.label,()=>onClick(item.key),"link"):node("span",item.label),node("b",info));legend.append(row);
  }
  if(shape==="donut"){svg.append(svgNode("circle",{cx:120,cy:120,r:58,class:"pie-hole","pointer-events":"none"}),svgNode("text",{x:120,y:125,"text-anchor":"middle",class:"pie-unit","pointer-events":"none"},unit));}
  layout.append(svg,legend);root.append(layout,node("p",ui("合計 ")+fmt(total)+" "+unit,"chart-unit"));if(chartViews.get(id)?.settings.statistics!==0)root.append(statisticLabels(statistics(visible.map(item=>item[1])),unit,ui("目前顯示項目")));
}
function timeline(id,noteId,series,unit=ui("次"),average=false){
  const svg=$(id);svg.replaceChildren();svg.onpointermove=svg.onpointerleave=svg.onkeydown=svg.onfocus=svg.onblur=null;
  if(!series?.length){$(noteId).textContent="--";return;}
  const s=chartViews.get(id)?.settings||{},range=bounds(id,series),end=range.end;
  let interval=Math.max(60000,s.interval||3600000);while((end-range.start)/interval>240)interval*=2;
  const start=Math.floor(range.start/interval)*interval,slots=Math.max(1,Math.ceil((end-start)/interval)),values=Array.from({length:slots},(_,i)=>({time:start+interval*i,calls:0,samples:0}));
  for(const item of series){const time=new Date(item.time).getTime(),index=Math.floor((time-start)/interval);if(time>=range.start&&time<=end&&index>=0&&index<slots&&Number.isFinite(item.calls)){values[index].calls+=item.calls;values[index].samples++;}}
  if(average)for(const item of values)item.calls=item.samples?Math.round(item.calls/item.samples*100)/100:null;
  const stats=statistics(average?series.filter(item=>{const time=new Date(item.time).getTime();return time>=range.start&&time<=end;}).map(item=>item.calls):values.map(item=>item.calls));
  const peak=Math.max(...values.map(item=>item.calls||0),s.statistics===0?0:stats?.p99||0,1),max=s.maximum||niceMax(peak),scale=740/(svg.clientWidth||740),labelSize=Math.max(11,parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--font-size"))*.75||11),left=Math.min(300,Math.max(58,(fmt(max).length*labelSize*.66+12)*scale)),plotWidth=710-left,width=plotWidth/slots;
  svg.style.setProperty("--chart-label-size",labelSize*scale+"px");
  for(let i=0;i<=4;i++){const y=190-i*38;svg.append(svgNode("line",{x1:left,x2:710,y1:y,y2:y,class:"grid-line"}),svgNode("text",{x:left-10,y:y+4,"text-anchor":"end"},fmt(max*i/4)));}
  svg.append(svgNode("text",{x:left,y:20},unit),svgNode("line",{x1:left,x2:left,y1:38,y2:190,class:"axis-line"}),svgNode("line",{x1:left,x2:710,y1:190,y2:190,class:"axis-line"}));
  const points=[];values.forEach((item,i)=>{if(item.calls===null){points.push(null);return;}const height=Math.min(item.calls,max)/max*152,x=left+i*width;points.push((x+width/2)+","+(190-height));if(s.shape!=="line"){const rect=svgNode("rect",{x,y:190-height,width:Math.max(.5,width-1),height,rx:2});svg.append(rect);}});
  if(s.shape==="line"){let segment=[];const draw=()=>{if(segment.length>1)svg.append(svgNode("polyline",{points:segment.join(" "),class:"trend-line",fill:"none"}));else if(segment.length){const [cx,cy]=segment[0].split(",");svg.append(svgNode("circle",{cx,cy,r:3,class:"trend-dot"}));}segment=[];};for(const point of points)if(point===null)draw();else segment.push(point);draw();}
  if(stats&&s.statistics!==0)for(const [label,value,cls]of [["平均值",stats.mean,"mean-line"],["P99",stats.p99,"p99-line"],["Low 1% (P1)",stats.p1,"p1-line"]]){const y=190-Math.min(value,max)/max*152,line=svgNode("line",{x1:left,x2:710,y1:y,y2:y,class:"statistic-line "+cls});svg.append(line);}
  const cursor=svgNode("g",{class:"chart-cursor",visibility:"hidden","pointer-events":"none"}),guide=svgNode("line",{y1:38,y2:190}),dot=svgNode("circle",{r:5});cursor.append(guide,dot);svg.append(cursor);svg.tabIndex=0;
  let selected=0;
  function showPoint(index,event){
    selected=Math.max(0,Math.min(index,values.length-1));const item=values[selected],x=left+(selected+.5)*width,y=190-Math.min(item.calls,max)/max*152;
    guide.setAttribute("x1",x);guide.setAttribute("x2",x);dot.setAttribute("cx",x);dot.setAttribute("cy",y);dot.setAttribute("visibility",item.calls===null?"hidden":"visible");cursor.setAttribute("visibility","visible");
    const heading=when(new Date(Math.max(item.time,range.start)).toISOString())+" - "+when(new Date(Math.min(item.time+interval,end)).toISOString()),text=(item.calls===null?"--":fmt(item.calls)+" "+unit);showChartTip(heading,text,event,svg);
    svg.setAttribute("aria-label",ui(chartViews.get(id)?.title||"活動趨勢")+" · "+heading+": "+text);
  }
  svg.onpointermove=event=>{const matrix=svg.getScreenCTM();if(!matrix)return;const point=svg.createSVGPoint();point.x=event.clientX;point.y=event.clientY;const local=point.matrixTransform(matrix.inverse());if(local.x<left||local.x>710||local.y<38||local.y>190){cursor.setAttribute("visibility","hidden");hideChartTip();return;}showPoint(Math.floor((local.x-left)/width),event);};
  svg.onfocus=()=>showPoint(selected);svg.onkeydown=event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();showPoint(event.key==="Home"?0:event.key==="End"?values.length-1:selected+(event.key==="ArrowRight"?1:-1));};
  svg.onpointerleave=svg.onblur=()=>{cursor.setAttribute("visibility","hidden");hideChartTip();};
  const ticks=Math.max(1,Math.min(4,Math.floor(plotWidth/scale/(labelSize*10))));for(let i=0;i<=ticks;i++){const time=start+(end-start)*i/ticks,x=left+plotWidth*i/ticks,label=new Date(time).toLocaleString(locale,{month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit",hour12:false});svg.append(svgNode("line",{x1:x,x2:x,y1:190,y2:196}),svgNode("text",{x,y:218,"text-anchor":i===0?"start":i===ticks?"end":"middle"},label));}
  const summary=node("span",ui("每格 ")+fmt(interval/60000)+ui(" 分鐘 · 最高 ")+fmt(Math.max(...values.map(v=>v.calls||0),0))+" "+unit+(average?ui(" · 每格平均"):"")+(peak>max?ui(" · 超過上限的數值已截短"):""),"chart-note-summary");bindHelp(summary,summary.textContent);$(noteId).replaceChildren(summary);
  if(stats&&s.statistics!==0)$(noteId).append(statisticLabels(stats,unit,ui(average?"區間內更新樣本":"區間內時間格, 含零活動")));
}
function hourly(events){const counts=Object.create(null);for(const event of events)if(event.timestamp){const key=event.timestamp.slice(0,13)+":00:00Z";counts[key]=(counts[key]||0)+1;}return Object.entries(counts).sort().map(([time,calls])=>({time,calls}));}
const overviewShapes={"activity-chart":["line","折線圖"],"source-chart":["bar","橫條圖"],"model-chart":["column","直條圖"],"environment-chart":["bar","橫條圖"],"trigger-chart":["bar","橫條圖"],"overview-tools-chart":["bar","橫條圖"],"overview-token-chart":["bar","橫條圖"],"overview-error-chart":["bar","橫條圖"],"overview-file-chart":["column","直條圖"],"overview-sql-chart":["column","直條圖"],"overview-cpu-chart":["line","折線圖"],"overview-source-column":["column","直條圖"],"overview-model-ring":["donut","環圈圖"],"overview-model-pie":["pie","圓餅圖"],"overview-token-stack":["stacked","堆疊直條圖"],"overview-web-chart":["line","折線圖"],"overview-error-time-chart":["line","折線圖"],"overview-refresh-chart":["line","折線圖"],"overview-read-chart":["line","折線圖"],"overview-git-chart":["bar","橫條圖"],"overview-skill-chart":["bar","橫條圖"],"overview-check-chart":["bar","橫條圖"],"overview-quota-chart":["bar","橫條圖"],"overview-dot-chart":["bar","橫條圖"]};
function chartControls(id,trend=false){
  const controls=node("div",null,"chart-options"),form=node("div",null,"chart-controls"),saved=preferences.charts?.[id]||{};
  const localDate=time=>new Date(time-new Date(time).getTimezoneOffset()*60000).toISOString().slice(0,16);
  const settings={range:trend?"recent":"all",length:id.startsWith("monitor-")?60:24,unit:id.startsWith("monitor-")?60000:3600000,interval:id.startsWith("monitor-")?60000:3600000,top:display.ranking==="all"?0:display.ranking,maximum:0,statistics:trend?1:0,shape:"bar",start:localDate(Date.now()-86400000),end:localDate(Date.now())};
  const fields={};
  function copyNode(tag,text){const el=node(tag,ui(text));if(editableCopy(text))copyTargets.push({text:el.firstChild,base:text,before:"",after:""});return el;}
  function select(key,label,items){const el=node("select");for(const [value,text]of items){const option=copyNode("option",text);option.value=value;el.append(option);}fields[key]=el;const wrap=copyNode("label",label);wrap.append(el);form.append(wrap);}
  function input(key,label,type,min,max){const el=node("input");el.type=type;if(min!=null)el.min=min;if(max!=null)el.max=max;fields[key]=el;const wrap=copyNode("label",label);wrap.append(el);form.append(wrap);}
  select("range","時間範圍",[["all","全部紀錄"],["recent","最近一段時間"],["custom","指定起訖"]]);
  input("length","最近","number",1,365);select("unit","時間單位",[[60000,"分鐘"],[3600000,"小時"],[86400000,"天"]]);
  input("start","開始時間","datetime-local");input("end","結束時間","datetime-local");
  if(trend)select("interval","時間間隔",[[60000,"1 分鐘"],[300000,"5 分鐘"],[900000,"15 分鐘"],[3600000,"1 小時"],[21600000,"6 小時"],[86400000,"1 天"]]);
  else select("top","顯示項目",[...display.options.map(value=>[value,"前 "+value+" 項"]),[0,"全部"]]);
  const types=overviewShapes[id]?[overviewShapes[id]]:trend?[["bar","長條圖"],["line","折線圖"]]:[["bar","橫條圖"],["column","直條圖"],...(["usage-model-chart","overview-token-chart"].includes(id)?[["stacked","堆疊直條圖"]]:[]),["donut","環圈圖"],["pie","圓餅圖"]],shape=saved.shape;
  if(overviewShapes[id])settings.shape=overviewShapes[id][0];else if(types.some(([key])=>key===shape))settings.shape=shape;
  input("maximum",trend?"Y 軸上限 (0 = 自動)":"數值上限 (0 = 自動)","number",0,1000000000);
  if(trend)select("statistics","統計指標",[[1,"顯示平均值與百分位數"],[0,"隱藏統計指標"]]);
  for(const [key,el]of Object.entries(fields)){const value=saved[key]??settings[key];if(el.tagName==="SELECT"?[...el.options].some(o=>o.value===String(value)):el.type==="number"?Number.isFinite(Number(value))&&Number(value)>=Number(el.min)&&Number(value)<=Number(el.max):typeof value==="string")settings[key]=["length","unit","interval","top","maximum","statistics"].includes(key)?Number(value):value;el.value=settings[key];}
  function showFields(){fields.maximum.parentElement.hidden=!trend&&["donut","pie"].includes(settings.shape);for(const key of ["length","unit"])fields[key].parentElement.hidden=settings.range!=="recent";for(const key of ["start","end"])fields[key].parentElement.hidden=settings.range!=="custom";}
  form.addEventListener("change",event=>{const key=Object.entries(fields).find(([,el])=>el===event.target)?.[0];if(!key)return;if(key==="length")roundNumber(event.target);const next={...settings,[key]:["length","unit","interval","top","maximum","statistics"].includes(key)?Number(event.target.value):event.target.value};if(!event.target.checkValidity()||next.range==="custom"&&(!next.start||!next.end||new Date(next.start)>=new Date(next.end))){event.target.setAttribute("aria-invalid","true");feedback("chart-settings-message",ui("請檢查時間起訖或數值"),"error");return;}event.target.removeAttribute("aria-invalid");Object.assign(settings,next);showFields();saveView();if(data)renderCharts();feedback("chart-settings-message",ui("圖表已更新"));});
  showFields();controls.append(form);controls.hidden=true;const heading=$(id).previousElementSibling,head=node("div",null,"panel-head chart-head"),actions=node("div",null,"chart-actions"),open=button("⚙",()=>openChartSettings(id),"chart-setting-button"),cycle=button("",()=>{settings.shape=types[(types.findIndex(([key])=>key===settings.shape)+1)%types.length][0];showFields();saveView();renderCharts();},"chart-type-button");
  function updateType(){const index=types.findIndex(([key])=>key===settings.shape),label=ui(types[index][1]),next=ui(types[(index+1)%types.length][1]);cycle.textContent=label+" ↻";cycle.setAttribute("aria-label",ui(heading.dataset.copyBase||heading.textContent)+ui(" · 切換圖表類型, 目前 ")+label);cycle.title=ui("下一個: ")+next;}
  updateType();cycle.hidden=!!overviewShapes[id];cycle.dataset.chartType=id;open.setAttribute("aria-label",heading.textContent+ui(" 圖表設定"));heading.replaceWith(head);actions.append(cycle,open);head.append(heading,actions);$(id).before(controls);chartViews.set(id,{settings,fields,controls,title:heading.dataset.copyBase||heading.textContent,updateType});
}
function openChartSettings(id){$("chart-settings-message").textContent="";chartSelection=id;const view=chartViews.get(id);$("chart-settings-title").textContent=ui(view.title)+ui(" · 圖表設定");view.controls.hidden=false;$("chart-settings-content").replaceChildren(view.controls);if(!$("chart-dialog").open)openDialog($("chart-dialog"));}
$("chart-settings-close").addEventListener("click",()=>$("chart-dialog").close());$("chart-dialog").addEventListener("close",()=>{if(chartSelection){const view=chartViews.get(chartSelection);view.controls.hidden=true;$(chartSelection).before(view.controls);chartSelection=null;}});
function renderCharts(){
  if(!data)return;for(const view of chartViews.values())view.updateType();const c=data.codex,threads=c.threads||[],events=data.mcp?.events||[];
  bars("usage-model-chart",usageModelCounts,{unknown:"--"},null,"tokens",usageModelSegments);
  timeline("activity-chart","activity-chart-note",c.activity_series||[]);
  bars("top-tools",ranged("top-tools",c.tool_series||[]).reduce((out,e)=>(out[e.tool]=(out[e.tool]||0)+e.calls,out),Object.create(null)),null,tool=>openDetail({kind:"tool",tool}));
  bars("overview-source-column",countBy(ranged("overview-source-column",events),"server"));bars("overview-model-ring",countBy(ranged("overview-model-ring",threads),"model"),{unknown:"--"},null,ui("對話數"));bars("overview-model-pie",countBy(ranged("overview-model-pie",threads),"model"),{unknown:"--"},null,ui("對話數"));bars("overview-token-stack",usageModelCounts,{unknown:"--"},null,"tokens",usageModelSegments);
  bars("source-chart",countBy(ranged("source-chart",events),"server"),null,server=>{if(server==="web"){switchTab("web");return;}switchTab("mcp");selectMcpSource(server);});
  bars("model-chart",countBy(ranged("model-chart",threads),"model"),{unknown:ui("未知")},null,ui("對話數"));
  bars("environment-chart",countBy(ranged("environment-chart",threads),"environment"),labels.environment,null,ui("對話數"));
  bars("trigger-chart",countBy(ranged("trigger-chart",threads),"trigger"),labels.trigger,null,ui("對話數"));
  bars("mcp-action-chart",countBy(ranged("mcp-action-chart",events.filter(e=>e.server!=="web"&&(mcpSource==="all"||e.server===mcpSource))),"action"),actionLabels);
  bars("sqlite-operation-chart",countBy(ranged("sqlite-operation-chart",c.sqlite?.events||[]),"operation"),sqlLabels);
  const git=(c.git?.events||[]).filter(e=>$("filter-git").value==="all"||e.operation===$("filter-git").value);
  bars("git-chart",countBy(ranged("git-chart",git),"operation"),null,key=>{$("filter-git").value=key;renderGit();renderCharts();});
  timeline("git-time-chart","git-chart-note",hourly(git));
  bars("skill-chart",countBy(ranged("skill-chart",c.skills?.events||[]),"skill"),null,skill=>openDetail({kind:"skill",skill}));
  bars("check-chart",countBy(ranged("check-chart",c.checks||[]),"operation"));
  timeline("web-time-chart","web-chart-note",hourly(events.filter(e=>e.server==="web")));
  const files=filteredFiles();bars("file-operation-chart",countBy(ranged("file-operation-chart",files),"operation"),fileLabels);timeline("file-time-chart","file-chart-note",hourly(files));
  const errors=data.errors?.events||[];
  bars("error-source-chart",countBy(ranged("error-source-chart",errors),"category"),errorCategories,()=>switchTab("errors"));
  timeline("error-time-chart","error-chart-note",errors.map(e=>({time:e.timestamp,calls:1})),"次");
  for(const [id,key,unit,average]of [["refresh","refresh_ms","ms",true],["cpu","cpu_ms","ms",true],["read","read_bytes","bytes",false]])timeline("monitor-"+id+"-chart","monitor-"+id+"-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample[key]})),unit,average);
  bars("overview-tools-chart",ranged("overview-tools-chart",c.tool_series||[]).reduce((out,e)=>(out[e.tool]=(out[e.tool]||0)+e.calls,out),Object.create(null)));bars("overview-token-chart",usageModelCounts,{unknown:"--"},null,"tokens",usageModelSegments);bars("overview-error-chart",countBy(ranged("overview-error-chart",errors),"category"),errorCategories);bars("overview-file-chart",countBy(ranged("overview-file-chart",c.file_activity?.events||[]),"operation"),fileLabels);bars("overview-sql-chart",countBy(ranged("overview-sql-chart",c.sqlite?.events||[]),"operation"),sqlLabels);timeline("overview-cpu-chart","overview-cpu-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample.cpu_ms})),"ms",true);bars("overview-git-chart",countBy(ranged("overview-git-chart",c.git?.events||[]),"operation"),null,()=>{switchTab("workflow");selectSubPage("workflow","git","activity-tabs",activityPages);});
  bars("overview-skill-chart",countBy(ranged("overview-skill-chart",c.skills?.events||[]),"skill"));bars("overview-check-chart",countBy(ranged("overview-check-chart",c.checks||[]),"operation"));
  timeline("overview-web-chart","overview-web-note",hourly(events.filter(event=>event.server==="web")));timeline("overview-error-time-chart","overview-error-time-note",errors.map(event=>({time:event.timestamp,calls:1})),"次");
  for(const [id,key,unit,average]of [["refresh","refresh_ms","ms",true],["read","read_bytes","bytes",false]])timeline("overview-"+id+"-chart","overview-"+id+"-note",(data.monitor?.history||[]).map(sample=>({time:sample.time,calls:sample[key]})),unit,average);
  renderAllowance("overview-quota-chart",c.usage);
  bars("overview-dot-chart",countBy(ranged("overview-dot-chart",c.dots?.events||[]),"artifact_type"),null,null,ui("項"));
  applyOverview();for(const view of tableViews.values())adaptTable(view);arrangeOverview();
  arrangeContentCards();syncHelp();
}
function applyAppearance(){
  if(!["auto","light","dark"].includes(appearance.mode))appearance.mode="dark";
  if(!["slate","neutral"].includes(appearance.theme))appearance.theme="slate";
  if(!["green","blue","orange"].includes(appearance.accent))appearance.accent="green";
  appearance.font=Math.min(18,Math.max(12,Number.isInteger(appearance.font)?appearance.font:14));
  const mode=appearance.mode==="auto"?(systemDark.matches?"dark":"light"):appearance.mode,root=document.documentElement;
  root.dataset.mode=mode;root.dataset.theme=appearance.theme;root.dataset.accent=appearance.accent;root.dataset.font=appearance.font;
  $("dark-toggle").setAttribute("aria-pressed",String(mode==="dark"));$("dark-toggle").title=mode==="dark"?ui("切換淺色模式"):ui("切換深色模式");$("dark-toggle").setAttribute("aria-label",$("dark-toggle").title);
  for(const el of document.querySelectorAll("[data-mode]"))el.setAttribute("aria-pressed",String(el.dataset.mode===appearance.mode));
  $("theme-select").value=appearance.theme;$("accent-select").value=appearance.accent;$("font-size").value=appearance.font;if(data)for(const view of tableViews.values())adaptTable(view);syncHelp();
}
let diagnosticSource=preferences.tab==="logs"?"logs":preferences.diagnosticSource||"errors",activitySource=["git","checks"].includes(preferences.tab)?preferences.tab:["git","checks"].includes(preferences.activitySource)?preferences.activitySource:"git";
function selectSubPage(parent,key,navId,entries){for(const [value,tabId,panelId]of entries){const active=value===key;$(tabId).setAttribute("aria-selected",String(active));$(tabId).tabIndex=active?0:-1;$(panelId).hidden=!active;}if(parent==="errors"){diagnosticSource=key;preferences.diagnosticSource=key;if(key==="logs"&&data&&!$("view-errors").hidden)loadLogs();}else{activitySource=key;preferences.activitySource=key;}if(data){renderCharts();saveView();}}
const diagnosticPages=[["errors","diagnostic-errors","error-content"],["logs","tab-logs","view-logs"]],activityPages=[["git","tab-git","view-git"],["checks","tab-checks","view-checks"]];
function bindSubPages(parent,navId,entries,selected){for(const [index,[key,tabId]]of entries.entries()){$(tabId).addEventListener("click",()=>selectSubPage(parent,key,navId,entries));$(tabId).addEventListener("keydown",event=>{if(event.altKey||!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?entries.length-1:(index+(["ArrowRight","ArrowDown"].includes(event.key)?1:entries.length-1))%entries.length;$(entries[next][1]).click();$(entries[next][1]).focus();});}selectSubPage(parent,selected,navId,entries);}
function switchTab(name){
  if(["errors","logs"].includes(name)){selectSubPage("errors",name==="logs"?"logs":diagnosticSource,"diagnostic-tabs",diagnosticPages);name="errors";}
  if(["git","checks","workflow"].includes(name)){selectSubPage("workflow",name==="workflow"?activitySource:name,"activity-tabs",activityPages);name="workflow";}
  if(name==="jev"){name="mcp";mcpSource="jev";if(data)renderMcp();}
  for(const tab of document.querySelectorAll("[data-tab]")){const active=tab.dataset.tab===name;tab.setAttribute("aria-selected",String(active));tab.tabIndex=active?0:-1;$("view-"+tab.dataset.tab).hidden=!active;}
  if(data){renderCharts();saveView();}
  if(name==="errors"&&diagnosticSource==="logs"&&data)loadLogs();
}
function filteredThreads(){
  const query=$("thread-search").value.trim().toLocaleLowerCase(),filters=[["filter-type","activity_type"],["filter-environment","environment"],["filter-status","status"],["filter-reasoning","reasoning_effort"]];
  return (data?.codex.threads||[]).filter(t=>{
    if(query&&!(title(t)+" "+t.thread_id).toLocaleLowerCase().includes(query))return false;
    if(filters.some(([id,key])=>$(id).value!=="all"&&$(id).value!==(t[key]||"unknown")))return false;
    const project=$("filter-project").value,trigger=$("filter-trigger").value;
    if(project!=="all"&&project!==(t.project_scope||"unknown")&&project!=="id:"+t.project_id)return false;
    const source=t.trigger==="orbit"?"dot":t.trigger==="automation"?"schedule":t.trigger||"unknown";
    return trigger==="all"||trigger===source||trigger==="schedule"&&t.has_schedule;
  });
}
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
function renderCodex(){
  const c=data.codex;
  selectOptions("filter-status",[["all",ui("全部狀態")],...[...new Set(["running","completed","observed",...(c.threads||[]).map(t=>t.status).filter(Boolean)])].map(value=>[value,labels.status[value]||value])]);
  if(pendingStatus){if([...$("filter-status").options].some(option=>option.value===pendingStatus))$("filter-status").value=pendingStatus;pendingStatus=null;}
  selectOptions("filter-reasoning",[["all",ui("全部等級")],...[...new Set((c.threads||[]).map(t=>t.reasoning_effort).filter(Boolean))].sort().map(value=>[value,value]),["unknown",ui("未知")]]);
  if(pendingReasoning){if([...$("filter-reasoning").options].some(option=>option.value===pendingReasoning))$("filter-reasoning").value=pendingReasoning;pendingReasoning=null;}
  const threads=sortRecords("codex-rows",filteredThreads(),[title,t=>t.thread_id,t=>t.project_name||t.project_id,t=>labels.type[t.activity_type],t=>labels.environment[t.environment],t=>labels.trigger[t.trigger],t=>t.model,t=>labels.status[t.status]||t.status,t=>t.reasoning_effort,...Object.keys(tokenLabels).map(key=>t=>t.tokens?.[key]),t=>t.tool_calls,t=>t.task_duration_ms,t=>t.updated_at],t=>t.updated_at),tools=toolCounts(threads),all=$("page-size").value==="all";
  const size=all?Math.max(threads.length,1):Number($("page-size").value);
  pageCount=Math.max(1,Math.ceil(threads.length/size));page=Math.min(Math.max(page,1),pageCount);
  const start=(page-1)*size,visible=threads.slice(start,start+(all?lazyLimit:size));
  $("codex-scope").textContent=c.health==="disabled"?ui("觀察已關閉"):ui("追蹤 ")+fmt(c.files||0)+ui(" 個 session 檔案 · ")+fmt(c.threads?.length||0)+ui(" 個對話");
  if(!settingsBusy)$("track-all").checked=!!data.settings?.track_all;
  cards("codex-cards",[[ui("符合篩選的對話"),fmt(threads.length),ui("全部 ")+fmt(c.threads?.length||0)+ui(" 個")],[ui("執行中"),fmt(threads.filter(t=>t.status==="running").length),ui("已完成 ")+fmt(threads.filter(t=>t.status==="completed").length)+ui(" 個")],[ui("工具呼叫"),fmt(Object.values(tools).reduce((sum,count)=>sum+count,0)),ui("工具種類 ")+fmt(Object.keys(tools).length)],[ui("累計讀取量"),fmt(Math.round((c.bytes_read||0)/1024))+" KiB",ui("損壞紀錄 ")+fmt(c.malformed_lines||0)+ui(" 行")]]);
  replaceRows("codex-rows",...visible.map(t=>{
    const row=node("tr"),identity=cell(row,null,"thread-cell");
    identity.append(node("span",title(t),"thread-name"));
    cell(row,t.thread_id,"mono thread-id");cell(row,t.project_name||t.project_id||(t.project_scope==="none"?ui("無專案"):"--"));
    for(const value of [labels.type[t.activity_type],labels.environment[t.environment],labels.trigger[t.trigger]])cell(row).append(tag(value||"--"));
    cell(row,t.model||"--","mono");cell(row).append(statusBadge(t));
    cell(row).append(tag(t.reasoning_effort||"--"));
    for(const key of Object.keys(tokenLabels))valueCell(row,t.tokens?.[key]);
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
  cards("tool-cards",[[ui("工具呼叫"),fmt(c.observed_tool_calls||0),ui("實際工具呼叫紀錄")],[ui("工具種類"),fmt(Object.keys(tools).length),ui("不同工具名稱")],[ui("exec 辨識"),fmt(Object.values(nested).reduce((n,value)=>n+value,0)),ui("內層工具 ")+fmt(Object.keys(nested).length)+ui(" 種")],[ui("有回傳時間"),fmt(threads.flatMap(t=>t.tool_events||[]).filter(e=>e.completed_at).length),ui("目前工具明細")]]);
  $("tools").replaceChildren(...sorted(tools).map(([tool,count])=>{const el=toolButton(tool,()=>openDetail({kind:"tool",tool}),count);return el;}));
  if(!Object.keys(tools).length)$("tools").append(node("p","--","empty"));
  $("nested-tools").replaceChildren(...sorted(nested).map(([tool,count])=>{const el=toolButton(tool,()=>openDetail({kind:"nested-tool",tool}),count);return el;}));
  if(!Object.keys(nested).length)$("nested-tools").append(node("p","--","empty"));
  attachTables();
}
let mcpSource=typeof preferences.mcpSource==="string"?preferences.mcpSource:preferences.tab==="jev"?"jev":"all";
function selectMcpSource(source){
  mcpSource=source;preferences.mcpSource=source;mcpPage=1;$("filter-mcp-server").value=source;$("filter-mcp-category").value="all";$("filter-mcp-result").value="all";
  if(data){renderMcp();renderCharts();attachTables();saveView();}
}

function dragSubTabs(nav,key,identity){
  const tabs=[...nav.children],saved=Array.isArray(preferences.subOrders?.[key])?preferences.subOrders[key]:[],order=[...new Set(saved.filter(id=>tabs.some(tab=>identity(tab)===id)).concat(tabs.map(identity)))];
  for(const id of order)nav.append(tabs.find(tab=>identity(tab)===id));
  const config={root:nav,horizontal:true,order:()=>[...nav.children].map(tab=>tab.dataset.tabOrder),label:id=>[...nav.children].find(tab=>tab.dataset.tabOrder===id)?.textContent||id,move:(id,target,after)=>{const order=config.order().filter(key=>key!==id),index=order.indexOf(target);if(index<0||id===target)return;order.splice(index+(after?1:0),0,id);for(const key of order)nav.append([...nav.children].find(tab=>tab.dataset.tabOrder===key));preferences.subOrders={...preferences.subOrders,[key]:order};saveView();}};
  for(const tab of nav.children){tab.classList.add("tab-order-row");tab.dataset.tabOrder=identity(tab);if(tab.dataset.dragWired)return;tab.dataset.dragWired="true";dragTab(tab,tab.dataset.tabOrder,config);tab.addEventListener("keydown",event=>{if(event.altKey||!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();event.stopImmediatePropagation();const tabs=[...nav.children].filter(tab=>!tab.hidden),index=tabs.indexOf(tab),next=event.key==="Home"?0:event.key==="End"?tabs.length-1:(index+(event.key==="ArrowRight"?1:tabs.length-1))%tabs.length;tabs[next].click();tabs[next].focus();},true);}
}
function arrangeHighlights(){const root=$("overview-highlights"),cards=[...root.children],saved=Array.isArray(preferences.highlightOrder)?preferences.highlightOrder:[],order=[...new Set(saved.filter(id=>cards.some(card=>card.dataset.tabOrder===id)).concat(cards.map(card=>card.dataset.tabOrder)))];for(const id of order)root.append(cards.find(card=>card.dataset.tabOrder===id));const config={root,grid:true,order:()=>[...root.children].map(card=>card.dataset.tabOrder),label:id=>[...root.children].find(card=>card.dataset.tabOrder===id)?.querySelector("h3").textContent||id,move:(id,target,after)=>{const order=config.order().filter(key=>key!==id),index=order.indexOf(target);if(index<0||id===target)return;order.splice(index+(after?1:0),0,id);for(const key of order)root.append([...root.children].find(card=>card.dataset.tabOrder===key));preferences.highlightOrder=order;saveView();}};for(const card of root.children)dragTab(card,card.dataset.tabOrder,config);}


function dragCardGroup(root,key,identity){const cards=[...root.children].filter(card=>card.matches(".panel,.source-card"));if(cards.length<2)return;const saved=Array.isArray(preferences.cardOrders?.[key])?preferences.cardOrders[key]:[],order=[...new Set(saved.filter(id=>cards.some(card=>identity(card)===id)).concat(cards.map(identity)))];for(const id of order)root.append(cards.find(card=>identity(card)===id));const config={root,grid:true,order:()=>[...root.children].filter(card=>!card.hidden).map(identity),label:id=>cards.find(card=>identity(card)===id)?.querySelector("h3,.source-name").textContent||id,move:(id,target,after)=>{const order=[...root.children].map(identity).filter(key=>key!==id),index=order.indexOf(target);if(index<0||id===target)return;order.splice(index+(after?1:0),0,id);for(const key of order)root.append(cards.find(card=>identity(card)===key));preferences.cardOrders={...preferences.cardOrders,[key]:order};saveView();}};for(const card of cards){card.classList.add("tab-order-row");card.dataset.tabOrder=identity(card);const handle=card.tagName==="BUTTON"?card:card.querySelector("h3");if(!handle||handle.dataset.cardDragWired)continue;handle.dataset.cardDragWired="true";if(handle.tagName!=="BUTTON"){handle.tabIndex=0;handle.classList.add("direct-drag");}handle.title=ui("拖曳調整順序, 或用方向鍵上下移動");dragTab(handle,identity(card),config);}}
function arrangeContentCards(){for(const [index,root]of [...document.querySelectorAll(".panels:not(.overview-panels)")].entries()){for(const [position,card]of [...root.children].entries())card.dataset.cardKey||=card.querySelector(".chart,.bar-chart,[id]")?.id||card.id||"card-"+position;dragCardGroup(root,"panels-"+index,card=>card.dataset.cardKey);}dragCardGroup($("mcp-source-cards"),"mcp-sources",card=>card.dataset.sourceKey);}

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
function metricLabel(key){const credits={credits:"Credits",credits_balance:"Credits 餘額",credits_used:"已使用 credits",credits_remaining:"剩餘 credits",credits_limit:"Credits 上限",credits_total:"Credits 總量",credits_has_credits:"Credits 可用",credits_unlimited:"Credits 無上限"};const base=key.startsWith("usage_")?key.slice(6):key;const common={plan_type:"方案",has_credits:"Credits 可用",unlimited:"無上限",calls:"操作次數",timestamp:"時間",time:"時間",operation:"操作",source:"來源",model:"Model",status:"結果",latency_ms:"耗時 (ms)",average_latency_ms:"平均耗時 (ms)",http_status:"HTTP status",request_bytes:"送出大小 (bytes)",response_bytes:"回傳大小 (bytes)",known_response_bytes:"已知回傳大小 (bytes)",request_body_bytes:"送出大小 (bytes)",since:"最早紀錄",attempts:"HTTP 嘗試",input_known_calls:"輸入 Token 有值",output_known_calls:"輸出 Token 有值",input_unknown_calls:"輸入 Token 缺值",output_unknown_calls:"輸出 Token 缺值",response_unknown_attempts:"回傳大小缺值",statuses:"結果分布"};return mcpFields[key]||ui(credits[base]||common[base]||key.replaceAll("_"," "));}
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
  const container=node("div",null,"thread-mcp-content"),groups=[];content.append(node("h4",ui("MCP 觀察")),container);
  for(const server of servers){const panel=node("section"),records=events.filter(event=>event.server===server);groups.push([server,server,panel]);
    const summary=node("div",null,"cards");cards(summary,[[ui("工具呼叫"),fmt(records.filter(event=>!event.nested).length),ui("來源實際記錄的呼叫")],[ui("exec 辨識"),fmt(records.filter(event=>event.nested).length),ui("程式碼中的呼叫位置")],[ui("已提供結果"),fmt(records.filter(event=>event.result?.status).length),ui("有結果狀態的紀錄")]]);panel.append(summary);
    const durations=records.filter(event=>Number.isFinite(event.duration_ms)).map(event=>event.duration_ms);if(durations.length)panel.append(statisticLabels(statistics(durations),"ms",ui("有獨立耗時的工具呼叫")));
    const metrics=mcpMetrics(records,"detail:thread:"+server+":metrics");if(metrics)panel.append(node("h4",ui("來源提供的指標")),metrics);
    if(server==="jev")appendJevCalls(panel,thread);
    if(records.length)panel.append(node("h4",ui("操作紀錄")),table([ui("時間"),ui("工具"),ui("操作類型"),ui("結果"),ui("耗時 (ms)"),ui("操作摘要")],records.slice(0,100).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,event.tool,"mono");cell(row,actionLabels[event.action]||event.action);cell(row).append(tag(labels.status[event.result?.status]||event.result?.status||"--"));valueCell(row,event.duration_ms);cell(row,mcpBrief(event));return clickableRow(row,()=>openDetail({kind:"mcp",event}));}),ui("操作紀錄"),"detail:thread:"+server+":operations"));
  }
  modalTabs(container,groups,"thread-mcp",detail.mcpSource||groups[0][0],source=>detail.mcpSource=source);
}


function telemetryFields(record){return Object.entries(record||{}).filter(([,value])=>value==null||["number","boolean","string"].includes(typeof value));}
function telemetryValue(value){return value==null?"--":typeof value==="boolean"?ui(value?"是":"否"):typeof value==="number"?fmt(value):/^\d{4}-\d\d-\d\dT/.test(value)?when(value):labels.status[value]||value;}
function telemetryPanel(server,report){
  const panel=node("section",null,"panel source-telemetry");panel.append(node("h3",server+" · "+ui("來源紀錄")),metadataList([[ui("狀態"),logHealthLabels[report.health]||report.health],[ui("開始紀錄"),when(report.enabled_at)]]));const metrics=telemetryFields(report.summary).map(([key,value])=>[metricLabel(key),telemetryValue(value)]);for(const [key,value]of Object.entries(report.summary||{}))if(value&&typeof value==="object"&&!Array.isArray(value))for(const [name,count]of telemetryFields(value))metrics.push([metricLabel(key)+" · "+metricLabel(name),telemetryValue(count)]);if(metrics.length)panel.append(node("h4",ui("用量摘要")),metadataList(metrics));
  const recent=report.recent||[];if(recent.length){const fields=[...new Set(recent.flatMap(record=>telemetryFields(record).map(([key])=>key)))];panel.append(node("h4",ui("操作紀錄")),table(fields.map(metricLabel),recent.map(record=>{const row=node("tr");for(const key of fields)cell(row,telemetryValue(record[key]));return clickableRow(row,()=>openDetail({kind:"source-record",server,record}));}),ui("來源紀錄"),"source:"+server+":recent"));}
  if(report.series?.length){const fields=[...new Set(report.series.flatMap(record=>telemetryFields(record).map(([key])=>key)))];panel.append(node("h4",ui("活動紀錄")),table(fields.map(metricLabel),report.series.map(record=>{const row=node("tr");for(const key of fields)cell(row,telemetryValue(record[key]));return row;}),ui("活動紀錄"),"source:"+server+":series"));}return panel;
}
function connectionBadge(source){const connection=source.connection||{},names={response:"最近有回應",pending:"等待回應",error:"連線錯誤",paused:"觀察停用",unconfirmed:"尚未確認"},badge=tag(ui(names[connection.state]||"尚未確認"),"connection-status connection-"+(connection.state||"unconfirmed"));bindHelp(badge,ui("依本機呼叫, 回傳與診斷紀錄判斷")+" · "+when(connection.observed_at));return badge;}
function mcpTags(source,events){
  if(Array.isArray(source.tags)&&source.tags.length)return source.tags.map(label=>tag(label,"category"));
  const tools=[...new Set(events.filter(event=>event.server===source.server).map(event=>event.tool))],words=(source.server.replace(/([a-z])([A-Z])/g,"$1 $2")+" "+tools.join(" ")+" "+(source.description||"")+" "+events.filter(event=>event.server===source.server).flatMap(event=>Object.keys(event.result||{})).join(" ")).toLowerCase(),rules=[[/(?:saved_tokens|saving|compress|reduction)/,"Token 節省"],[/(?:docs?|documentation|wiki|manual|context)/,"文件查詢"],[/(?:search|find)/,"搜尋"],[/(?:sqlite|sql|database|query)/,"資料庫"],[/(?:ocr|extract|document|pdf)/,"文件處理"],[/(?:browser|navigate|screenshot)/,"瀏覽器"],[/(?:test|check|validate|verify)/,"驗證"],[/(?:^|[\s_.:-])(?:create|write|update|edit)(?:[\s_.:-]|$)/,"寫入"],[/(?:^|[\s_.:-])(?:read|get|list)(?:[\s_.:-]|$)/,"讀取"],[/(?:rank|evaluate|summari)/,"分析"]],tags=[];
  if(source.category!=="other")tags.push(ui(data.mcp.categories[source.category])||source.category);
  for(const [pattern,label]of rules)if(pattern.test(words)&&!tags.includes(ui(label)))tags.push(ui(label));
  return (tags.length?tags:[ui("其他 MCP")]).slice(0,2).map(label=>{const badge=tag(label,"category");bindHelp(badge,ui("依來源用途, 工具名稱與回傳指標分類"));return badge;});
}
function renderMcp(){
  const source=data.mcp||{servers:[],events:[],categories:{}},m={...source,servers:source.servers.filter(s=>s.server!=="web"),events:source.events.filter(e=>e.server!=="web")};
  renderMcpTabs(m.servers);const servers=m.servers.filter(s=>s.enabled&&(mcpSource==="all"||s.server===mcpSource)),selectedEvents=m.events.filter(e=>mcpSource==="all"||e.server===mcpSource);
  $("mcp-scope").textContent=(mcpSource==="all"?fmt(m.servers.length)+ui(" 個來源 · "):"")+fmt(selectedEvents.length)+ui(" 筆操作紀錄");
  cards("mcp-cards",[[ui("MCP 呼叫"),fmt(servers.reduce((n,s)=>n+s.calls,0)),mcpSource==="all"?ui("來源 ")+fmt(servers.length)+ui(" 個"):""],[ui("exec 辨識"),fmt(servers.reduce((n,s)=>n+s.recognized,0)),ui("程式碼中的呼叫位置")],[ui("已提供結果"),fmt(servers.reduce((n,s)=>n+s.known_status,0)),ui("有結果狀態的紀錄")],[ui("錯誤"),fmt(servers.reduce((n,s)=>n+s.errors,0)),ui("包含失敗與逾時")]]);
  $("mcp-panel").hidden=!m.servers.length;$("mcp-chart-panel").hidden=!m.servers.length;
  selectOptions("filter-mcp-category",[["all",ui("全部分類")],...[...new Set(m.servers.map(s=>s.category).concat(m.events.map(e=>e.category)))].map(key=>[key,ui(m.categories[key])||key])]);
  selectOptions("filter-mcp-server",[["all",ui("全部來源")],...m.servers.map(s=>[s.server,s.server])]);
  $("mcp-source-cards").replaceChildren(...m.servers.map(s=>{const el=button("",()=>{selectMcpSource(s.server);},"source-card"),tags=node("div",null,"tag-line");el.dataset.sourceKey=s.server;tags.append(...mcpTags(s,m.events),connectionBadge(s),tag(s.enabled?s.origin==="configured"?ui("已設定"):ui("有紀錄"):ui("已關閉"),s.enabled?s.origin==="configured"?"configured":"observed":"disabled"));el.append(node("div",s.server,"source-name mono"),...s.description?[node("p",s.description,"source-description")]:[],tags,node("p",fmt(s.calls)+ui(" 次呼叫 · ")+fmt(s.recognized)+ui(" 處 exec 辨識")),node("p",ui("錯誤 ")+fmt(s.errors)+ui(" · 平均 ")+fmt(s.average_ms)+" ms · P99 "+fmt(s.p99_ms)+" ms","muted"));return el;}));
  const selectedSource=m.servers.find(source=>source.server===mcpSource);$("mcp-purpose-panel").hidden=mcpSource==="all";$("mcp-purpose").textContent=selectedSource?.description||"";$("mcp-purpose").hidden=!selectedSource?.description;$("mcp-purpose-panel").querySelector("h3").hidden=!selectedSource?.description;$("mcp-purpose-origin").replaceChildren(...selectedSource?[connectionBadge(selectedSource),document.createTextNode(" · "+ui("最後回應")+": "+when(selectedSource.connection?.last_response_at))]:[]);
  if(mcpSource!=="all")$("filter-mcp-server").value=mcpSource;
  $("source-record-range").hidden=!Object.keys(data.mcp?.telemetry||{}).some(server=>mcpSource==="all"||server===mcpSource);$("mcp-telemetry").replaceChildren(...Object.entries(data.mcp?.telemetry||{}).filter(([server])=>mcpSource==="all"||server===mcpSource).map(([server,report])=>telemetryPanel(server,report)));
  const metrics=mcpMetrics(selectedEvents,"mcp-metric-rows");$("mcp-metrics-panel").hidden=!metrics;$("mcp-metrics").replaceChildren(...metrics?[metrics]:[]);
  const filtered=m.events.filter(e=>(mcpSource==="all"||e.server===mcpSource)&&($("filter-mcp-category").value==="all"||e.category===$("filter-mcp-category").value)&&($("filter-mcp-server").value==="all"||e.server===$("filter-mcp-server").value)&&($("filter-mcp-result").value==="all"||$("filter-mcp-result").value==="error"&&["error","failed","timeout","cancelled","canceled","missing_dependencies"].includes(e.result?.status)||$("filter-mcp-result").value==="known"&&e.result?.status||$("filter-mcp-result").value==="unknown"&&!e.result?.status));
  const rows=sortRecords("mcp-rows",filtered,[e=>e.timestamp,e=>e.thread_name||e.thread_id,eventProject,e=>e.server,e=>data.mcp.categories[e.category]||e.category,e=>e.tool,e=>actionLabels[e.action],e=>e.nested?ui("exec 辨識"):ui("工具呼叫"),e=>e.result?.status,e=>e.duration_ms,mcpBrief],e=>e.timestamp),size=tableSize("mcp-rows"),pages=Math.max(1,Math.ceil(rows.length/size));mcpPage=Math.max(1,Math.min(mcpPage,pages));
  replaceRows("mcp-rows",...rows.slice((mcpPage-1)*size,mcpPage*size).map(e=>{const row=node("tr");cell(row,when(e.timestamp));eventThreadCell(row,e);projectCell(row,e);cell(row,e.server,"mono");cell(row).append(tag(ui(m.categories[e.category])||e.category));cell(row,e.tool,"operation-cell mono");cell(row).append(tag(actionLabels[e.action]||e.action));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));cell(row).append(tag(labels.status[e.result?.status]||e.result?.status||"--",e.result?.status||""));valueCell(row,e.duration_ms);cell(row,mcpBrief(e));return clickableRow(row,()=>openDetail({kind:"mcp",event:e}));}));
  $("mcp-empty").textContent=rows.length?"":ui("沒有符合條件的操作");$("mcp-page-summary").textContent=ui("第 ")+mcpPage+" / "+pages+ui(" 頁 · ")+fmt(rows.length)+ui(" 筆");$("mcp-prev").disabled=mcpPage===1;$("mcp-next").disabled=mcpPage===pages;syncPageInput($("mcp-page-number"),mcpPage,pages);
  attachTables();

}
function renderWeb(){
  const events=(data.mcp?.events||[]).filter(e=>e.server==="web"),refs=references(events),urls=new Set(refs.map(e=>e.url));
  $("web-scope").textContent=ui("查看網路工具操作與參考頁面");
  cards("web-cards",[[ui("網路工具呼叫"),fmt(events.filter(e=>!e.nested).length),ui("exec 辨識 ")+fmt(events.filter(e=>e.nested).length)+ui(" 處")],[ui("參考網址"),fmt(urls.size),ui("不同參考頁面")],[ui("涉及對話"),fmt(new Set(events.map(e=>e.thread_id)).size),ui("有網路工具紀錄的對話")],[ui("已提供結果"),fmt(events.filter(e=>e.result?.status).length),ui("錯誤 ")+fmt(events.filter(e=>["error","failed","timeout"].includes(e.result?.status)).length)+ui(" 次")]]);
  replaceRows("web-event-rows",...events.map(e=>{const row=node("tr"),links=references([e]),tool=node("span",null,"tool-identity");tool.append(node("span","web.run","tool-name mono"),tag("Web"));cell(row,when(e.timestamp));eventThreadCell(row,e);projectCell(row,e);cell(row).append(tool);cell(row).append(tag(labels.status[e.result?.status]||e.result?.status||"--"));valueCell(row,links.length);cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"mcp",event:e}));}));
  $("web-event-empty").textContent=events.length?"":ui("無");
}
function fileLocation(event){return event.workdir&&!/^(?:[A-Za-z]:[\\/]|[\\/])/.test(event.path)?event.workdir.replace(/[\\/]$/u,"")+(event.workdir.includes("\\")?"\\":"/")+event.path:event.path;}
function fileProjectKey(event){const thread=threadIndex.get(event.thread_id)||{};return thread.project_id?"project:"+thread.project_id:thread.project_name?"name:"+thread.project_name:thread.project_scope==="none"?"none":"unknown";}
function filteredFiles(){const query=$("file-search").value.trim().toLowerCase(),operation=$("filter-file-operation").value,method=$("filter-file-method").value,project=$("filter-file-project").value,tool=$("filter-file-tool").value;return (data.codex.file_activity?.events||[]).filter(e=>(!query||fileLocation(e).toLowerCase().includes(query))&&(operation==="all"||operation===e.operation)&&(method==="all"||e.nested===(method==="nested"))&&(project==="all"||fileProjectKey(e)===project)&&(tool==="all"||e.tool===tool));}
function fileRows(events,includeThread=true){return events.map(e=>{const row=node("tr");cell(row,when(e.timestamp));if(includeThread){eventThreadCell(row,e);projectCell(row,e);}cell(row).append(tag(fileLabels[e.operation]||e.operation));cell(row,fileLocation(e),"mono path-cell");const tool=cell(row,null,"operation-cell");tool.append(toolButton(e.tool||"apply_patch",()=>openDetail({kind:e.nested?"nested-tool":"tool",tool:e.tool||"apply_patch",ids:[e.thread_id]})));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));cell(row,when(e.completed_at));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"file",event:e}));});}
function renderFiles(){
  const records=data.codex.file_activity||{events:[],total:0},projects=new Map();for(const event of records.events){const key=fileProjectKey(event);if(!["none","unknown"].includes(key))projects.set(key,eventProject(event)||key);}
  selectOptions("filter-file-project",[["all",ui("全部專案")],["none",ui("無專案")],["unknown",ui("未知")],...[...projects].sort((a,b)=>a[1].localeCompare(b[1]))]);selectOptions("filter-file-tool",[["all",ui("全部工具")],...[...new Set(records.events.map(event=>event.tool).filter(Boolean))].sort().map(tool=>[tool,tool])]);
  for(const [id,value]of [["filter-file-project",pendingFileProject],["filter-file-tool",pendingFileTool]])if(value&&[...$(id).options].some(option=>option.value===value))$(id).value=value;pendingFileProject=pendingFileTool=null;
  const events=filteredFiles();$("files-scope").textContent=ui("從工具參數辨識檔案讀寫操作")+" · "+fmt(records.total)+ui(" 筆操作");
  cards("file-cards",[[ui("符合篩選的紀錄"),fmt(events.length),ui("全部 ")+fmt(records.total)+ui(" 筆")],[ui("讀取"),fmt(events.filter(e=>e.operation==="read").length),ui("有指定檔案位置")],[ui("寫入與變更"),fmt(events.filter(e=>e.operation!=="read").length),ui("新增 / 修改 / 刪除 / 移動")],[ui("不同檔案位置"),fmt(new Set(events.map(fileLocation)).size),ui("目前篩選結果")]]);
  replaceRows("file-rows",...fileRows(events));$("file-empty").textContent=events.length?"":ui("無");
}
function renderGit(){
  const git=data.codex.git||{events:[],operations:{},total:0},select=$("filter-git"),selected=select.value;
  select.replaceChildren(...[["all",ui("全部操作")],...sorted(git.operations).map(([key,count])=>[key,key+" ("+fmt(count)+")"])].map(([value,label])=>{const el=node("option",label);el.value=value;return el;}));
  if([...select.options].some(option=>option.value===selected))select.value=selected;
  if(pendingGit){if([...select.options].some(option=>option.value===pendingGit))select.value=pendingGit;pendingGit=null;}
  const events=git.events.filter(event=>select.value==="all"||event.operation===select.value);
  $("git-scope").textContent=data.settings?.observations.git===false?ui("Git 觀察已關閉"):ui("點操作名稱可查看所屬工具呼叫");
  cards("git-cards",[[ui("Git 操作"),fmt(git.total),ui("操作種類 ")+fmt(Object.keys(git.operations).length)],[ui("涉及對話"),fmt(new Set(git.events.map(e=>e.thread_id)).size),ui("目前操作紀錄")],[ui("工具已回傳"),fmt(git.events.filter(e=>e.completed_at).length),ui("依工具回傳時間")],[ui("工作目錄"),fmt(new Set(git.events.map(e=>e.repository).filter(Boolean)).size),ui("已辨識的目錄名稱")]]);
  replaceRows("git-rows",...events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.operation,()=>openDetail({kind:"operation",event}),"link mono"));cell(row,event.repository||ui("未知"));cell(row,when(event.completed_at));cell(row,fmt(event.duration_ms));return row;}));
  $("git-empty").textContent=events.length?"":ui("無");
}
function renderWorkflow(){
  const skills=data.codex.skills||{events:[],counts:{}},checks=data.codex.checks||[];
  replaceRows("skill-rows",...skills.events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(event.has_document===false?node("span",event.skill):button(event.skill,()=>openDetail({kind:"skill",skill:event.skill}),"link"));return row;}));
  $("skill-empty").textContent=skills.events.length?"":ui("無");
  cards("check-cards",[[ui("驗證操作"),fmt(checks.length),ui("目前載入紀錄")],[ui("操作種類"),fmt(new Set(checks.map(e=>e.operation)).size),ui("test / build / lint")],[ui("涉及對話"),fmt(new Set(checks.map(e=>e.thread_id)).size),ui("有驗證操作的對話")],[ui("工具已回傳"),fmt(checks.filter(e=>e.completed_at).length),ui("有回傳時間的紀錄")]]);
  replaceRows("check-rows",...checks.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.operation,()=>openDetail({kind:"operation",event}),"link mono"));cell(row,when(event.completed_at));cell(row,fmt(event.duration_ms));return row;}));
  $("check-empty").textContent=checks.length?"":ui("無");
}

let uptimeSample=null;
function renderUptime(){if(!uptimeSample||document.hidden)return;const elapsed=Math.max(0,Math.floor(uptimeSample.seconds+(performance.now()-uptimeSample.received)/1000)),days=Math.floor(elapsed/86400),clock=[Math.floor(elapsed/3600)%24,Math.floor(elapsed/60)%60,elapsed%60].map(value=>String(value).padStart(2,"0")).join(":");$("program-uptime").textContent=ui("已執行 ")+fmt(days)+ui(" 天")+" "+clock;}
setInterval(renderUptime,1000);

function render(next){
  data=next;threadIndex=new Map((data.codex.threads||[]).map(thread=>[thread.thread_id,thread]));const c=data.codex,j=data.jev,threads=c.threads||[];
  $("program-version").textContent=data.monitor?.version?"v"+data.monitor.version:"--";document.documentElement.dataset.revision=data.revision||"";$("program-started").textContent=ui("啟動 ")+when(data.started_at);$("program-started").dateTime=data.started_at||"";$("program-started").title=ui("此監測程式的啟動時間");$("program-uptime").title=ui("此監測程式持續執行的時間, 每秒更新");if(Number.isFinite(data.monitor?.uptime_seconds))uptimeSample={seconds:data.monitor.uptime_seconds,received:performance.now()};renderUptime();
  $("live").textContent=ui(data.monitor?.health==="error"?data.updated_at?"資料整理失敗, 顯示上次結果":"資料整理失敗, 等待重試":data.updated_at?"本機連線正常":"正在整理資料");
  $("overview-scope").textContent=ui("觀察開始 ")+when(c.started_at||data.started_at);
  const duration=threads.filter(t=>t.task_duration_ms!=null).reduce((n,t)=>n+t.task_duration_ms,0);
  cards("overview-cards",[[ui("對話"),fmt(threads.length),ui("執行中 ")+fmt(threads.filter(t=>t.status==="running").length)+ui(" 個")],[ui("工具呼叫"),fmt(c.observed_tool_calls||0),ui("工具種類 ")+fmt(Object.keys(c.tools||{}).length)],[ui("工作累計時間"),fmt(Math.round(duration/60000))+ui(" 分鐘"),ui("已取得時間的對話 ")+fmt(threads.filter(t=>t.task_duration_ms!=null).length)+ui(" 個")],[ui("觀察來源"),fmt(data.mcp?.servers?.length||0),ui("檔案修改 ")+fmt(threads.reduce((n,t)=>n+(t.file_changes?.length||0),0))+ui(" 筆")]]);
  for(const [id,available]of [["mcp",data.availability?.jev||data.mcp?.servers?.some(s=>s.server!=="web")],["web",data.mcp?.servers?.some(s=>s.server==="web")]]){$("tab-"+id).hidden=!available;if(!available&&$("tab-"+id).getAttribute("aria-selected")==="true")switchTab("overview");}

  renderMonitor();renderErrors();renderLogSummary();renderSQLite();renderSources();if(!$("view-errors").hidden&&!$("view-logs").hidden)loadLogs();
  updateProjects();renderCodex();renderUsage();renderDots();renderGit();renderWorkflow();renderTools();renderMcp();renderWeb();renderFiles();renderCharts();renderHighlights();
  $("updated").textContent=data.updated_at?ui("更新 ")+new Date(data.updated_at).toLocaleTimeString(locale,{hour12:false}):ui("尚未更新");$("updated").title=when(data.updated_at);$("updated").dateTime=data.updated_at||"";
  if(!settingsBusy&&data.settings){if(document.activeElement!==$("refresh-interval"))$("refresh-interval").value=data.settings.interval;schedule(data.settings.interval);}
  if(detail&&detail.kind!=="jev")renderDetail();attachTables();
}
const errorCategories=editableLabels({conversation:"對話",mcp:"MCP",tool:"工具",codex:"Codex",monitor:"觀察程式"});
const errorSources=editableLabels({codex_desktop:"Codex App",codex_core:"Codex Core",session:"對話紀錄",tool_result:"工具回傳",jev_telemetry:"Jev 紀錄",monitor:"觀察程式"});
const errorReasons=editableLabels({message_submit_failed:"對話送出失敗",stream_interrupted:"回應連線中斷 / 重試",mcp_diagnostic:"MCP 診斷事件",tool_diagnostic:"工具診斷事件",codex_diagnostic:"Codex 診斷事件",process_exit:"命令回傳非零代碼",file_not_found:"找不到檔案",http_error:"HTTP 請求失敗",network_unavailable:"網路無法連線",invalid_response:"回傳格式無法辨識",response_too_large:"回傳內容過大",tool_error:"工具回報錯誤",refresh_failed:"資料整理失敗",http_response_error:"HTTP 回應錯誤",error:"對話回報錯誤",turn_failed:"對話執行失敗",turn_aborted:"對話中止"});
function errorReason(e){return errorReasons[e.reason||e.code]||labels.status[e.code]||ui("來源回報錯誤");}
function errorRows(events){return events.map(e=>{const row=node("tr");cell(row,when(e.timestamp));cell(row).append(tag(ui(e.severity==="warning"?"警告":"錯誤")));cell(row,errorCategories[e.category]||e.category);if(e.thread_id)eventThreadCell(row,e);else cell(row,"--");projectCell(row,e);cell(row,errorSources[e.source]||e.source);if(e.tool)cell(row).append(toolButton(e.server?"mcp__"+e.server+"__"+e.tool:e.tool,()=>openDetail({kind:"tool",tool:e.server?"mcp__"+e.server+"__"+e.tool:e.tool})));else cell(row,"--");cell(row,errorReason(e));cell(row,e.code||"--","mono");return clickableRow(row,()=>openDetail({kind:"error",event:e}));});}
function renderErrors(){
  const errors=data.errors||{},events=errors.events||[],failed=events.filter(e=>e.severity==="error"),warnings=events.filter(e=>e.severity==="warning"),recent=failed.filter(e=>{const time=new Date(e.timestamp).getTime();return time>=Date.now()-86400000&&time<=Date.now()+60000;});
  $("errors-quick").textContent=ui("錯誤 ")+fmt(recent.length);$("errors-quick").classList.toggle("has-errors",!!recent.length);$("errors-quick").title=ui("最近 24 小時 · ")+fmt(recent.length)+ui(" 筆錯誤")+(failed[0]?ui(" · 最新 ")+when(failed[0].timestamp):"");
  const health=errors.diagnostics?.health||{},names={ok:ui("正常"),missing:ui("未找到"),unsupported:ui("格式未支援"),unavailable:ui("無法讀取"),partly_unavailable:ui("部分無法讀取"),disabled:ui("未啟用"),waiting:ui("等待中")};
  $("error-scope").textContent=(errors.enabled?ui("診斷紀錄: App ")+(names[health.desktop]||ui("未知"))+" · Core "+(names[health.core]||ui("未知")):ui("對話與工具錯誤觀察已停用"))+ui(" · 保留最近 ")+fmt(errors.limit||1000)+ui(" 筆事件")+ui(" · 保存最近 24 小時錯誤摘要")+(errors.history?.health==="unavailable"?ui(" · 摘要保存失敗, 目前顯示記憶體資料"):"");
  cards("error-cards",[[ui("最近 24 小時錯誤"),fmt(recent.length),ui("點右上角可快速查看")],[ui("目前載入的錯誤"),fmt(failed.length),ui("包含所有已載入的時間")],[ui("警告"),fmt(warnings.length),ui("包含連線重試與診斷警告")],[ui("涉及對話"),fmt(new Set(events.map(e=>e.thread_id).filter(Boolean)).size),ui("可點整列查看明細")]]);
  replaceRows("error-rows",...errorRows(events));$("error-empty").textContent=events.length?"":ui("無");
}
$("errors-quick").addEventListener("click",()=>{diagnosticSource="errors";switchTab("errors");const view=tableViews.get("error-rows");if(view){tableStates["error-rows"]={...tableStates["error-rows"],page:1,filters:{"等級":ui("錯誤")}};renderTableFilters("error-rows");paginateTable("error-rows");persistTables();}$("view-errors").scrollIntoView({block:"start"});});
const monitorEventLabels=editableLabels({started:"程式啟動",settings_applied:"設定已套用",recording_enabled:"Jev 紀錄已啟用",recording_disabled:"Jev 紀錄已停用",refresh_failed:"資料整理失敗",recovered:"資料整理恢復",http_response_error:"HTTP 回應錯誤"});
const logHealthLabels=editableLabels({ok:"正常",ready:"就緒",enabled:"已啟用",missing:"未找到",config_missing:"尚未設定",waiting:"等待讀取",disabled:"已停用",paused:"已暫停",memory:"記憶體保存",unsupported:"格式未支援",unavailable:"無法存取",partly_unavailable:"部分無法存取",invalid_config:"設定無效",error:"讀取失敗"});
const logLevelLabels={error:"error",warning:"warn",warn:"warn",info:"info",debug:"debug",trace:"trace",fatal:"fatal",critical:"critical"};
function logSource(source){return errorSources[source]||source;}
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
  $("sqlite-scope").textContent=data.settings.observations.sqlite===false?ui("SQL 操作觀察已停用"):ui("從工具呼叫與 SQL 診斷辨識操作類型")+" · "+fmt(events.length)+ui(" 筆操作");
  cards("sqlite-cards",[[ui("SQL 操作"),fmt(events.length),ui("最近保留的操作紀錄")],[ui("資料庫"),fmt(new Set(events.map(sqlLocation).filter(Boolean)).size),ui("可確認的檔案與記憶體資料庫")],[ui("SQL 診斷紀錄"),fmt(events.filter(e=>e.recognition==="diagnostic_log").length),ui("來源實際提供的 SQL metadata")],[ui("工具失敗"),fmt(new Set(events.filter(e=>e.result==="failed").map(e=>e.thread_id+":"+e.call_id)).size),ui("外層工具呼叫, 非單一 SQL 結果")]]);
  replaceRows("sqlite-rows",...events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,event.statement,"mono");cell(row,sqlLabels[event.operation]||event.operation);cell(row,event.engine);cell(row,sqlLocation(event)||"--","path-cell mono");cell(row).append(tag(sqlResults[event.result]||event.result));if(event.thread_id)eventThreadCell(row,event);else cell(row,"--");projectCell(row,event);cell(row,sqlMethod(event));cell(row,event.tool||"--","mono");valueCell(row,event.container_duration_ms);cell(row,event.call_id||"--","mono");cell(row,event.source?logSource(event.source):ui("對話紀錄"));valueCell(row,event.duration_ms);valueCell(row,event.rows_affected);valueCell(row,event.rows_returned);return clickableRow(row,()=>openDetail({kind:"sqlite",event}));}));
  $("sqlite-empty").textContent=events.length?"":ui("無");
  const measured=events.filter(event=>Number.isFinite(event.duration_ms));$("sqlite-measured").replaceChildren(node("h4",ui("SQL 耗時統計")),measured.length?statisticLabels(statistics(measured.map(event=>event.duration_ms)),"ms",ui("僅統計來源有提供耗時的 SQL 紀錄")):node("p","--","muted"));
}
function renderSources(){
  const scopes=editableLabels({recent_tail_and_incremental_metadata:"讀取近期 session 尾端與新增紀錄, 並補回最近 24 小時的錯誤摘要",titles_classification_and_local_thread_metadata:"讀取本機 catalog 的對話名稱, 專案與執行設定",recent_metadata_and_24h_error_backfill:"讀取近期診斷 metadata, 分批回補最近 24 小時的錯誤摘要",opt_in_telemetry:"讀取使用者啟用的 Jev telemetry 紀錄",runtime_samples_and_bounded_event_metadata:"效能樣本保存在記憶體. 程式事件與最近 24 小時錯誤摘要採循環保存",confirmed_lifecycle_and_skill_checkpoints:"保存最近確認的對話狀態與 Skills 讀取紀錄",interface_copy_and_tool_descriptions:"介面偏好保存在此瀏覽器, 工具用途由觀察設定管理"}),modes=editableLabels({read_only:"唯讀",bounded_metadata_cache:"循環保存",user_preferences:"使用者設定"});
  for(const tab of document.querySelectorAll('main>section[role="tabpanel"]')){
    const id=tab.id.slice(5),sources=Object.entries(data.sources||{}).filter(([,source])=>id==="overview"||source.features?.includes(id)||id==="mcp"&&source.features?.includes("jev")||id==="errors"&&source.features?.includes("logs")||id==="workflow"&&source.features?.some(feature=>["git","workflow","checks"].includes(feature)));let footer=tab.querySelector(":scope>.source-reference");
    if(!footer){footer=node("section",null,"panel source-reference");tab.append(footer);}const opened=new Set([...footer.querySelectorAll("details[open]")].map(el=>el.dataset.source));
    const heading=node("h3",ui("資料來源與讀取範圍"));heading.title=fieldDescription("資料來源與讀取範圍");
    footer.replaceChildren(heading,...sources.map(([key,source])=>{const el=node("details"),summary=node("summary",source.name+" · "+(modes[source.mode]||source.mode));el.dataset.source=key;el.open=opened.has(key);el.append(summary,node("p",scopes[source.scope]||source.scope,"muted"));for(const location of source.locations||[])el.append(node("p",location,"mono source-location"));if(!source.locations?.length)el.append(node("p",ui("尚無已設定的來源位置"),"muted"));if(source.health)el.append(node("p",ui("來源狀態: ")+(logHealthLabels[source.health]||source.health),"muted"));return el;}));
    if(!sources.length)footer.append(node("p",ui("此頁使用目前已載入的觀測資料"),"muted"));
  }
}
function renderLogSummary(){
  const logs=data.logs||{},sources=logData?.sources||logs.sources||[],problems=sources.filter(source=>["missing","config_missing","unsupported","unavailable","partly_unavailable","invalid_config","error"].includes(source.health));
  cards("log-cards",[[ui("保留 Log 事件"),fmt(logData?.entries?.length??logs.retained),ui("點整列查看紀錄明細")],[ui("觀察來源"),fmt(sources.length),ui("來源診斷與歷史回補")],[ui("需要檢查的來源"),fmt(problems.length),ui("點來源列查看原因")],[ui("程式 Log 容量"),measure(sources.find(source=>source.source==="monitor")?.bytes,"bytes"),ui("最多 128 KiB, 自動輪替")]]);
  $("log-scope").textContent=logs.enabled===false?ui("Log 觀察已停用"):ui("來源 Log 與最近 24 小時錯誤回補");
  if(logs.enabled===false){logData=null;replaceRows("log-rows");$("log-message").textContent=ui("Log 觀察已停用");renderLogSources(logs.sources||[]);}else renderLogSources(sources);
}
async function loadLogs(){
  if(logBusy){logQueued=true;return;}logBusy=true;
  if(!logData)$("log-message").textContent=ui("正在讀取 Log 紀錄");
  try{
    const response=await request("/api/logs",{cache:"no-store"});if(!response.ok)throw new Error();logData=response.data;
    if(!data||$("view-logs").hidden)return;
    renderLogSummary();
    replaceRows("log-rows",...(logData?.entries||[]).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row).append(tag(logLevelLabels[event.severity]||event.severity,event.severity));cell(row,logSource(event.source));cell(row,event.module||"--","mono");cell(row,logMessage(event));cell(row,event.code||"--","mono");if(event.thread_id)eventThreadCell(row,event);else cell(row,"--");projectCell(row,event);cell(row,event.method||event.tool||"--","mono");cell(row,event.error_type||"--","mono");cell(row,event.file||"--","mono");valueCell(row,event.record_id);return clickableRow(row,()=>openDetail({kind:"log",event}));}));
    $("log-message").textContent=logData?.entries?.length?ui("保留最近 ")+fmt(logData.entries.length)+ui(" 筆. 紀錄明細依來源可取得的欄位顯示"):ui(logData?.enabled===false?"Log 觀察已停用":"無");attachTables();
  }catch{$("log-message").textContent=ui(logData?"Log 讀取失敗, 顯示上次取得的紀錄. 將自動重試":"Log 讀取失敗, 將自動重試");}finally{logBusy=false;if(logQueued){logQueued=false;if(!$("view-logs").hidden)loadLogs();}}
}
function renderMonitor(){
  const m=data.monitor||{},c=data.codex,enabled=Object.values(data.settings?.observations||{}).filter(Boolean).length;
  const activeHelp=document.activeElement.closest("#monitor-storage .metadata-help"),helpIndex=activeHelp?[...activeHelp.parentElement.querySelectorAll(".metadata-help")].indexOf(activeHelp):-1,helpDismissed=activeHelp?.classList.contains("help-dismissed");
  cards("monitor-cards",[[ui("整理狀態"),ui(m.health==="error"?"整理失敗":m.health==="ok"?"正常":"啟動中"),ui("累計錯誤 ")+fmt(m.errors||0)+ui(" 次")],[ui("執行時間"),fmt(Math.floor((m.uptime_seconds||0)/60))+ui(" 分鐘"),ui("已整理 ")+fmt(m.refreshes||0)+ui(" 次")],[ui("本輪整理耗時"),fmt(m.refresh_ms)+" ms",ui("CPU 時間 ")+fmt(m.cpu_ms)+" ms"],[ui("保留工具紀錄"),fmt(m.retained_calls||0),ui("上限 ")+fmt(m.call_limit)+ui(" 筆")]]);
  $("monitor-runtime").replaceChildren(metadataList([[ui("程式版本"),m.version||ui("未知")],[ui("Python 版本"),m.python||ui("未知")],[ui("系統"),m.platform||ui("未知")],[ui("處理器架構"),m.architecture||ui("未知")],["PID",m.pid||ui("未知")],[ui("啟用的觀察項目"),fmt(enabled)],[ui("HTTP 請求"),fmt(m.requests||0)],[ui("HTTP 錯誤回應"),fmt(m.http_errors||0)],[ui("上次整理錯誤"),m.last_error_at?when(m.last_error_at):ui("無")],[ui("目前錯誤類型"),m.error_type||ui("無")]]));
  $("monitor-storage").replaceChildren(metadataList([
    [ui("追蹤 session 檔案"),fmt(c.files||0)+" / "+fmt(m.file_limit),ui("目前追蹤的 session 檔案數 / 程式上限. 一個對話可能有多個檔案, 所以不等於對話數")],
    [ui("保留效能樣本"),fmt(m.history?.length||0)+" / "+fmt(m.history_limit),ui("每次資料整理完成會保存一筆耗時, CPU 與讀取量, 供效能圖表使用. 顯示目前筆數 / 上限, 超過上限移除最舊樣本, 重啟後清空")],
    [ui("保留狀態事件"),fmt(m.events?.length||0)+" / "+fmt(m.event_limit),ui("程式啟動, 設定變更, 整理錯誤與恢復等事件的目前筆數 / 上限. 保存在記憶體, 超過上限移除最舊事件, 重啟後清空")],
    [ui("待完成的紀錄片段 (bytes)"),fmt(m.buffer_bytes),ui("session 最後一行尚未寫完時, 暫存等待下次讀取的內容大小. 這是待解析內容的位元組數, 不是尚未完成的工作次數")],
    [ui("片段保留上限 (bytes)"),fmt(m.buffer_limit),ui("所有 session 未完成行的暫存總上限. 超過時移除片段, 避免記憶體持續增加")],
    [ui("累計移除的舊工具紀錄"),fmt(m.trimmed_calls||0),ui("工具紀錄超過每個 session 或整體保留上限時, 移除舊資料的累計筆數. 重新建立 Codex 觀察後重新累積")],
    [ui("累計 session 讀取量 (bytes)"),fmt(c.bytes_read),ui("目前 Codex 觀察累計讀取的 session 位元組數, 包含初次尾端, 增量與狀態回查. 重新建立 Codex 觀察後重新累積")],
    [ui("損壞紀錄 (行)"),fmt(c.malformed_lines),ui("讀到無法解析為 JSON 的完整行數. 未寫完的最後一行會先暫存等待, 不直接算成損壞")],
    [ui("上次資料大小 (bytes)"),fmt(m.snapshot_bytes),ui("最近一次資料 API 回應的 JSON 大小, 尚未 gzip 壓縮")],
    [ui("上次傳輸大小 (bytes)"),fmt(m.transfer_bytes),ui("最近一次資料 API 回應實際送出的內容大小. 使用 gzip 時為壓縮後大小, 不包含 HTTP header")],
    [ui("Jev 資料庫大小 (bytes)"),fmt(m.jev_database_bytes),ui("Jev 的 SQLite 主資料庫檔案大小, 不含 WAL / SHM 暫存檔")]
  ]));
  if(helpIndex>=0){const term=$("monitor-storage").querySelectorAll(".metadata-help")[helpIndex];if(term){term.classList.toggle("help-dismissed",!!helpDismissed);term.focus({preventScroll:true});}}
  $("monitor-retention").textContent=ui("效能樣本與狀態事件保存在記憶體, 超過上限會移除舊資料, 重新啟動後重新累積");
  const names=monitorEventLabels;
  replaceRows("monitor-event-rows",...(m.events||[]).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,names[event.kind]||event.kind);cell(row,event.error_type||"--","mono");return row;}));
}
function renderHighlights(){
  const c=data.codex,m=data.mcp||{events:[],categories:{}};
  const items=[[ui("工具"),"tools",fmt(c.observed_tool_calls||0)+ui(" 次呼叫"),fmt(Object.keys(c.tools||{}).length)+ui(" 種工具")],[ui("Git 操作"),"git",fmt(c.git?.total||0)+ui(" 筆"),Object.entries(c.git?.operations||{}).sort((a,b)=>b[1]-a[1]).slice(0,3).map(([key,n])=>key+" "+n).join(" · ")],[ui("Skills"),"skills",fmt(Object.keys(c.skills?.counts||{}).length)+ui(" 個 Skills"),fmt(c.skills?.events?.length||0)+ui(" 筆讀取紀錄")],[ui("驗證"),"checks",fmt(c.checks?.length||0)+ui(" 筆"),ui("test / build / lint")],[ui("檔案"),"files",fmt(c.file_activity?.total||0)+ui(" 筆"),ui("讀取 ")+fmt(c.file_activity?.operations?.read||0)+ui(" 次")],[ui("網路"),"web",fmt(m.events.filter(e=>e.server==="web").length)+ui(" 筆"),ui("參考網址 ")+fmt(new Set(references(m.events).map(e=>e.url)).size)+ui(" 個")]];
  items.push([ui("錯誤紀錄"),"errors",fmt(data.errors?.severities?.error||0)+ui(" 筆錯誤"),fmt(data.errors?.severities?.warning||0)+ui(" 筆警告")]);
  items.push([ui("Log 紀錄"),"logs",fmt(data.logs?.retained||0)+ui(" 筆事件"),ui("來源健康狀態與執行紀錄")]);
  items.push([ui("SQL / SQLite"),"sqlite",fmt(c.sqlite?.total||0)+ui(" 筆操作"),ui("工具紀錄與 SQL 診斷")]);

  for(const source of data.mcp?.servers||[]){if(!source.enabled||source.server==="web")continue;const latest=m.events.filter(event=>event.server===source.server).sort((a,b)=>(b.completed_at||b.timestamp||"").localeCompare(a.completed_at||a.timestamp||""))[0],report=data.mcp.telemetry?.[source.server],info=report?telemetryFields(report.summary).slice(0,3).map(([key,value])=>metricLabel(key)+": "+telemetryValue(value)).join(" · "):latest?mcpBrief(latest):ui("已設定");items.push([source.server,"mcp",fmt(source.calls)+ui(" 次呼叫 · ")+fmt(source.recognized)+ui(" 處 exec 辨識"),info,source.server]);}
  $("overview-highlights").replaceChildren(...items.map(([name,tab,value,info,category])=>{const el=button(null,()=>{if(category&&tab==="mcp"){openDetail({kind:"mcp-source",server:category});return;}if(tab==="jev"){openDetail({kind:"mcp-source",server:"jev"});return;}switchTab(tab);},"highlight-card tab-order-row");el.dataset.tabOrder=category?"mcp:"+category:tab;el.append(node("h3",name),node("div",value,"highlight-value"),node("p",info,"muted"));return el;}));arrangeHighlights();
}
function metadataList(items){
  const list=node("dl",null,"metadata-grid");
  for(const [label,value,description]of items){
    const help=description||fieldDescription(label);
    const term=node("dt",label);
    if(help){const mark=node("span","ⓘ","help-mark");mark.setAttribute("aria-hidden","true");term.classList.add("metadata-help");term.tabIndex=0;term.setAttribute("aria-label",label);term.replaceChildren(node("span",label,"help-label"),mark);bindHelp(term,help);for(const event of ["mouseleave","blur"])term.addEventListener(event,()=>term.classList.remove("help-dismissed"));}
    const entry=node("dd");if(value instanceof Node)entry.append(value);else entry.textContent=value==null||value===""?"--":value;list.append(term,entry);
  }
  return list;
}
function table(headers,rows,title,key){const wrap=node("div",null,"table-wrap"),el=node("table"),head=node("thead"),tr=node("tr"),body=node("tbody");el.dataset.tableKey=key||"detail:"+detail.kind+":"+headers.join("|");if(key)body.id=key;if(title)el.dataset.tableTitle=title;for(const label of headers)tr.append(node("th",label));head.append(tr);body.append(...rows);el.append(head,body);wrap.append(el);return wrap;}

function toolEventRows(events){return events.slice(0,100).map(event=>{const row=node("tr");cell(row,when(event.timestamp));const outer=cell(row,null,"tool-column");outer.append(toolButton(event.tool,()=>openDetail({kind:"tool",tool:event.tool})));const inner=cell(row,null,"nested-tool-column"),list=node("div",null,"tools");for(const [tool,count]of sorted(event.nested_tools))list.append(toolButton(tool,()=>openDetail({kind:"nested-tool",tool}),count));inner.append(list.childElementCount?list:node("span","--"));cell(row,when(event.completed_at));valueCell(row,event.duration_ms);return row;});}
const toolPurposes={exec:"執行 JavaScript, 可在一次操作中呼叫其他工具並整理回傳結果",exec_command:"執行終端機命令, 例如讀取檔案, Git 操作或測試",write_stdin:"向已啟動的命令送入資料, 或取得後續執行結果",apply_patch:"依 patch 新增, 修改或刪除檔案",web__run:"搜尋網路, 開啟參考頁面, 或取得天氣與其他網路資料",view_image:"檢視本機圖片",js:"在持續存在的 JavaScript runtime 中執行操作",js_reset:"重設 JavaScript runtime 與其中的變數",document_status:"檢查文件處理套件與 OCR 是否就緒",extract_document:"取得文件文字與位置資訊, 必要時執行 OCR, 可預覽或寫出 Markdown",inspect_markdown:"讀取 Markdown 的標題, 指定段落與 SHA-256",locate_markdown_extracts:"尋找與原始文件相符的 Markdown 抽出檔",update_markdown:"檢查目前 hash 後, 預覽或寫入 Markdown 更新",workspace_status:"查看環境檢查工具的版本與可讀取範圍",compare_environment:"比對兩個目錄的檔案與 SHA-256, 找出環境差異",validation_evidence:"整理既有驗證紀錄與結果",jev_rank:"依問題將候選內容排序, 決定優先閱讀順序",jev_evaluate:"依明確的評分項目評估指定內容",jev_status:"檢查 Jev 設定, 可選擇測試 API 連線",pull_requests_checks:"讀取 PR / MR 的 CI 檢查結果",create_worktree:"建立供目前工作使用的 Git worktree",archive_worktree:"保存 worktree 的工作快照並封存",restore_worktree:"還原已封存的 worktree",fork_thread:"從既有對話建立保留脈絡的分支",handoff_thread:"移轉工作與 Git 狀態",automation_update:"建立或調整排程工作",read_thread:"讀取指定工作的狀態與摘要",list_threads:"列出目前工作與對話",open_in_codex:"在 Codex 面板開啟檔案, 頁面或其他成果"};
function defaultPurpose(tool){const match=tool.match(/^mcp__(.+)__(.+)$/);if(match)return data?.mcp?.servers?.find(source=>source.server===match[1])?.description||"--";const suffix=tool.split("__").at(-1).split(".").at(-1);return ui(toolPurposes[tool]||toolPurposes[suffix]||"--");}
function purpose(tool){return data?.settings?.tool_descriptions?.[tool]||defaultPurpose(tool);}
function purposeBlock(content,tool){const section=node("div",null,"purpose-block");section.append(node("p",purpose(tool),"tool-purpose"),button(ui("編輯工具說明"),()=>openToolDocs(tool),"link"));content.append(section);}
function appendJevCalls(content,t){if(!data.availability?.jev&&!t.jev_calls?.length)return;content.append(node("h4",ui("Jev 呼叫")));if(!t.jev_calls?.length){content.append(node("p","--","empty"));return;}for(const call of t.jev_calls){const row=node("div",null,"detail-row");row.append(node("span",when(call.timestamp)),button(call.operation+ui(" · 送出 / 回傳"),()=>openJev(call,t),"link"));content.append(row);}}
const detailHistory=[];
function rememberDetail(){if(detail&&$("detail-dialog").open){detailHistory.push({view:detail,scroll:$("detail-content").scrollTop});if(detailHistory.length>20)detailHistory.shift();}}
function clearDetailTabs(){const nav=$("detail-content").previousElementSibling;if(nav?.classList.contains("modal-tabs"))nav.remove();}
function updateDetailBack(){$("detail-back").disabled=!detailHistory.length;}
function openDetail(value,remember=true){if(remember)rememberDetail();detail=value;renderDetail();$("detail-content").scrollTop=0;updateDetailBack();if(!$("detail-dialog").open)openDialog($("detail-dialog"));}
function renderDetail(){
  clearDetailTabs();
  if(!detail||!data)return;
  const content=$("detail-content");content.replaceChildren();
  if(detail.kind==="thread"||detail.kind==="thread-tools"){
    const t=data.codex.threads.find(item=>item.thread_id===detail.id)||detail.thread;detail.thread=t;
    $("detail-title").textContent=(detail.kind==="thread-tools"?ui("工具明細 · "):"")+title(t);
    if(detail.kind==="thread"){
      content.append(metadataList([["Thread ID",t.thread_id],[ui("對話建立時間"),when(t.created_at)],[ui("最近活動時間"),when(t.updated_at)],[ui("Token 更新時間"),when(t.token_updated_at)],[ui("類型"),labels.type[t.activity_type]||ui("未知")],[ui("執行位置"),labels.environment[t.environment]||ui("未知")],[ui("專案"),t.project_name||t.project_id||(t.project_scope==="none"?ui("無專案"):ui("未知"))],[ui("活動來源"),labels.trigger[t.trigger]||ui("未知")],[ui("排程綁定"),t.has_schedule==null?ui("未知"):ui(t.has_schedule?"有":"無")],[ui("狀態"),statusBadge(t)],[ui("Model"),t.model||"--"],[ui("Reasoning 等級"),t.reasoning_effort||"--"],[ui("Model 設定更新時間"),when(t.context_updated_at)]]));
      content.append(metadataList([[ui("狀態確認時間"),when(t.status_updated_at)],[ui("資料來源"),t.event_only?ui("操作或診斷紀錄中的對話 ID"):t.metadata_only?ui("本機對話目錄"):ui("Codex session 與本機目錄")],[ui("狀態來源"),t.status_source==="session_lifecycle"?ui("工作開始 / 完成事件"):t.status_source==="catalog_status"?ui("本機目錄狀態"):t.status_source==="cached_lifecycle"?ui("最近確認的工作事件"):t.status_backfill_pending?ui("正在回查較早的工作事件"):ui("來源未提供狀態")]]));
      if(t.metadata_only)content.append(node("p",ui(t.event_only?"目前只取得紀錄中的對話 ID, 尚未載入對應的對話資訊":t.activity_type==="chat"?"這筆雲端對話的資料來自本機目錄. 可取得的欄位依本機目錄資料":"目前只取得對話目錄資料, 尚未載入對應的 session"),"muted"));
      const executionLabels={model_provider:"Provider",cli_version:ui("Codex 版本"),originator:ui("執行程式"),history_mode:ui("歷史紀錄模式"),approval_policy:ui("操作審核模式"),sandbox_mode:ui("Sandbox 模式"),collaboration_mode:ui("協作模式"),context_window:ui("Context window (tokens)"),git_branch:ui("紀錄時的 Git 分支"),git_commit:ui("紀錄時的 Git commit"),parent_thread_id:ui("上層 Thread ID"),agent_nickname:ui("Agent 名稱"),agent_role:ui("Agent 角色"),service_tier:ui("服務等級"),reasoning_summary:ui("Reasoning 摘要模式"),approvals_reviewer:ui("操作審核來源")},execution=Object.entries(t.execution||{}).map(([key,value])=>[executionLabels[key]||key,typeof value==="number"?fmt(value):value]);
      if(execution.length)content.append(node("h4",ui("執行設定")),metadataList(execution));
      content.append(metadataList([[ui("工作總耗時 (秒)"),fmt(t.task_duration_ms==null?null:Math.round(t.task_duration_ms/100)/10)],[ui("已取得起訖的工作次數"),fmt(t.task_runs)]]));
      content.append(node("h4",ui("Token 用量 (tokens)")),metadataList(Object.entries({...tokenLabels,cache_write_input_tokens:ui("Cache write input")}).map(([key,label])=>[label,fmt(t.tokens?.[key])])),node("h4",ui("工具使用 (次)")));
    }else content.append(threadLink(t,ui("查看對話細節")));
    const counts=node("div",null,"tools");
    for(const [tool,count]of sorted(t.tools)){const el=toolButton(tool,()=>openDetail({kind:"tool",tool,ids:[t.thread_id]}),count);counts.append(el);}
    content.append(counts,node("p",ui("工具呼叫總數 ")+fmt(t.tool_calls)+ui(" 次"),"muted"));
    if(Object.keys(t.nested_tools||{}).length){const nested=node("div",null,"tools");for(const [tool,count]of sorted(t.nested_tools)){const el=toolButton(tool,()=>openDetail({kind:"nested-tool",tool,ids:[t.thread_id]}),count);nested.append(el);}content.append(node("h4",ui("exec 內辨識到的工具")),nested);}
    if(t.tool_events?.length)content.append(node("h4",ui("工具呼叫紀錄")),table([ui("呼叫時間"),ui("工具"),ui("內層工具"),ui("回傳時間"),ui("耗時 (ms)")],toolEventRows(t.tool_events)));
    content.append(node("h4",ui("檔案讀寫紀錄")));const files=[...(t.file_reads||[]),...(t.file_changes||[])];
    if(files.length)content.append(table([ui("時間"),ui("操作"),ui("檔案位置"),ui("工具"),ui("紀錄方式"),ui("工具回傳時間"),ui("耗時 (ms)")],fileRows(files,false)));else content.append(node("p",ui("無"),"empty"));
    const threadErrors=(data.errors?.events||[]).filter(e=>e.thread_id===t.thread_id);if(threadErrors.length)content.append(node("h4",ui("錯誤與警告")),table([ui("時間"),ui("等級"),ui("類型"),ui("對話"),ui("專案"),ui("來源"),ui("工具"),ui("錯誤說明"),ui("代碼")],errorRows(threadErrors)));
    appendThreadMcp(content,t);
  }else if(detail.kind==="tool"||detail.kind==="nested-tool"){
    const nested=detail.kind==="nested-tool",key=nested?"nested_tools":"tools",threads=(data.codex.threads||[]).filter(t=>(!detail.ids||detail.ids.includes(t.thread_id))&&t[key]?.[detail.tool]);
    $("detail-title").textContent=(nested?ui("exec 內層工具 · "):ui("工具明細 · "))+detail.tool;
    purposeBlock(content,detail.tool);
    content.append(node("p",ui("共 ")+fmt(threads.reduce((sum,t)=>sum+t[key][detail.tool],0))+(nested?ui(" 處辨識紀錄"):ui(" 次"))+" · "+fmt(threads.length)+ui(" 個對話"),"detail-total"));
    content.append(table([ui("對話"),nested?ui("辨識數量"):ui("使用次數")],threads.map(t=>{const row=node("tr");cell(row).append(threadLink(t));cell(row,fmt(t[key][detail.tool]));return row;})));
    const events=threads.flatMap(t=>(t.tool_events||[]).filter(e=>nested?e.nested_tools?.[detail.tool]:e.tool===detail.tool)).sort((a,b)=>(b.timestamp||"").localeCompare(a.timestamp||""));
    if(events.length)content.append(node("h4",ui("呼叫時間")),table([ui("呼叫時間"),ui("工具"),ui("內層工具"),ui("回傳時間"),ui("耗時 (ms)")],toolEventRows(events)));
  }else if(detail.kind==="operation"){
    const event=detail.event;$("detail-title").textContent=event.operation;
    content.append(metadataList([[ui("呼叫時間"),when(event.timestamp)],[ui("工具回傳時間"),when(event.completed_at)],[ui("工具耗時 (ms)"),fmt(event.duration_ms)],[ui("工作目錄名稱"),event.repository||ui("未知")],[ui("工具 Call ID"),event.call_id]]));
    const t=data.codex.threads.find(item=>item.thread_id===event.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
  }else if(detail.kind==="file"){
    const e=detail.event;$("detail-title").textContent=ui("檔案操作 · ")+(fileLabels[e.operation]||e.operation);
    content.append(metadataList([[ui("檔案位置"),fileLocation(e)],[ui("紀錄中的路徑"),e.path],[ui("工作目錄"),e.workdir||"--"],[ui("紀錄方式"),e.nested?ui("exec 程式碼中的呼叫位置"):ui("工具呼叫")],[ui("呼叫時間"),when(e.timestamp)],[ui("工具回傳時間"),when(e.completed_at)],[ui("耗時 (ms)"),fmt(e.duration_ms)],[ui("exec 整次耗時 (ms)"),fmt(e.container_duration_ms)],["Call ID",e.call_id]]));
    content.append(node("h4",ui("工具用途")));purposeBlock(content,e.tool);const t=data.codex.threads.find(t=>t.thread_id===e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
  }else if(detail.kind==="skill"){
    $("detail-title").textContent=ui("Skill 讀取 · ")+detail.skill;
    const events=(data.codex.skills?.events||[]).filter(event=>event.skill===detail.skill);
    content.append(node("p",fmt(data.codex.skills?.counts?.[detail.skill]||0)+ui(" 次讀取"),"detail-total"));
    content.append(node("h4",ui("Skill 檔案")));
    if(!detail.documents){content.append(node("p",detail.documentError||ui("讀取文件中"),"empty"));if(!detail.documentError&&!detail.documentLoading)loadSkillDocuments(detail);}
    else if(!detail.files?.length)content.append(node("p",ui("找不到這個 Skill 的本機文件"),"empty"));
    else{
      if(detail.root)content.append(metadataList([[ui("Skill 位置"),detail.root]]));
      const kinds={document:ui("文件"),python:ui("Python 腳本"),file:ui("其他檔案")};
      content.append(table([ui("更新時間"),ui("檔案"),ui("類型"),ui("大小 (bytes)")],detail.files.map(file=>{const row=node("tr");cell(row,when(file.modified_at));cell(row,null,"path-cell").append(button(file.relative_path,()=>{rememberDetail();detail={...detail,fileInfo:file};updateDetailBack();if(file.readable)loadSkillDocuments(detail,file.relative_path);else{detail.documentRequest={};detail.documentLoading=false;detail.documentPath=null;detail.documentError=null;renderDetail();}},"link mono"));cell(row).append(tag(kinds[file.kind]||file.kind),file.format?tag(file.format.toUpperCase()):node("span"));valueCell(row,file.bytes);return row;}),ui("Skill 檔案")));
      if(detail.filesTruncated)content.append(node("p",ui("檔案較多, 清單顯示目前掃描到的項目"),"muted"));
      const selected=detail.documents.find(file=>file.relative_path===detail.documentPath),fileInfo=detail.fileInfo||selected;
      if(fileInfo)content.append(node("h4",fileInfo.relative_path),metadataList([[ui("檔案位置"),fileInfo.path],[ui("類型"),kinds[fileInfo.kind]||fileInfo.kind],[ui("更新時間"),when(fileInfo.modified_at)],[ui("大小 (bytes)"),fmt(fileInfo.bytes)]]));
      if(detail.documentLoading)content.append(node("p",ui("讀取文件中"),"empty"));
      else if(detail.documentError)content.append(node("p",detail.documentError,"empty"));
      else if(selected){content.append(metadataList([[ui("讀取大小 (bytes)"),fmt(selected.read_bytes)],["SHA-256",selected.sha256]]),node("pre",selected.text,"payload skill-document"));if(selected.truncated)content.append(node("p",ui("文件較長, 顯示前段內容"),"muted"));}
    }
    content.append(node("h4",ui("近期讀取紀錄")),table([ui("時間"),ui("對話")],events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);return row;})));
  }else if(detail.kind==="error"){
    const e=detail.event;$("detail-title").textContent=errorReason(e);
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("等級"),ui(e.severity==="warning"?"警告":"錯誤")],[ui("類型"),errorCategories[e.category]||e.category],[ui("來源"),errorSources[e.source]||e.source],[ui("代碼"),e.code||"--"],[ui("錯誤類型"),e.error_type||"--"],[ui("來源模組"),e.module||"--"],[ui("操作"),e.method||"--"],["HTTP status",fmt(e.http_status)],[ui("命令回傳代碼"),fmt(e.exit_code)],["Thread ID",e.thread_id||"--"],["Call ID",e.call_id||"--"]]));
    const causeNames=editableLabels({rate_limit:"請求超過速率限制",quota_exceeded:"額度不足",authentication_failed:"認證失敗",permission_denied:"權限不足",connection_refused:"連線遭拒",timeout:"請求逾時",stream_interrupted:"回應串流中斷",invalid_response:"回應格式無效"});
    content.append(node("h4",ui("錯誤內容與追蹤")),metadataList([[ui("失敗原因"),causeNames[e.cause]||errorReason(e)],[ui("來源檔案"),e.file||"--"],[ui("紀錄 ID"),fmt(e.record_id)],["Request ID",e.request_id||"--"],["Trace ID",e.trace_id||"--"],[ui("重試次數"),fmt(e.attempt)],[ui("紀錄保留方式"),ui("最近 24 小時摘要")]]));
    const trace=Object.fromEntries(Object.entries(e).filter(([key])=>!["thread_name","project_name"].includes(key))),copy=button(ui("複製追蹤資料"),async()=>{try{await navigator.clipboard.writeText(JSON.stringify(trace,null,2));feedback("action-message",ui("追蹤資料已複製"));}catch{feedback("action-message",ui("剪貼簿無法寫入"),"error");}});content.append(copy);
    const logSource=(data.logs?.sources||[]).find(source=>source.source===e.source);if(logSource)content.append(button(ui("查看來源狀態"),()=>openDetail({kind:"log-source",source:logSource})));
    if(e.tool){const full=e.server?"mcp__"+e.server+"__"+e.tool:e.tool;content.append(node("h4",ui("工具用途")));purposeBlock(content,full);}
    const t=threadIndex.get(e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
    const mcp=(data.mcp?.events||[]).find(item=>item.thread_id===e.thread_id&&item.call_id===e.call_id&&item.server===e.server);if(mcp)content.append(button(ui("查看 MCP 操作"),()=>openDetail({kind:"mcp",event:mcp})));
  }else if(detail.kind==="mcp-source"){
    const source=(data.mcp?.servers||[]).find(item=>item.server===detail.server),events=(data.mcp?.events||[]).filter(event=>event.server===detail.server);$("detail-title").textContent=detail.server;
    if(source){if(source.description)content.append(node("p",source.description));const labels=node("div",null,"tag-line");labels.append(...mcpTags(source,events),connectionBadge(source));content.append(labels,metadataList([[ui("工具呼叫"),fmt(source.calls)],[ui("exec 辨識"),fmt(source.recognized)],[ui("已提供結果"),fmt(source.known_status)],[ui("錯誤"),fmt(source.errors)],[ui("最近活動"),when(source.last_at)],[ui("最後回應"),when(source.connection?.last_response_at)],[ui("等待回應"),fmt(source.connection?.pending_calls)]]));}
    if(data.mcp?.telemetry?.[detail.server])content.append(telemetryPanel(detail.server,data.mcp.telemetry[detail.server]));
    const metrics=mcpMetrics(events,"detail:source:"+detail.server);if(metrics)content.append(metrics);if(events.length)content.append(node("h4",ui("操作紀錄")),table([ui("時間"),ui("工具"),ui("結果"),ui("耗時 (ms)"),ui("操作摘要")],events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,event.tool,"mono");cell(row,labels.status[event.result?.status]||event.result?.status||"--");valueCell(row,event.duration_ms);cell(row,mcpBrief(event));return clickableRow(row,()=>openDetail({kind:"mcp",event}));}),ui("操作紀錄"),"detail:source:"+detail.server+":events"));content.append(button(ui("查看來源頁面"),()=>{$("detail-dialog").close();switchTab("mcp");selectMcpSource(detail.server);}));
  }else if(detail.kind==="mcp"){
    const e=detail.event,full=e.server==="web"?"web__run":"mcp__"+e.server+"__"+e.tool;
    $("detail-title").textContent=e.server+" · "+e.tool;purposeBlock(content,full);
    content.append(metadataList([[ui("來源"),e.server],[ui("分類"),ui(data.mcp.categories[e.category])||e.category],[ui("紀錄方式"),e.nested?ui("exec 程式碼中的呼叫位置"):ui("工具呼叫")],[ui("時間"),when(e.timestamp)],[ui("回傳時間"),when(e.completed_at)],[ui("耗時 (ms)"),fmt(e.duration_ms)],[ui("exec 整次耗時 (ms)"),fmt(e.container_duration_ms)],[ui("結果"),labels.status[e.result?.status]||e.result?.status||ui("未知")],["Call ID",e.call_id]]));
    const names={format:ui("文件格式"),ocr_mode:ui("OCR 模式"),write:ui("寫入設定"),write_output:ui("寫出文件"),max_files:ui("檔案數量上限"),written:ui("已寫入"),truncated:ui("內容截斷"),content_chars:ui("文字長度 (字元)"),ocr_status:ui("OCR 結果"),ocr_eligible_items:ui("待處理 OCR 項目"),ocr_processed_items:ui("已處理 OCR 項目"),ocr_omitted_items:ui("省略 OCR 項目"),ocr_errors:ui("OCR 錯誤數"),changed_files:ui("差異檔案數"),only_in_source_files:ui("只在來源的檔案數"),only_in_target_files:ui("只在目標的檔案數"),evidence_runs:ui("驗證紀錄數"),checks_passed:ui("檢查通過數"),checks_failed:ui("檢查失敗數"),checks_skipped:ui("略過檢查數"),findings:ui("發現項目數"),findings_critical:"Critical",findings_high:"High",findings_medium:"Medium",findings_low:"Low",retries:ui("重試次數"),http_attempts:ui("HTTP 嘗試次數"),input_tokens:"Input tokens",output_tokens:"Output tokens"};
    const selected={...e.metadata,...e.result},items=Object.entries(selected).filter(([key,v])=>!["references","resources","status"].includes(key)&&["string","boolean","number"].includes(typeof v)).map(([key,v])=>[names[key]||key,typeof v==="boolean"?(v?ui("是"):ui("否")):typeof v==="number"?fmt(v):labels.status[v]||v]);
    if(items.length)content.append(node("h4",ui("操作摘要")),metadataList(items));
    const ids=Object.entries(e.metadata?.resources||{});if(ids.length)content.append(node("h4",ui("資源 ID")),metadataList(ids));
    const t=data.codex.threads.find(t=>t.thread_id===e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
    const urls=[...new Set([...(e.metadata?.references||[]),...(e.result?.references||[])])];if(urls.length){content.append(node("h4",ui("參考網址")+" ("+fmt(urls.length)+ui(" 個")+")"));for(const url of urls)content.append(referenceLink(url));}
  }
  if(detail.kind==="source-record"){const {server,record}=detail;$("detail-title").textContent=server+" · "+ui("來源紀錄");content.append(metadataList(telemetryFields(record).map(([key,value])=>[metricLabel(key),telemetryValue(value)])));for(const [key,value]of Object.entries(record))if(Array.isArray(value)&&value.length){const fields=[...new Set(value.flatMap(item=>telemetryFields(item).map(([key])=>key)))];content.append(node("h4",metricLabel(key)),table(fields.map(metricLabel),value.map(item=>{const row=node("tr");for(const field of fields)cell(row,telemetryValue(item[field]));return row;}),metricLabel(key),"source:"+server+":"+key));}}
  if(detail.kind==="log"){
    const e=detail.event;$("detail-title").textContent=logMessage(e);
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("等級"),logLevelLabels[e.severity]||e.severity],[ui("來源"),logSource(e.source)],[ui("模組"),e.module||"--"],[ui("代碼"),e.code||"--"],[ui("操作"),e.method||e.tool||ui("未知")],[ui("錯誤類型"),e.error_type||"--"],[ui("檔案"),e.file||"--"],[ui("紀錄 ID"),fmt(e.record_id)],["Thread ID",e.thread_id||"--"],["HTTP status",fmt(e.http_status)]]));
    if(e.thread_id)content.append(threadLink(threadIndex.get(e.thread_id)||{...e,metadata_only:true,event_only:true},ui("查看對話")));
    content.append(node("h4",ui("紀錄欄位")),node("pre",JSON.stringify(e,null,2),"payload"));
  }
  if(detail.kind==="sqlite"){
    const e=detail.event;$("detail-title").textContent=ui("SQL 操作")+" · "+e.statement;
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("SQL 操作"),e.statement],[ui("操作類型"),sqlLabels[e.operation]||e.operation],[ui("資料庫引擎"),e.engine],[ui("資料庫位置"),sqlLocation(e)||ui("未知")],[ui("工作目錄"),e.workdir||"--"],[ui("紀錄方式"),sqlMethod(e)],[ui("工具"),e.tool||ui("未知")],[ui("工具回傳狀態"),sqlResults[e.result]||e.result],[ui("工具整次耗時 (ms)"),fmt(e.container_duration_ms)],[ui("SQL 耗時 (ms)"),fmt(e.duration_ms)],[ui("影響列數"),fmt(e.rows_affected)],[ui("回傳列數"),fmt(e.rows_returned)],[ui("來源"),e.source?logSource(e.source):ui("對話紀錄")],[ui("模組"),e.module||"--"],["Call ID",e.call_id||"--"]]),node("p",ui("操作來自工具紀錄或 SQL 診斷 metadata. 原始 SQL 與查詢資料不保存"),"muted"));
    if(e.thread_id)content.append(threadLink(threadIndex.get(e.thread_id)||{...e,event_only:true,metadata_only:true},ui("查看對話")));
  }
  if(detail.kind==="log-source"){
    const s=(logData?.sources||data.logs?.sources||[]).find(source=>source.source===detail.source.source)||detail.source;$("detail-title").textContent=logSource(s.source)+ui(" · 觀察來源");
    const hints=editableLabels({missing:"目前未找到來源檔案, 來源產生紀錄後會自動讀取",config_missing:"尚未設定此來源, 請先確認來源紀錄已啟用",unsupported:"來源資料格式與目前可讀取的欄位不相符",unavailable:"目前無法存取來源, 請檢查檔案權限或資料庫使用狀態",partly_unavailable:"部分來源無法讀取, 其餘可讀取的紀錄會繼續更新",disabled:"此來源已停用, 可從 Tab 或總體設定啟用"});
    content.append(node("p",hints[s.health]||ui("來源已取得的欄位與目前讀取範圍如下")),metadataList([[ui("狀態"),logHealthLabels[s.health]||s.health],[ui("最近檢查"),when(s.checked_at)],[ui("錯誤類型"),s.error_type||ui("無")],[ui("已讀取 (行)"),fmt(s.read_lines)],[ui("已辨識 (行)"),fmt(s.parsed_lines)],[ui("未辨識 (行)"),fmt(s.unsupported_lines)],[ui("過長紀錄 (行)"),fmt(s.oversized_lines)],[ui("歷史回補"),s.backfill_pending==null?ui("不適用"):s.backfill_pending?ui("分批回補中"):ui("已完成目前來源回補")],[ui("每輪讀取上限"),s.read_limit==null?ui("未知"):fmt(s.read_limit)+" "+s.read_unit],[ui("保存大小 (bytes)"),fmt(s.bytes)],[ui("保存上限 (bytes)"),fmt(s.byte_limit)]]));
    if(s.files?.length)content.append(table([ui("檔案"),ui("已讀取位置 (bytes)"),ui("待完成片段 (bytes)"),ui("紀錄 ID")],s.files.map(file=>{const row=node("tr");cell(row,file.name,"mono");valueCell(row,file.offset);valueCell(row,file.pending_bytes);valueCell(row,file.record_id);return row;}),ui("Log 來源檔案")));
    if(s.source==="monitor")content.append(node("p",ui("程式事件保存於 CODEX_HOME/monitoring/local-activity-monitor.jsonl, 最多保留目前與上一份檔案. 效能樣本與本輪狀態事件仍保存在記憶體"),"muted"));
    if(s.source==="session"||s.source==="jev_telemetry")content.append(button(ui("查看完整操作紀錄"),()=>{$("detail-dialog").close();switchTab(s.source==="session"?"codex":"jev");},"link"));
  }
  if(detail.kind==="thread")groupThreadDetails(content);
  attachTables();
}
async function loadSkillDocuments(selected,path){
  const token={};selected.documentRequest=token;selected.documentLoading=true;selected.documentError=null;if(path)selected.documentPath=path;if(detail===selected)renderDetail();
  try{
    const response=await request("/api/codex/skill?"+new URLSearchParams({skill:selected.skill,...path?{file:path}:{}}),{cache:"no-store"});if(!response.ok)throw new Error(ui("Skill 文件觀察已關閉"));if(selected.documentRequest!==token)return;
    selected.files=response.data.files;selected.root=response.data.root;selected.filesTruncated=response.data.files_truncated;
    const documents=response.data.documents;selected.documents=path?[...(selected.documents||[]).filter(file=>file.relative_path!==path),...documents]:documents;
    if(!path)selected.documentPath=(documents.find(file=>file.name.toLowerCase()==="readme.md")||documents[0])?.relative_path;
    if(path&&!documents.length)selected.documentError=ui("文件已移動或無法讀取, 請重新開啟 Skill");
  }catch(error){if(selected.documentRequest===token)selected.documentError=error.message||ui("文件讀取失敗");}finally{if(selected.documentRequest===token){selected.documentLoading=false;if(detail===selected)renderDetail();}}
}
async function openJev(call,thread,remember=true,scroll=0){
  if(remember)rememberDetail();clearDetailTabs();updateDetailBack();
  const selected={kind:"jev",call,thread};detail=selected;$("detail-title").textContent="Jev "+call.operation+ui(" · 送出 / 回傳");$("detail-content").replaceChildren(node("p",ui("讀取內容中"),"empty"));
  if(!$("detail-dialog").open)openDialog($("detail-dialog"));
  try{
    const query=new URLSearchParams({thread:call.thread_id,call:call.call_id,index:call.index});
    const response=await request("/api/codex/jev?"+query,{cache:"no-store"});if(!response.ok)throw new Error(ui("此項觀察已關閉"));
    const result=response.data;if(detail!==selected)return;
    const content=$("detail-content");content.replaceChildren();
    const t=thread||data.codex.threads.find(item=>item.thread_id===call.thread_id);if(t)content.append(threadLink(t,ui("返回對話 · ")+title(t)));
    content.append(metadataList([[ui("呼叫時間"),when(call.timestamp)],[ui("回傳時間"),when(call.completed_at)],[ui("工具 Call ID"),call.call_id]]));
    content.append(node("h4",ui("送出至 Jev")),node("pre",result.request==null?ui("本機紀錄未提供獨立的 Jev request"):JSON.stringify(result.request,null,2),"payload"));
    content.append(node("h4",result.response_scope==="containing_tool_call"?ui("包含 Jev 的工具呼叫回傳"):ui("Jev 回傳")),node("pre",result.response==null?ui("本機紀錄未提供回傳內容"):typeof result.response==="string"?result.response:JSON.stringify(result.response,null,2),"payload"));
  }catch(error){if(detail===selected)$("detail-content").replaceChildren(node("p",error.message||ui("內容讀取失敗"),"empty"));}
  finally{if(detail===selected)$("detail-content").scrollTop=scroll;}
}
document.addEventListener("visibilitychange",()=>{if(!document.hidden)refresh();});
function schedule(seconds){if(refreshTimer&&refreshSeconds===seconds)return;refreshSeconds=seconds;if(refreshTimer)clearInterval(refreshTimer);refreshTimer=setInterval(()=>{if(!document.hidden)refresh();},seconds*1000);$("refresh-note").textContent=ui("每 ")+seconds+ui(" 秒更新");}
async function request(url,options){const control=new AbortController(),timer=setTimeout(()=>control.abort(),15000);try{const response=await fetch(url,{...options,signal:control.signal}),body=await response.text();return {ok:response.ok,status:response.status,data:response.headers.get("Content-Type")?.includes("application/json")?JSON.parse(body):body};}finally{clearTimeout(timer);}}
function compatibleSettings(saved,current,categories){
  const next={};for(const [key,min,max]of [["interval",1,3600],["max_files",1,5000]])if(Number.isInteger(saved[key])&&saved[key]>=min&&saved[key]<=max)next[key]=saved[key];if(typeof saved.track_all==="boolean")next.track_all=saved.track_all;
  const observations=Object.fromEntries(Object.entries(saved.observations||{}).filter(([key,value])=>Object.hasOwn(current.observations,key)&&typeof value==="boolean"));if(Object.keys(observations).length)next.observations=observations;
  for(const key of ["mcp_sources","mcp_categories","tool_descriptions","mcp_descriptions","mcp_tags"]){const entries=Object.entries(saved[key]||{}).filter(([name,value])=>["tool_descriptions","mcp_descriptions"].includes(key)?/^[a-zA-Z0-9_.:-]{1,160}$/.test(name)&&typeof value==="string"&&value.length<=400:/^[a-zA-Z0-9_.-]{1,80}$/.test(name)&&(key==="mcp_sources"?typeof value==="boolean":key==="mcp_tags"?Array.isArray(value)&&value.length<=4&&value.every(text=>typeof text==="string"&&text.trim().length<=40&&text.trim()):typeof value==="string"&&Object.hasOwn(categories,value))).slice(0,64);if(entries.length)next[key]=Object.fromEntries(entries);}
  return next;
}
async function refresh(){
  if(busy){refreshQueued=true;return;}busy=true;const current=version;
  try{
    const url="/api/snapshot?window="+encodeURIComponent($("window").value);
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
  catch{$("live").textContent=ui("暫時無法連線, 將自動重試");}finally{busy=false;if(refreshQueued){refreshQueued=false;refresh();}}
}
async function post(url,value){let response;try{response=await request(url,{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(value)});}catch{throw new Error(ui("設定格式無效或連線中斷"));}if(!response.ok){const error=new Error(response.data?.error||ui("設定更新失敗"));error.status=response.status;throw error;}return response.data;}
async function changeSettings(value){
  if(settingsBusy)throw new Error(ui("設定更新中"));
  settingsBusy=true;version++;feedback("settings-message",ui("正在套用設定"),"pending");
  try{const next=await post("/api/settings",value);data.settings=next;schedule(next.interval);$("refresh-interval").value=next.interval;$("session-count").disabled=next.track_all;$("apply-session-count").disabled=next.track_all;saveView();feedback("settings-message",value.interval!=null?ui("更新頻率已套用: ")+next.interval+ui(" 秒"):value.max_files!=null?ui("追蹤數量已套用: ")+next.max_files:ui("設定已套用"));}
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
$("track-all").addEventListener("change",async()=>{const input=$("track-all"),value=input.checked;input.disabled=true;try{await changeSettings({track_all:value});}catch{input.checked=!value;}finally{input.disabled=false;}});
function renderObservationSettings(){
  const monitoring=$("observation-data"),mcp=$("observation-mcp"),recording=$("observation-recording");monitoring.replaceChildren();mcp.replaceChildren();recording.replaceChildren();
  const purposes={codex:"讀取對話狀態與 Token 紀錄",metadata:"補充對話名稱, 專案與執行設定",usage:"讀取帳戶額度與 Credits 快照",tool_events:"辨識工具呼叫與回傳時間",web:"整理網路操作與參考頁面",files:"辨識檔案讀寫操作",git:"辨識 Git 操作",skills:"保存 Skills 讀取紀錄",checks:"辨識 test, build 與 lint 操作",sqlite:"辨識 SQL 操作與 SQLite 診斷",errors:"整理錯誤明細與最近 24 小時紀錄",logs:"讀取來源 Log 與執行紀錄",mcp:"辨識 MCP 操作與結果",jev:"讀取 Jev 用量與耗時",jev_calls:"讀取指定 Jev 呼叫的送出與回傳內容"};
  function observation(key,parent){if(data.settings.observations[key]==null||(key.startsWith("jev")&&!data.availability?.jev))return;const row=node("label",null,"setting-row"),input=node("input"),name=node("span",observationLabels[key]||key);input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=!!data.settings.observations[key];input.name=key;bindHelp(name,ui(purposes[key]));row.append(name,input);input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({observations:{[key]:requested}});}catch{input.checked=!requested;}finally{input.disabled=false;}});parent.append(row);}
  const groups=node("div",null,"observation-groups");for(const [title,keys]of [["對話與用量",["codex","metadata","usage","tool_events"]],["專案活動",["files","git","skills","checks","web"]],["診斷紀錄",["errors","logs","sqlite"]]]){const card=node("section",null,"observation-group");card.append(node("h4",ui(title)));for(const key of keys)observation(key,card);groups.append(card);}monitoring.append(groups);
  for(const key of ["mcp"])observation(key,mcp);
  const searchLabel=node("label",ui("搜尋 MCP"),"document-label"),search=node("input"),sources=node("div",null,"source-settings-grid");search.type="search";search.placeholder=ui("來源名稱或標籤");searchLabel.append(search);mcp.append(searchLabel,sources);
  for(const source of data.mcp?.servers||[]){
    const card=node("section",null,"source-setting-card"),head=node("label",null,"setting-row"),input=node("input");input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=source.enabled;input.setAttribute("aria-label",source.server+ui(" 觀察開關"));head.append(node("strong",source.server,"mono"),input);card.append(head);input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({mcp_sources:{[source.server]:requested}});}catch{input.checked=!requested;}finally{input.disabled=false;}});
    const row=node("label",ui("分類"),"setting-row"),select=node("select");select.setAttribute("aria-label",source.server+ui(" 分類"));for(const [key,text]of Object.entries(data.mcp.categories)){const option=node("option",ui(text));option.value=key;select.append(option);}select.value=source.category;row.append(select);card.append(row);select.addEventListener("change",async()=>{select.disabled=true;try{await changeSettings({mcp_categories:{[source.server]:select.value}});}catch{select.value=source.category;}finally{select.disabled=false;}});
    const labels=node("label",ui("用途標籤"),"document-label"),tags=node("input"),defaultTags=mcpTags({...source,tags:null},data.mcp.events||[]).map(tag=>tag.textContent);tags.type="text";tags.maxLength=180;tags.value=(source.tags||defaultTags).join(", ");tags.setAttribute("aria-label",source.server+ui(" 用途標籤"));bindHelp(labels,ui("最多 4 個標籤, 每個 40 字, 以逗號分開"));labels.append(tags);card.append(labels);
    const description=node("textarea"),descriptionLabel=node("label",ui("用途說明"),"document-label");description.rows=3;description.maxLength=400;description.value=source.description||"";description.setAttribute("aria-label",source.server+ui(" 用途說明"));descriptionLabel.append(description);
    const actions=node("div",null,"settings-actions"),save=button(ui("儲存"),async()=>{const list=[...new Set(tags.value.split(/[,，]/).map(text=>text.trim()).filter(Boolean))];if(list.length>4||list.some(text=>text.length>40)){feedback("settings-message",ui("最多 4 個標籤, 每個 40 字, 以逗號分開"),"error");return;}save.disabled=true;try{await changeSettings({mcp_descriptions:{[source.server]:description.value.trim()===source.default_description?"":description.value.trim()},mcp_tags:{[source.server]:JSON.stringify(list)===JSON.stringify(defaultTags)?[]:list}});feedback("settings-message",ui("MCP 設定已儲存"));renderObservationSettings();}catch{}finally{save.disabled=false;}}),reset=button(ui("還原預設"),async()=>{try{await changeSettings({mcp_descriptions:{[source.server]:""},mcp_tags:{[source.server]:[]},mcp_categories:{[source.server]:source.default_category}});renderObservationSettings();}catch{}});reset.hidden=!["mcp_descriptions","mcp_tags","mcp_categories"].some(key=>Object.hasOwn(data.settings[key]||{},source.server));actions.append(save,reset);card.append(descriptionLabel,actions);card.dataset.sourceSearch=(source.server+" "+(source.tags||defaultTags).join(" ")).toLowerCase();sources.append(card);
  }
  search.addEventListener("input",()=>{const query=search.value.trim().toLowerCase();for(const card of sources.children)card.hidden=!card.dataset.sourceSearch.includes(query);});
  recording.append(node("p",ui("紀錄設定由各 MCP 來源管理"),"muted"));
  for(const source of data.mcp?.servers||[]){if(source.server==="web")continue;const state=data.mcp.recording_status?.[source.server],row=node("div",null,"setting-row");row.append(node("span",source.server,"mono"),tag(state?.enabled==null?"--":ui(state.enabled?"已啟用":"已停用")));if(state?.health)row.append(node("span",logHealthLabels[state.health]||state.health,"muted"));recording.append(row);}if(!data.mcp?.servers?.length)recording.append(node("p","--","empty"));syncHelp();syncModalDrag($("settings-dialog"));
}
function openSettings(){
  $("language-select").value=locale;
  $("display-options").value=display.options.join(", ");fillDisplayOptions($("default-ranking"),display.ranking);fillDisplayOptions($("default-table"),display.table);
  if(!data)return;applyAppearance();$("refresh-interval").value=data.settings.interval;$("session-count").value=data.settings.max_files;$("session-count").disabled=data.settings.track_all;$("apply-session-count").disabled=data.settings.track_all;$("track-all").checked=data.settings.track_all;
  renderObservationSettings();
  docTools=[...new Set([...docTools,...Object.keys(data.codex.tools||{}),...Object.keys(data.codex.nested_tools||{}),...Object.keys(data.settings.tool_descriptions||{})])].sort();renderToolDocs();
  $("settings-message").textContent="";if(!$("settings-dialog").open)openDialog($("settings-dialog"));
}

const tableViews=new Map(),specialTables=new Set(["codex-rows","mcp-rows","docs-rows"]);
const tableStates=Object.fromEntries(Object.entries(preferences.tables||{}).filter(([key,value])=>key.length<=400&&value&&(value.size==="all"||Number.isInteger(value.size)&&value.size>=1&&value.size<=200)&&Number.isInteger(value.page)&&value.page>0).slice(0,500));
if((preferences.tableSchema||1)<2&&tableStates["codex-rows"]?.sort?.column>=3)tableStates["codex-rows"].sort.column++;
if((preferences.tableSchema||1)<3){
  const mappings={"codex-rows":[0,3,6,8,9,10,11,12,13,14,15],"mcp-rows":[0,1,2,4,7,8],"file-rows":[0,1,2,3,4,6,7],"jev-rows":[0,1,3,4,5,7,8]};
  for(const [key,mapping]of Object.entries(mappings))if(tableStates[key]?.sort?.column>=0)tableStates[key].sort.column=mapping[tableStates[key].sort.column]??-1;
}
if((preferences.tableSchema||1)<4)for(const key of ["mcp-rows","file-rows","web-event-rows"])if(tableStates[key]?.sort?.column>=2)tableStates[key].sort.column++;
if((preferences.tableSchema||1)<5&&tableStates["web-event-rows"]?.sort?.column>=5)tableStates["web-event-rows"].sort.column=tableStates["web-event-rows"].sort.column===5?6:8;
if((preferences.tableSchema||1)<6){const state=tableStates["web-event-rows"];if(state?.sort){if(state.sort.header==="參考網址"){state.sort.column=-1;delete state.sort.header;}else if(state.sort.column>=6)state.sort.column--;}if(state?.hidden)state.hidden=state.hidden.filter(label=>!["參考網址","參考網址 (個)"].includes(label));}
preferences.tableSchema=6;
const defaultHiddenColumns={"sqlite-rows":["操作類型","資料庫引擎","紀錄方式","工具整次耗時 (ms)","Call ID","影響列數","回傳列數"],"log-rows":["專案","操作","錯誤類型","檔案","紀錄 ID"],"log-source-rows":["已讀取 (bytes)","已讀取 (行)","過長紀錄 (行)"],"error-rows":["專案","工具"],"codex-rows":["Thread ID","類型","位置","活動來源","Cached input","Reasoning output"],"mcp-rows":["分類","紀錄方式","操作摘要"],"file-rows":["紀錄方式"],"jev-rows":["HTTP 結果"],"web-event-rows":["紀錄方式"]};
let tableSettingsKey=null;

const sortCollator=new Intl.Collator("zh-TW",{numeric:true,sensitivity:"base"});
function comparable(value){if(value==null||value===""||value==="--"||value===ui("未知")||value===ui("--"))return null;if(typeof value==="number")return value;const text=String(value).trim();if(/^-?[\d,]+(?:\.\d+)?$/.test(text))return Number(text.replaceAll(",",""));const date=text.match(/^(\d{4})[/-](\d{1,2})[/-](\d{1,2})[ T](\d{1,2}):(\d{2})(?::(\d{2}))?/);if(date)return new Date(Number(date[1]),Number(date[2])-1,Number(date[3]),Number(date[4]),Number(date[5]),Number(date[6]||0)).getTime();return text;}
function compareValues(a,b,descending){a=comparable(a);b=comparable(b);if(a===null)return b===null?0:1;if(b===null)return -1;const result=typeof a==="number"&&typeof b==="number"?a-b:sortCollator.compare(String(a),String(b));return descending?-result:result;}
function sortColumn(key,sort){const view=tableViews.get(key);return sort.header&&view?.headers.includes(sort.header)?view.headers.indexOf(sort.header):sort.column;}
function sortRecords(key,items,columns,time){const sort=tableStates[key]?.sort||{column:-1,descending:true},index=sortColumn(key,sort),get=index<0?time:columns[index]||time;return [...items].sort((a,b)=>compareValues(get(a),get(b),sort.descending));}
function sortRows(key,view,rows=view.rows){const sort=tableStates[key]?.sort||{column:-1,descending:true},column=sortColumn(key,sort),index=column<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):column;const get=row=>index<0?row.querySelector("[data-sort-time]")?.dataset.sortTime:rowCells(row)[index]?.dataset.sortValue??rowValue(row,index);return [...rows].sort((a,b)=>compareValues(get(a),get(b),sort.descending));}
function changeTableSort(key,sort){if(sort.column>=0)sort.header=tableViews.get(key)?.headers[sort.column];const size=key==="codex-rows"?$("page-size").value:tableStates[key]?.size||display.table;tableStates[key]={...tableStates[key],size:size==="all"?"all":Number(size),page:1,sort};if(key==="codex-rows"){page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);persistTables();attachTables();if($("table-dialog").open)renderTableSort(key);}
function renderTableSort(key){const view=tableViews.get(key),sort=tableStates[key]?.sort||{column:-1,descending:true};$("table-sort-column").replaceChildren(...[[-1,ui("時間 (預設)")],...view.headers.map((label,index)=>[index,ui(label)])].map(([value,label])=>{const option=node("option",label);option.value=value;return option;}));$("table-sort-column").value=String(sortColumn(key,sort));$("table-sort-direction").value=sort.descending?"desc":"asc";}
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
    controls.dataset.filterKey=key;if(!existingControls)wrap.previousElementSibling.before(controls);view.filters={controls,fields,search};
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
function syncTableHead(view){
  const head=view.table.tHead;if(!head)return;
  if(!view.table.closest(".table-scroll")){head.style.transform="";return;}
  const rect=view.table.getBoundingClientRect(),content=view.table.closest("dialog")?.querySelector("#detail-content,.settings-content"),top=content?content.getBoundingClientRect().top:0,offset=Math.max(0,Math.min(top-rect.top,rect.height-head.offsetHeight));
  head.style.transform="translateY("+offset+"px)";
}
let tableHeaderFrame=null;
window.addEventListener("scroll",()=>{if(tableHeaderFrame!=null)return;tableHeaderFrame=requestAnimationFrame(()=>{tableHeaderFrame=null;for(const view of tableViews.values())if(view.table.getClientRects().length&&view.table.closest(".table-scroll"))syncTableHead(view);});},{capture:true,passive:true});
const defaultColumnHeaders=["最近活動時間","時間","呼叫時間","對話名稱","對話","專案","Skill","操作","SQL 操作","檔案位置","資料庫位置","工具","來源","Model","Reasoning 等級","狀態","結果","工具回傳狀態","錯誤說明","等級","操作類型","資料庫引擎","類型","位置","活動來源","Input","Cached input","Output","Reasoning output","Total","工具 (次)","工作總耗時 (秒)","耗時 (ms)","工具耗時 (ms)","工具整次耗時 (ms)","工具回傳時間","紀錄方式","代碼","Thread ID","Call ID"];
function columnOrder(key){const view=tableViews.get(key),saved=tableStates[key]?.columns,defaults=key==="docs-rows"?view.headers:defaultColumnHeaders.filter(header=>view.headers.includes(header)).concat(view.headers);return [...new Set((Array.isArray(saved)?saved:[]).filter(header=>view.headers.includes(header)).concat(defaults))].map(header=>view.headers.indexOf(header));}
function applyTableHeatmap(key){
  const view=tableViews.get(key),enabled=tableStates[key]?.heatmap===true;if(!enabled&&!view.heatmapActive)return;view.heatmapActive=enabled;const rows=[...view.body.children],maximum=[];
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
  $("table-columns").replaceChildren(...columnOrder(key).map(index=>{const header=view.headers[index],option=node("div",null,"column-option"),label=node("label"),input=node("input"),handle=option;option.tabIndex=0;option.dataset.columnKey=header;handle.setAttribute("aria-label",ui(header)+ui(" 欄位拖曳排序"));handle.title=ui("拖曳排序, 或用方向鍵移動");input.type="checkbox";input.checked=!hidden.includes(header);input.addEventListener("change",()=>{const next=new Set(tableStates[key]?.hidden||defaultHiddenColumns[key]||[]);if(input.checked)next.delete(header);else next.add(header);if(view.headers.every(name=>next.has(name))){input.checked=true;feedback("table-settings-message",ui("至少保留一個欄位"),"error");return;}tableStates[key]={...tableStates[key],hidden:[...next]};applyTableColumns(key);persistTables();feedback("table-settings-message",ui("顯示欄位已更新"));});label.append(input,node("span",ui(header)));option.append(label);dragColumn(handle,key,header);return option;}));
}
function tableSize(key,fallback=display.table){const size=tableStates[key]?.size??fallback;return size==="all"?Number.MAX_SAFE_INTEGER:size;}
function replaceRows(id,...rows){const body=$(id);body.replaceChildren(...rows);const view=tableViews.get(id);if(view)view.sourceChanged=true;}
function attachTables(){
  for(const el of document.querySelectorAll("table")){
    const body=el.querySelector("tbody"),key=el.dataset.tableKey||body?.id;if(!body||!key)continue;
    let view=tableViews.get(key);
    if(!view||view.table!==el){
      const wrap=el.closest(".table-wrap"),previous=wrap.previousElementSibling,caption=el.dataset.tableTitle||(/^H[1-4]$/.test(previous?.tagName)?previous.textContent:previous?.querySelector("h3,h4")?.textContent||wrap.closest(".panel,dialog")?.querySelector("h3,h4")?.textContent||ui("表格")),toolbar=node("div",null,"table-toolbar"),gear=button("⚙",()=>openTableSettings(key),"chart-setting-button");gear.setAttribute("aria-label",caption.trim()+ui(" 表格設定"));gear.title=ui("表格設定");toolbar.append(gear);wrap.before(toolbar);
      const headerCells=[...el.querySelectorAll("th")];view={table:el,body,rows:[],first:null,title:caption.trim(),headerCells,headers:headerCells.map(th=>th.dataset.copyBase||th.textContent)};tableViews.set(key,view);
      if(!specialTables.has(key)){const footer=node("div",null,"pagination table-pagination"),summary=node("span"),controls=node("div"),prev=button("←",()=>moveTablePage(key,-1)),next=button("→",()=>moveTablePage(key,1)),label=node("label",ui("第 ")),input=node("input"),total=node("span");prev.setAttribute("aria-label",ui("上一頁"));next.setAttribute("aria-label",ui("下一頁"));input.type="number";input.min=1;input.className="table-page-input";input.setAttribute("aria-label",view.title+ui(" 頁碼"));pageInput(input,()=>{tableStates[key]={...tableStates[key],size:tableStates[key]?.size||display.table,page:Number(input.value)};paginateTable(key);persistTables();});label.append(input,node("span",ui(" 頁 ")));controls.append(prev,label,total,next);footer.append(summary,controls);wrap.after(footer);Object.assign(view,{footer,summary,prev,next,input,total});}
    }
    if(!specialTables.has(key)){if(view.sourceChanged||body.firstChild!==view.first){view.rows=[...body.children];view.sourceChanged=false;}renderTableFilters(key);paginateTable(key);}
    applyTableColumns(key);
    const sort=tableStates[key]?.sort||{column:-1,descending:true},column=sortColumn(key,sort),active=column<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):column;
    for(const [index,th]of view.headerCells.entries()){th.setAttribute("aria-sort",index===active?sort.descending?"descending":"ascending":"none");if(!th.querySelector("button")){const control=button("",()=>{const current=tableStates[key]?.sort||{column:-1,descending:true},selected=sortColumn(key,current),column=selected<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):selected;changeTableSort(key,{column:index,descending:column===index?!current.descending:false});},"sort-header");th.replaceChildren(control);th.dataset.columnKey=view.headers[index];th.title=(fieldDescription(view.headers[index])?fieldDescription(view.headers[index])+"; ":"")+ui("點擊升降冪排序. 拖曳調整欄位順序. Alt + 方向鍵也可移動");dragColumn(th,key,view.headers[index],true);}th.querySelector("button").textContent=ui(view.headers[index])+(index===active?sort.descending?" ↓":" ↑":" ↕");}
    adaptTable(view);

  }
  attachFilterSections();for(const dialog of document.querySelectorAll("dialog"))syncModalDrag(dialog);syncHelp();
}
function paginateTable(key){
  const view=tableViews.get(key);if(!view)return;const state=tableStates[key]||{size:display.table,page:1},size=tableSize(key),rows=matchingRows(key,view.rows),pages=Math.max(1,Math.ceil(rows.length/size));state.page=Math.max(1,Math.min(state.page,pages));tableStates[key]=state;
  const start=(state.page-1)*size;view.body.replaceChildren(...sortRows(key,view,rows).slice(start,start+size));view.first=view.body.firstChild;syncPageInput(view.input,state.page,pages);view.total.textContent="/ "+pages;view.summary.textContent=ui("共 ")+fmt(rows.length)+ui(" 筆")+(rows.length!==view.rows.length?ui(" · 全部 ")+fmt(view.rows.length)+ui(" 筆"):"");view.prev.disabled=state.page===1;view.next.disabled=state.page===pages;view.footer.hidden=!view.rows.length;applyTableColumns(key);
}
function persistTables(){preferences.tables=tableStates;saveView();}
function moveTablePage(key,delta){const state=tableStates[key]||{size:display.table,page:1};state.page+=delta;tableStates[key]=state;paginateTable(key);persistTables();}
function openTableSettings(key){tableSettingsKey=key;const view=tableViews.get(key);$("table-settings-title").textContent=(view?.title||ui("表格"))+ui(" · 表格設定");$("table-size").value=key==="codex-rows"?$("page-size").value:String(tableStates[key]?.size||display.table);$("table-settings-message").textContent="";$("table-heatmap").checked=tableStates[key]?.heatmap===true;$("table-heatmap").disabled=!Array.from(view.body.querySelectorAll("[data-heat-value]")).length;fillDisplayOptions($("table-size"),$("table-size").value);renderTableSort(key);renderTableColumns(key);if(!$("table-dialog").open)openDialog($("table-dialog"));}
$("table-size").addEventListener("change",()=>{const key=tableSettingsKey,value=$("table-size").value;tableStates[key]={...tableStates[key],size:value==="all"?"all":Number(value),page:1};if(key==="codex-rows"){$("page-size").value=value;page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);persistTables();feedback("table-settings-message",ui("顯示數量已更新"));});
$("table-heatmap").addEventListener("change",()=>{const key=tableSettingsKey;tableStates[key]={...tableStates[key],heatmap:$("table-heatmap").checked};applyTableHeatmap(key);persistTables();feedback("table-settings-message",ui("數值熱度已更新"));});
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
  $("docs-page-summary").textContent=ui("第 ")+docsPage+" / "+pages+ui(" 頁 · ")+tools.length+(docsMode==="tools"?" 個工具":" 項介面文字");$("docs-prev").disabled=docsPage===1;$("docs-next").disabled=docsPage===pages;syncPageInput($("docs-page-number"),docsPage,pages);attachTables();
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
  const options=[...new Set($("display-options").value.split(/[,\s]+/).filter(Boolean).map(value=>Math.round(Number(value))))],ranking=$("default-ranking").value,table=$("default-table").value,next={options,ranking:ranking==="all"?"all":Number(ranking),table:table==="all"?"all":Number(table)};
  if(!validDisplay(next)){feedback("display-message",ui("請輸入 1 到 200 的整數, 最多 8 個選項, 以逗號分開"),"error");return;}
  display=next;$("display-options").value=options.join(", ");for(const view of chartViews.values())if(view.fields.top){view.settings.top=display.ranking==="all"?0:display.ranking;fillDisplayOptions(view.fields.top,display.ranking,true);}
  fillDisplayOptions($("page-size"),display.table);for(const key of tableViews.keys()){tableStates[key]={...tableStates[key],size:display.table,page:1};redrawTable(key);}persistTables();renderCharts();attachTables();feedback("display-message",ui("顯示數量已套用到所有排行榜與表格"));
});
function modalTabs(container,groups,id,selected,onSelect){
  const nav=node("div",null,"document-modes modal-tabs"),entries=[];nav.setAttribute("role","tablist");nav.setAttribute("aria-label",ui("明細分類"));
  for(const [index,[key,label,panel]]of groups.entries()){const tab=button(label,()=>select(key)),tabId=id+"-tab-"+index,panelId=id+"-panel-"+index;tab.id=tabId;tab.dataset.modalKey=key;tab.dataset.copyBase=label;tab.setAttribute("role","tab");tab.setAttribute("aria-controls",panelId);panel.id=panelId;panel.setAttribute("role","tabpanel");panel.setAttribute("aria-labelledby",tabId);entries.push({key,tab,panel});nav.append(tab);}
  function select(key){for(const entry of entries){const active=entry.key===key;entry.tab.setAttribute("aria-selected",String(active));entry.tab.tabIndex=active?0:-1;entry.panel.hidden=!active;}onSelect?.(key);}
  for(const [index,entry]of entries.entries())entry.tab.addEventListener("keydown",event=>{if(event.altKey)return;if(event.altKey||!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?entries.length-1:(index+(["ArrowRight","ArrowDown"].includes(event.key)?1:entries.length-1))%entries.length;select(entries[next].key);entries[next].tab.focus();});
  const previous=container.previousElementSibling;if(previous?.classList.contains("modal-tabs"))previous.remove();container.before(nav);container.replaceChildren(...entries.map(entry=>entry.panel));select(entries.some(entry=>entry.key===selected)?selected:entries[0].key);dragSubTabs(nav,id,tab=>tab.dataset.modalKey);
}
function groupThreadDetails(content){
  const groups=new Map(),headingGroup=new Map([[ui("工具使用 (次)"),"tools"],[ui("exec 內辨識到的工具"),"tools"],[ui("工具呼叫紀錄"),"tools"],[ui("MCP 觀察"),"mcp"],[ui("檔案讀寫紀錄"),"files"],[ui("MCP / Web 操作"),"mcp"],[ui("網路參考"),"mcp"],[ui("錯誤與警告"),"errors"]]),names={summary:ui("對話資訊"),tools:ui("工具"),files:ui("檔案"),mcp:"MCP",errors:ui("錯誤紀錄")};let key="summary";
  for(const child of [...content.children]){if(child.tagName==="H4"&&headingGroup.has(child.textContent))key=headingGroup.get(child.textContent);if(!groups.has(key))groups.set(key,node("section"));groups.get(key).append(child);}
  modalTabs(content,[...groups].map(([id,panel])=>[id,names[id]||id,panel]),"thread-detail",detail.section,key=>detail.section=key);
}
function groupSettings(){
  const content=$("settings-dialog").querySelector(".settings-content"),status=$("settings-message"),groups=[];status.remove();let panel;
  for(const child of [...content.children]){if(child.dataset.modalSection){if(child.dataset.modalSection==="觀察來源"){panel=null;continue;}panel=node("section");groups.push([child.dataset.modalSection,ui(child.dataset.modalSection),panel]);}panel?.append(child);}
  const position=groups.findIndex(([key])=>key==="設定檔");for(const [index,[key,id]]of [["監測項目","observation-data"],["MCP 來源","observation-mcp"],["來源紀錄","observation-recording"]].entries()){const section=node("section");section.append(node("h4",ui(key)));const target=node("div");target.id=id;section.append(target);groups.splice(position+index,0,[key,ui(key),section]);}
  modalTabs(content,groups,"settings-sections","外觀");const nav=content.previousElementSibling,layout=node("div",null,"settings-layout");nav.setAttribute("aria-label",ui("設定分類"));nav.setAttribute("aria-orientation","vertical");content.before(layout);layout.append(nav,content);status.className="settings-feedback";layout.before(status);
}
let conversationSource=["conversations","usage","dots"].includes(preferences.conversationSource)?preferences.conversationSource:"conversations";
function selectConversationSource(key){conversationSource=key;preferences.conversationSource=key;for(const item of ["conversations","usage","dots"]){const selected=item===key,tab=$("codex-"+item);tab.setAttribute("aria-selected",String(selected));tab.tabIndex=selected?0:-1;$(item==="conversations"?"conversation-content":item+"-content").hidden=!selected;}if(data){attachTables();renderCharts();saveView();}}
for(const [index,key]of ["conversations","usage","dots"].entries()){const tab=$("codex-"+key);tab.addEventListener("click",()=>selectConversationSource(key));tab.addEventListener("keydown",event=>{if(event.altKey)return;if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const items=["conversations","usage","dots"],next=event.key==="Home"?0:event.key==="End"?2:(index+(event.key==="ArrowRight"?1:2))%3;selectConversationSource(items[next]);$("codex-"+items[next]).focus();});}selectConversationSource(conversationSource);dragSubTabs($("conversation-tabs"),"codex",tab=>tab.id);
function usageWindow(minutes){return minutes==null?"--":minutes%1440===0?fmt(minutes/1440)+ui(" 天"):minutes%60===0?fmt(minutes/60)+ui(" 小時"):fmt(minutes)+ui(" 分鐘");}
let usageModelCounts={},usageModelSegments={};
function renderAllowance(id,usage){
  const target=$(id);target.replaceChildren(...(usage?.limits||[]).map(item=>{const panel=node("section",null,"allowance-window"),expired=item.resets_at&&new Date(item.resets_at)<new Date(),progress=node("progress");panel.append(node("h4",usageWindow(item.window_minutes)),metadataList([[ui("剩餘比例"),item.remaining_percent==null?"--":fmt(item.remaining_percent)+"%"],[ui("已使用"),item.used_percent==null?"--":fmt(item.used_percent)+"%"],[ui("重設時間"),when(item.resets_at)],[ui("資料狀態"),ui(expired?"等待來源更新":"來源最後回報")]]));if(item.remaining_percent!=null){progress.max=100;progress.value=item.remaining_percent;progress.setAttribute("aria-label",ui("剩餘比例")+" "+fmt(item.remaining_percent)+"%");panel.append(progress);}panel.append(node("p",ui("最近更新時間")+": "+when(usage.updated_at),"snapshot-at"));return panel;}));if(!usage?.limits?.length)target.append(node("p","--","empty"));
}
function renderUsage(){
  const usage=data.codex.usage,threads=data.codex.threads||[],groups=new Map();
  $("usage-observed").textContent=ui("最後觀測")+": "+when(usage?.updated_at);renderAllowance("usage-windows",usage);
  cards("usage-cards",[[ui("方案"),usage?.plan_type||"--",usage?.limit_id||"--"],[ui("Credits 餘額"),fmt(usage?.credits?.balance),ui("來源提供的 credits")],[ui("Credits 可用"),usage?.credits?.has_credits==null?"--":ui(usage.credits.has_credits?"是":"否"),usage?.credits?.unlimited==null?"--":ui(usage.credits.unlimited?"無上限":"有上限")]]);
  for(const thread of threads){const model=thread.model||"unknown";if(!groups.has(model))groups.set(model,{model,threads:0,known:0,complete:true,tokens:{}});const group=groups.get(model);group.threads++;const t=thread.tokens||{};group.complete&&=[t.input_tokens,t.cached_input_tokens,t.output_tokens,t.total_tokens].every(Number.isFinite)&&t.input_tokens>=t.cached_input_tokens&&t.input_tokens+t.output_tokens===t.total_tokens;if(Object.values(thread.tokens||{}).some(Number.isFinite))group.known++;for(const [key,value]of Object.entries(thread.tokens||{}))if(Number.isFinite(value))group.tokens[key]=(group.tokens[key]||0)+value;}
  replaceRows("usage-model-rows",...[...groups.values()].map(group=>{const row=node("tr");cell(row,group.model==="unknown"?"--":group.model,"mono");valueCell(row,group.threads);valueCell(row,group.known);for(const key of ["input_tokens","cached_input_tokens","output_tokens","reasoning_output_tokens","total_tokens"])valueCell(row,group.tokens[key]);valueCell(row,group.tokens.input_tokens>0&&group.tokens.cached_input_tokens!=null&&group.tokens.cached_input_tokens<=group.tokens.input_tokens?Math.round(group.tokens.cached_input_tokens/group.tokens.input_tokens*10000)/100:null);return row;}));
  usageModelCounts=Object.fromEntries([...groups.values()].filter(group=>group.tokens.total_tokens!=null).map(group=>[group.model,group.tokens.total_tokens]));usageModelSegments=Object.fromEntries([...groups.values()].map(group=>{const t=group.tokens,complete=group.complete&&[t.input_tokens,t.cached_input_tokens,t.output_tokens,t.total_tokens].every(Number.isFinite)&&t.input_tokens>=t.cached_input_tokens&&t.input_tokens+t.output_tokens===t.total_tokens;return [group.model,complete?[["一般輸入",t.input_tokens-t.cached_input_tokens],["快取輸入",t.cached_input_tokens],["輸出",t.output_tokens]]:[["未分類",t.total_tokens]]];}));
  const account=telemetryFields({plan_type:usage?.plan_type,credits_balance:usage?.credits?.balance,has_credits:usage?.credits?.has_credits,unlimited:usage?.credits?.unlimited}).filter(([,value])=>value!=null);$("overview-billing").replaceChildren(account.length?metadataList(account.map(([key,value])=>[metricLabel(key),telemetryValue(value)])):node("p","--","empty"));

  $("usage-note").textContent=ui("依模型分析對話 Token 用量與快取比例");
}
function renderDots(){const dots=data.codex.dots||{},events=dots.events||[];cards("dot-cards",[[ui("產出紀錄"),fmt(Array.isArray(dots.events)?events.length:null),ui("已載入的產出 metadata")],[ui("關聯對話"),fmt(Array.isArray(dots.events)?new Set(events.map(event=>event.thread_id)).size:null),ui("可從列表開啟對話")],[ui("活動快照項目"),fmt(dots.activity_items),ui("本機快照中的項目數")]]);replaceRows("dot-rows",...events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row,event.artifact_type||"--");cell(row,event.thread_id,"mono");return row;}));$("dot-note").textContent=ui("查看 My dots 產出時間, 類型與關聯對話");}

let currentTabSettings=null;
function tabScope(name){return name==="workflow"?$("view-"+activitySource):name==="errors"?$(diagnosticSource==="logs"?"view-logs":"error-content"):name==="codex"?$(conversationSource==="conversations"?"conversation-content":conversationSource+"-content"):$("view-"+name);}
function visibleInScope(scope,element){return scope.contains(element)&&!element.closest("[hidden]");}
function openTabSettings(name){
  currentTabSettings=name;$("overview-layout-controls").hidden=name!=="overview";if(name==="overview")renderOverviewSettings();const scope=tabScope(name),charts=[...chartViews].filter(([id])=>visibleInScope(scope,$(id))),tables=[...tableViews].filter(([,view])=>visibleInScope(scope,view.table));
  $("tab-settings-title").textContent=$("tab-"+name).textContent+ui(" · Tab 設定");$("tab-chart-controls").hidden=!charts.length;$("tab-table-controls").hidden=!tables.length;fillDisplayOptions($("tab-table-size"),display.table);$("tab-settings-message").textContent="";
  const keys={overview:[],codex:["codex","metadata","usage"],tools:["tool_events"],mcp:["mcp"],web:["web"],files:["files"],sqlite:["sqlite"],jev:["jev","jev_calls"],git:["git"],workflow:["git","checks"],skills:["skills"],checks:["checks"],monitor:[],errors:["errors","logs"],logs:["logs","codex"]}[name]||[];
  $("tab-observations").replaceChildren(...keys.filter(key=>data.settings.observations[key]!=null).map(key=>{const row=node("label",null,"setting-row"),input=node("input");input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=data.settings.observations[key];input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({observations:{[key]:requested}});feedback("tab-settings-message",ui("觀察設定已套用"));}catch(error){input.checked=!requested;feedback("tab-settings-message",error.message,"error");}finally{input.disabled=false;}});row.title=ui("啟用或停用此觀察項目, 不會修改來源紀錄");row.append(node("span",observationLabels[key]||key),input);return row;}));
  $("tab-observations-heading").hidden=!keys.length;openDialog($("tab-dialog"));
}
$("apply-tab-charts").addEventListener("click",()=>{roundNumber($("tab-chart-length"));const name=currentTabSettings,scope=tabScope(name),range=$("tab-chart-range").value,length=Number($("tab-chart-length").value),unit=Number($("tab-chart-unit").value);if(!Number.isInteger(length)||length<1||length>365){feedback("tab-settings-message",ui("請輸入 1 到 365 的整數"),"error");return;}for(const [id,view]of chartViews)if(visibleInScope(scope,$(id))){Object.assign(view.settings,{range,length,unit});for(const key of ["range","length","unit"])view.fields[key].value=view.settings[key];view.fields.length.parentElement.hidden=view.fields.unit.parentElement.hidden=range!=="recent";view.fields.start.parentElement.hidden=view.fields.end.parentElement.hidden=true;}saveView();renderCharts();feedback("tab-settings-message",ui("時間範圍已套用到此 Tab 的圖表"));});
$("apply-tab-tables").addEventListener("click",()=>{const scope=tabScope(currentTabSettings),value=$("tab-table-size").value;for(const [key,view]of tableViews)if(visibleInScope(scope,view.table)){tableStates[key]={...tableStates[key],size:value==="all"?"all":Number(value),page:1};if(key==="codex-rows")fillDisplayOptions($("page-size"),value);redrawTable(key);}persistTables();attachTables();feedback("tab-settings-message",ui("每頁筆數已套用到此 Tab 的表格"));});
$("tab-settings-close").addEventListener("click",()=>$("tab-dialog").close());
function addTabSettings(){for(const tab of document.querySelectorAll("[data-tab]")){const head=$("view-"+tab.dataset.tab).querySelector(".section-head"),gear=button("⚙",()=>openTabSettings(tab.dataset.tab),"chart-setting-button tab-settings-button");gear.setAttribute("aria-label",tab.textContent+ui(" Tab 設定"));gear.title=ui("Tab 設定");const actions=head.querySelector(".section-actions")||node("div",null,"section-actions");actions.append(gear);if(!actions.parentElement)head.append(actions);}}


function configuration(){
  saveView();preferences.tables=tableStates;preferences.tabOrder=tabOrder;const keys=["tab","inputs","page","appearance","display","charts","tables","tableSchema","tabOrder","copy","settings","locale","mcpSource","overview","highlightOrder","subOrders","cardOrders","diagnosticSource","activitySource","conversationSource","filterCollapsed"];
  return {application:"local-activity-monitor",kind:"settings",version:1,exported_at:new Date().toISOString(),preferences:Object.fromEntries(keys.filter(key=>preferences[key]!=null).map(key=>[key,preferences[key]]))};
}
function readConfiguration(value){
  const object=item=>item&&typeof item==="object"&&!Array.isArray(item),bounded=(items,max,check)=>object(items)&&Object.keys(items).length<=max&&Object.entries(items).every(([key,item])=>key.length<=400&&!["__proto__","constructor","prototype"].includes(key)&&check(item,key));
  if(!object(value)||value.application!=="local-activity-monitor"||value.kind!=="settings"||value.version!==1||!object(value.preferences)||(value.recording!=null&&typeof value.recording!=="boolean"))throw new Error(ui("設定檔格式或版本不支援"));
  const p=value.preferences,a=p.appearance,s=p.settings;
  if(p.locale!=null&&!["zh-TW","en","ja"].includes(p.locale))throw new Error(ui("設定檔格式或版本不支援"));
  if(!object(a)||!["auto","light","dark"].includes(a.mode)||!["slate","neutral"].includes(a.theme)||!["green","blue","orange"].includes(a.accent)||!Number.isInteger(a.font)||a.font<12||a.font>18||!validDisplay(p.display)||!object(s)||!Number.isInteger(s.interval)||s.interval<1||s.interval>3600||!Number.isInteger(s.max_files)||s.max_files<1||s.max_files>5000||typeof s.track_all!=="boolean")throw new Error(ui("外觀, 更新頻率或顯示數量不在可用範圍"));
  if(!bounded(s.observations,64,item=>typeof item==="boolean")||!bounded(s.mcp_sources||{},64,item=>typeof item==="boolean")||!bounded(s.mcp_categories||{},64,item=>typeof item==="string"&&item.length<=80)||!bounded(s.tool_descriptions||{},64,item=>typeof item==="string"&&item.length<=400)||!bounded(s.mcp_descriptions||{},64,item=>typeof item==="string"&&item.length<=400)||!bounded(s.mcp_tags||{},64,item=>Array.isArray(item)&&item.length<=4&&item.every(text=>typeof text==="string"&&text.trim()&&text.length<=40))||!bounded(p.copy||{},500,item=>typeof item==="string"&&item.length<=400)||!bounded(p.inputs||{},100,item=>typeof item==="string"&&item.length<=2000))throw new Error(ui("觀察設定或介面文字格式無效"));
  if(!bounded(p.charts||{},100,item=>object(item)&&["all","recent","custom"].includes(item.range)&&Number.isInteger(item.length)&&item.length>=1&&item.length<=365&&[60000,3600000,86400000].includes(item.unit)&&[60000,300000,900000,3600000,21600000,86400000].includes(item.interval)&&Number.isInteger(item.top)&&item.top>=0&&item.top<=200&&Number.isFinite(item.maximum)&&item.maximum>=0&&item.maximum<=1e9&&(item.statistics==null||[0,1].includes(item.statistics))&&["bar","line","pie","donut","column","stacked"].includes(item.shape)&&typeof item.start==="string"&&typeof item.end==="string"&&Number.isFinite(new Date(item.start).getTime())&&Number.isFinite(new Date(item.end).getTime())&&(item.range!=="custom"||new Date(item.start)<new Date(item.end)))||!bounded(p.tables||{},500,item=>object(item)&&(item.size==="all"||Number.isInteger(item.size)&&item.size>=1&&item.size<=200)&&Number.isInteger(item.page)&&item.page>=1&&(item.heatmap==null||typeof item.heatmap==="boolean")&&(!item.hidden||Array.isArray(item.hidden)&&item.hidden.length<=100&&item.hidden.every(label=>typeof label==="string"&&label.length<=400))&&(!item.columns||Array.isArray(item.columns)&&item.columns.length<=100&&item.columns.every(label=>typeof label==="string"&&label.length<=400))&&(!item.filters||bounded(item.filters,100,value=>typeof value==="string"&&value.length<=2000))&&(!item.sort||object(item.sort)&&Number.isInteger(item.sort.column)&&item.sort.column>=-1&&item.sort.column<=100&&typeof item.sort.descending==="boolean")))throw new Error(ui("圖表或表格設定格式無效"));
  if(typeof p.tab!=="string"||p.tab.length>80||!Number.isInteger(p.page)||p.page<1||!Array.isArray(p.tabOrder)||p.tabOrder.length>100||!p.tabOrder.every(key=>typeof key==="string"&&key.length<=80)||!Number.isInteger(p.tableSchema||1)||(p.tableSchema||1)>6)throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.diagnosticSource!=null&&!["errors","logs"].includes(p.diagnosticSource)||p.activitySource!=null&&!["git","skills","checks"].includes(p.activitySource))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.mcpSource!=null&&(typeof p.mcpSource!=="string"||!/^[a-zA-Z0-9_.-]{1,80}$/.test(p.mcpSource)))throw new Error(ui("MCP 來源設定格式無效"));
  if(p.conversationSource!=null&&!["conversations","usage","dots"].includes(p.conversationSource))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.highlightOrder!=null&&(!Array.isArray(p.highlightOrder)||p.highlightOrder.length>100||!p.highlightOrder.every(id=>typeof id==="string"&&id.length<=80))||p.subOrders!=null&&!bounded(p.subOrders,100,items=>Array.isArray(items)&&items.length<=100&&items.every(id=>typeof id==="string"&&id.length<=80)))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.cardOrders!=null&&!bounded(p.cardOrders,100,items=>Array.isArray(items)&&items.length<=100&&items.every(id=>typeof id==="string"&&id.length<=80)))throw new Error(ui("Tab 或頁碼設定格式無效"));
  if(p.filterCollapsed!=null&&!bounded(p.filterCollapsed,500,item=>typeof item==="boolean"))throw new Error(ui("篩選設定格式無效"));
  if(p.overview!=null&&(!object(p.overview)||!["order","hidden"].every(key=>Array.isArray(p.overview[key])&&p.overview[key].length<=100&&p.overview[key].every(id=>typeof id==="string"&&id.length<=80))))throw new Error(ui("總覽圖表設定格式無效"));
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
    confirmChange("匯入設定?",ui("將套用外觀, 觀察開關, 顯示數量, 圖表, 表格與介面文字")+" · "+Object.keys(p.charts||{}).length+ui(" 個圖表 · ")+Object.keys(p.tables||{}).length+ui(" 個表格"),async()=>{
      const control=$("import-configuration");control.disabled=true;feedback("configuration-message",ui("正在匯入設定"),"pending");
      try{await post("/api/settings",imported.backend);const preferencesText=JSON.stringify(imported.preferences);localStorage.setItem(preferenceKey,preferencesText);feedback("configuration-message",ui("設定已匯入, 正在重新載入"));location.reload();}
      catch(error){feedback("configuration-message",error.message,"error");}finally{control.disabled=false;}
    },ui("確認匯入"));
  }catch(error){feedback("configuration-message",error instanceof SyntaxError?ui("設定內容不是有效的 JSON"):error.message,"error");}
});


const overviewGrid=document.querySelector(".overview-panels"),overviewPanels=new Map([...overviewGrid.children].map(panel=>{const chart=panel.querySelector(".chart,.bar-chart");panel.dataset.tabOrder=chart.id;panel.classList.add("tab-order-row");return [chart.id,panel];})),overviewPriority=["activity-chart","overview-billing","overview-quota-chart","overview-token-stack","overview-error-chart","source-chart","overview-tools-chart","model-chart","overview-file-chart","overview-git-chart","overview-check-chart","overview-skill-chart","overview-sql-chart","overview-cpu-chart","overview-refresh-chart","overview-read-chart","overview-web-chart","overview-dot-chart","environment-chart","trigger-chart","overview-error-time-chart","overview-source-column","overview-model-ring","overview-model-pie","overview-token-chart"],overviewDefault=[...new Set(overviewPriority.filter(id=>overviewPanels.has(id)).concat([...overviewPanels.keys()]))],overviewDefaultVisible=new Set(overviewPriority.slice(0,12)),overviewDefaultHidden=overviewDefault.filter(id=>!overviewDefaultVisible.has(id)),overviewLabel=id=>ui(chartViews.get(id)?.title||overviewPanels.get(id)?.querySelector("h3")?.textContent||id);
let overviewOrder=[...new Set((Array.isArray(preferences.overview?.order)?preferences.overview.order:[]).filter(id=>overviewPanels.has(id)).concat(overviewDefault))],overviewHidden=new Set((Array.isArray(preferences.overview?.hidden)?preferences.overview.hidden:overviewDefaultHidden).filter(id=>overviewPanels.has(id)));
function applyOverview(){for(const id of overviewOrder){const panel=overviewPanels.get(id);panel.hidden=overviewHidden.has(id);overviewGrid.append(panel);}preferences.overview={order:overviewOrder,hidden:[...overviewHidden]};arrangeOverview();}
function moveOverview(id,target,after=false){if(id===target)return;const next=overviewOrder.filter(key=>key!==id),index=next.indexOf(target);if(index<0)return;next.splice(index+(after?1:0),0,id);overviewOrder=next;applyOverview();saveView();renderOverviewSettings();feedback("tab-settings-message",ui("總覽圖表順序已更新"));}
function renderOverviewSettings(){const list=$("overview-layout-rows");list.replaceChildren(...overviewOrder.map(id=>{const row=node("div",null,"setting-row tab-order-row"),handle=row,label=node("label"),check=node("input");row.dataset.tabOrder=id;handle.setAttribute("aria-label",overviewLabel(id)+" "+ui("拖曳調整順序"));handle.title=ui("拖曳調整順序, 或用方向鍵上下移動");check.type="checkbox";check.checked=!overviewHidden.has(id);check.addEventListener("change",()=>{if(check.checked)overviewHidden.delete(id);else overviewHidden.add(id);applyOverview();if(data)renderCharts();saveView();feedback("tab-settings-message",ui("總覽圖表顯示已更新"));});label.append(check,node("span",overviewLabel(id)));row.tabIndex=0;row.append(label);dragTab(handle,id,{root:list,order:()=>overviewOrder,label:overviewLabel,move:moveOverview});return row;}));}
function setupOverview(){for(const [id,panel]of overviewPanels){const heading=panel.querySelector("h3");heading.tabIndex=0;heading.classList.add("direct-drag");heading.setAttribute("aria-label",overviewLabel(id)+" "+ui("拖曳調整順序"));heading.title=ui("拖曳調整順序, 或用方向鍵上下移動");dragTab(heading,id,{root:overviewGrid,order:()=>overviewOrder.filter(key=>!overviewHidden.has(key)),label:overviewLabel,move:moveOverview});}applyOverview();}
$("overview-layout-reset").addEventListener("click",()=>confirmChange("還原總覽圖表?","圖表的顯示項目與順序將回到預設",()=>{overviewOrder=[...overviewDefault];overviewHidden=new Set(overviewDefaultHidden);applyOverview();if(data)renderCharts();saveView();renderOverviewSettings();}));

const defaultTabOrder=[...document.querySelectorAll("[data-tab]")].map(tab=>tab.dataset.tab);
let tabOrder=[...new Set((Array.isArray(preferences.tabOrder)?preferences.tabOrder:[]).filter(id=>defaultTabOrder.includes(id)).concat(defaultTabOrder))];
function applyTabOrder(){const nav=document.querySelector(".tabs");for(const id of tabOrder)nav.append($("tab-"+id));}
function moveTab(id,target,after=false){
  if(id===target)return;const next=tabOrder.filter(key=>key!==id),index=next.indexOf(target);if(index<0)return;next.splice(index+(after?1:0),0,id);tabOrder=next;preferences.tabOrder=tabOrder;applyTabOrder();saveView();feedback("settings-message",ui("Tab 順序已更新"));
}
function dragTab(handle,id,config){
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
$("reset-all-configuration").addEventListener("click",()=>confirmChange("全部設定還原預設?","外觀, 語言, 顯示數量, 排序, 圖表, 表格, 介面文字與觀察設定都會還原",async()=>{const control=$("reset-all-configuration");control.disabled=true;try{const defaults=data.default_settings;if(!defaults)throw new Error(ui("無法取得預設設定, 請重新啟動程式"));await post("/api/settings",{...defaults,replace_customizations:true});localStorage.setItem(preferenceKey,JSON.stringify({settings:defaults}));location.reload();}catch(error){feedback("configuration-message",error.message,"error");}finally{control.disabled=false;}},ui("確認還原")));

$("observation-settings").addEventListener("click",openSettings);
$("language-select").addEventListener("change",()=>{if(!["zh-TW","en","ja"].includes($("language-select").value))return;locale=$("language-select").value;applyLanguage();saveView();if($("settings-dialog").open)openSettings();feedback("settings-message",ui("語言已切換"));});
$("settings-close").addEventListener("click",()=>$("settings-dialog").close());
$("dark-toggle").addEventListener("click",()=>{appearance.mode=document.documentElement.dataset.mode==="dark"?"light":"dark";applyAppearance();saveView();});
for(const el of document.querySelectorAll("[data-mode]"))el.addEventListener("click",()=>{appearance.mode=el.dataset.mode;applyAppearance();saveView();});
for(const [id,key]of [["theme-select","theme"],["accent-select","accent"],["font-size","font"]])$(id).addEventListener("change",()=>{if(key==="font")roundNumber($(id));if(!$(id).checkValidity()){applyAppearance();return;}appearance[key]=key==="font"?Number($(id).value):$(id).value;applyAppearance();saveView();feedback("settings-message",key==="font"?ui("字體大小已套用: ")+appearance.font+" px":ui("外觀已更新"));});
$("font-size").addEventListener("input",()=>{const value=Number($("font-size").value);if(!$("font-size").checkValidity()||!Number.isInteger(value))return;appearance.font=value;applyAppearance();saveView();feedback("settings-message",ui("外觀已更新"));});
$("apply-font").addEventListener("click",()=>{const input=$("font-size");if(!roundNumber(input)||!input.checkValidity()){feedback("settings-message",ui("請輸入有效數值"),"error");return;}appearance.font=Number(input.value);applyAppearance();saveView();feedback("settings-message",ui("字體大小已套用: ")+appearance.font+" px");});
systemDark.addEventListener("change",applyAppearance);
async function applySessionCount(){if(settingsBusy||!data)return;const input=$("session-count");roundNumber(input);if(!input.checkValidity()||!Number.isInteger(Number(input.value))){input.value=data.settings.max_files;feedback("settings-message",ui("session 數量請輸入 1 - 5000 的整數"),"error");return;}input.disabled=true;try{await changeSettings({max_files:Number(input.value)});}catch{input.value=data.settings.max_files;}finally{input.disabled=data.settings.track_all;}}
$("session-count").addEventListener("change",applySessionCount);$("apply-session-count").addEventListener("click",applySessionCount);
$("detail-close").addEventListener("click",()=>$("detail-dialog").close());
$("detail-dialog").addEventListener("close",()=>{detail=null;detailHistory.length=0;const nav=$("detail-content").previousElementSibling;if(nav?.classList.contains("modal-tabs"))nav.remove();$("detail-content").replaceChildren();for(const key of tableViews.keys())if(key.startsWith("detail:"))tableViews.delete(key);updateDetailBack();});
$("detail-back").addEventListener("click",async()=>{const previous=detailHistory.pop();if(!previous)return;if(previous.view.kind==="jev")await openJev(previous.view.call,previous.view.thread,false,previous.scroll);else{openDetail(previous.view,false);$("detail-content").scrollTop=previous.scroll;}updateDetailBack();});
for(const dialog of document.querySelectorAll("dialog"))dialog.addEventListener("click",event=>{if(event.target!==dialog)return;const rect=dialog.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom)dialog.close();});
for(const tab of document.querySelectorAll("[data-tab]")){
  tab.classList.add("tab-order-row");tab.dataset.tabOrder=tab.dataset.tab;dragTab(tab,tab.dataset.tab,{root:document.querySelector(".tabs"),order:()=>tabOrder.filter(key=>!$("tab-"+key).hidden),label:key=>$("tab-"+key).textContent,move:moveTab,horizontal:true});
  tab.addEventListener("click",()=>switchTab(tab.dataset.tab));
  tab.addEventListener("keydown",event=>{if(event.altKey)return;if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const tabs=[...document.querySelectorAll("[data-tab]")].filter(t=>!t.hidden),index=tabs.indexOf(tab),next=event.key==="Home"?0:event.key==="End"?tabs.length-1:(index+(event.key==="ArrowRight"?1:tabs.length-1))%tabs.length;switchTab(tabs[next].dataset.tab);tabs[next].focus();});
}
for(const id of ["filter-type","filter-environment","filter-project","filter-trigger","filter-status","filter-reasoning","page-size"])$(id).addEventListener("change",()=>{page=1;lazyLimit=50;if(data){renderCodex();attachTables();}saveView();});
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
$("window").addEventListener("change",()=>{version++;saveView();refresh();});
for(const id of viewInputs){const value=preferences.inputs?.[id],input=$(id);if(typeof value==="string"&&!['filter-project','filter-git'].includes(id)&&(input.tagName!=="SELECT"||[...input.options].some(option=>option.value===value)))input.value=value;}
page=Number.isInteger(preferences.page)?preferences.page:1;
if(preferences.tab==="jev")preferences.tab="mcp";if(preferences.tab==="logs")preferences.tab="errors";if(["git","checks"].includes(preferences.tab))preferences.tab="workflow";
dragSubTabs($("diagnostic-tabs"),"diagnostics",tab=>tab.id);dragSubTabs($("activity-tabs"),"activity",tab=>tab.id);
bindSubPages("errors","diagnostic-tabs",diagnosticPages,diagnosticSource);bindSubPages("workflow","activity-tabs",activityPages,activitySource);
if([...document.querySelectorAll("[data-tab]")].some(tab=>tab.dataset.tab===preferences.tab))switchTab(preferences.tab);
discoverCopy();applyCopy();applyTabOrder();groupSettings();fillDisplayOptions($("page-size"),preferences.inputs?.["page-size"]||display.table);
for(const id of ["activity-chart","git-time-chart","web-time-chart","file-time-chart","monitor-refresh-chart","monitor-cpu-chart","monitor-read-chart","error-time-chart"])chartControls(id,true);
for(const id of ["top-tools","source-chart","model-chart","environment-chart","trigger-chart","mcp-action-chart","git-chart","skill-chart","check-chart","file-operation-chart","error-source-chart"])chartControls(id);
chartControls("sqlite-operation-chart");chartControls("usage-model-chart");for(const id of ["overview-tools-chart","overview-token-chart","overview-error-chart","overview-file-chart","overview-sql-chart","overview-source-column","overview-model-ring","overview-model-pie","overview-token-stack"])chartControls(id);chartControls("overview-cpu-chart",true);for(const id of ["overview-git-chart","overview-skill-chart","overview-check-chart" ,"overview-dot-chart"])chartControls(id);for(const id of ["overview-web-chart","overview-error-time-chart","overview-refresh-chart","overview-read-chart"])chartControls(id,true);setupOverview();
pageInput($("mcp-page-number"),()=>{mcpPage=Number($("mcp-page-number").value);renderMcp();});
pageInput($("docs-page-number"),()=>{docsPage=Number($("docs-page-number").value);renderToolDocs();});
addTabSettings();
applyAppearance();loadLocales().then(()=>{applyLanguage();schedule(10);refresh();});
if(typeof ResizeObserver==="function"){const overviewObserver=new ResizeObserver(arrangeOverview);for(const panel of document.querySelector(".overview-panels").children)overviewObserver.observe(panel);}
let chartResizeTimer;addEventListener("resize",()=>{clearTimeout(chartResizeTimer);chartResizeTimer=setTimeout(()=>{if(data)renderCharts();},150);});
