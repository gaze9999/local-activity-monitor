"use strict";
const preferenceKey="local-activity-monitor.preferences.v1";
let preferences={};try{preferences=JSON.parse(localStorage.getItem(preferenceKey)||"{}");if(!preferences||typeof preferences!=="object"||Array.isArray(preferences))preferences={};}catch{}
function editableCopy(value){return typeof value==="string"&&value===value.trim()&&value.length>=2&&value.length<=400&&/[A-Za-z\u3400-\u9fff]/.test(value)&&!/^(?:ms|px|bytes|tokens|分鐘|小時|秒鐘|\d+\s*(?:筆|項|秒|分鐘|小時|天))$/i.test(value);}
preferences.copy=Object.fromEntries(Object.entries(preferences.copy||{}).filter(([key,value])=>editableCopy(key)&&typeof value==="string"&&value.length<=400).slice(0,500));
function validDisplay(value){return value&&Array.isArray(value.options)&&value.options.length>=1&&value.options.length<=8&&value.options.every(n=>Number.isInteger(n)&&n>=1&&n<=200)&&new Set(value.options).size===value.options.length&&[...value.options,"all"].includes(value.ranking)&&[...value.options,"all"].includes(value.table);}
let display=validDisplay(preferences.display)?preferences.display:{options:[5,10,20],ranking:10,table:20};
const copyCatalog=new Set(),copyTargets=[];
function ui(value){if(!editableCopy(value))return value;copyCatalog.add(value);return Object.hasOwn(preferences.copy,value)?preferences.copy[value]:value;}
function editableLabels(values){return Object.defineProperties({},Object.fromEntries(Object.entries(values).map(([key,text])=>{ui(text);return [key,{enumerable:true,get:()=>ui(text)}];})));}
function discoverCopy(){
  const walker=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);let text;
  while(text=walker.nextNode()){if(text.parentElement.closest("script,style,[data-copy=ignore]"))continue;const original=text.textContent,base=original.trim();if(!editableCopy(base))continue;const start=original.indexOf(base);copyTargets.push({text,base,before:original.slice(0,start),after:original.slice(start+base.length)});ui(base);if(/^(?:H[1-4]|TH)$/.test(text.parentElement.tagName))text.parentElement.dataset.copyBase=base;}
  for(const el of document.querySelectorAll("[placeholder],[aria-label],[title]"))for(const attribute of ["placeholder","aria-label","title"]){const base=el.getAttribute(attribute);if(!el.closest("[data-copy=ignore]")&&editableCopy(base)){copyTargets.push({el,attribute,base});ui(base);}}
}
function applyCopy(){for(const target of copyTargets)if(target.text)target.text.textContent=target.before+ui(target.base)+target.after;else target.el.setAttribute(target.attribute,ui(target.base));}
const $ = id => document.getElementById(id);
const numberFormat=new Intl.NumberFormat("zh-TW"),dateFormat=new Intl.DateTimeFormat("zh-TW",{year:"numeric",month:"numeric",day:"numeric",hour:"2-digit",minute:"2-digit",second:"2-digit",hour12:false});
const fmt = value => value == null ? ui("未知") : numberFormat.format(value);
function when(value){if(!value)return ui("尚無紀錄");const date=new Date(value);return Number.isFinite(date.getTime())?dateFormat.format(date):ui("未知");}
const labels = {
  status:editableLabels({ok:"完成",success:"完成",ready:"就緒",passed:"通過",failed:"失敗",error:"錯誤",timeout:"逾時",cancelled:"已取消",canceled:"已取消",partial:"部分完成",extracted:"已取得內容",written:"已寫入",preview:"預覽",unchanged:"未變更",fallback:"備用結果",skipped:"略過",dry_run:"試跑",running:"執行中",completed:"已完成",observed:"狀態未提供",pending:"等待中",queued:"排隊中",missing_dependencies:"缺少相依",used:"已執行",not_needed:"無需執行",disabled:"已停用"}),
  type:editableLabels({codex:"Codex",work:"ChatGPT Work",chat:"ChatGPT 對話",unknown:"未知"}),
  environment:editableLabels({local:"本機",cloud:"雲端",remote:"遠端",unknown:"未知"}),
  trigger:editableLabels({user:"手動對話",dot:"Dot",orbit:"Dot",schedule:"排程",automation:"排程",heartbeat:"Heartbeat",subagent:"Subagent",guardian_review:"自動審查",unknown:"未知"})
};
const tokenLabels = editableLabels({input_tokens:"Input",cached_input_tokens:"Cached input",output_tokens:"Output",reasoning_output_tokens:"Reasoning output",total_tokens:"Total"});
const observationLabels = editableLabels({codex:"對話紀錄",jev:"Jev 用量",metadata:"對話名稱與分類",git:"Git 操作",jev_calls:"Jev 送出與回傳內容",skills:"Skills 讀取",checks:"test / build / lint 操作",tool_events:"工具呼叫明細",mcp:"MCP 操作與結果",web:"網路工具與參考網址",files:"檔案讀寫紀錄",errors:"對話與工具錯誤"});
const actionLabels=editableLabels({read:"讀取",write:"寫入",change:"變更",deployment:"部署",operation:"操作"}),fileLabels=editableLabels({added:"新增",modified:"修改",deleted:"刪除",moved:"移動",write:"寫入",read:"讀取"});
const viewInputs=["thread-search","filter-type","filter-environment","filter-project","filter-trigger","filter-status","filter-reasoning","page-size","filter-git","window","filter-mcp-category","filter-mcp-server","filter-mcp-result","file-search","filter-file-operation","filter-file-method","filter-file-project","filter-file-tool"];
let pendingStatus=preferences.inputs?.["filter-status"];
let pendingFileProject=preferences.inputs?.["filter-file-project"],pendingFileTool=preferences.inputs?.["filter-file-tool"];
let loadedRevision=null,pendingRevision=null,preferencesApplied=false,pendingProject=preferences.inputs?.["filter-project"],pendingGit=preferences.inputs?.["filter-git"],pendingReasoning=preferences.inputs?.["filter-reasoning"];
let threadIndex=new Map();
let data = null, page = 1, pageCount = 1, refreshTimer = null, refreshSeconds = 10;
let busy = false, refreshQueued = false, settingsBusy = false, recordingBusy = false, version = 0, detail = null;
let lazyLimit=50,mcpPage=1,docsPage=1,docTools=[],chartSelection=null;
const docDrafts=new Map();
const chartViews=new Map(),systemDark=matchMedia("(prefers-color-scheme: dark)");
let appearance={mode:"dark",theme:"slate",accent:"green",font:14,...preferences.appearance};

function node(tag,text,cls){const el=document.createElement(tag);if(text!=null)el.textContent=text;if(cls)el.className=cls;return el;}
function button(text,action,cls){const el=node("button",text,cls);el.type="button";el.addEventListener("click",action);return el;}
function cell(row,text,cls){const el=node("td",text==null?null:text===ui("未知")||text===ui("尚無紀錄")?"--":text,cls);el.dataset.columnIndex=row.children.length;row.append(el);return el;}
function valueCell(row,value,cls){const el=cell(row,value==null?"--":fmt(value),cls);el.dataset.sortValue=value==null?"":String(value);return el;}
function tag(text,cls=""){return node("span",text===ui("未知")?"--":text,"badge "+cls);}
function clickableRow(row,action,label){row.classList.add("clickable-row");row.title=label||ui("開啟明細");row.addEventListener("click",event=>{if(!event.target.closest("button,a,input,select"))action();});return row;}
function feedback(id,text,state="success"){$(id).textContent=text;$(id).dataset.state=state;}
function pageInput(input,change){
  const apply=()=>{const value=Number(input.value);input.value=Math.max(1,Math.min(Number.isInteger(value)?value:1,Number(input.max)||1));change();};
  input.addEventListener("change",apply);input.addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();apply();}});input.title=ui("輸入頁碼後按 Enter 套用");
}
function syncPageInput(input,value,max){input.max=max;if(document.activeElement!==input)input.value=value;}
let overviewFrame=0;
function arrangeOverview(){
  if(overviewFrame)return;overviewFrame=requestAnimationFrame(()=>{overviewFrame=0;const grid=document.querySelector(".overview-panels"),panels=[...grid.children].filter(panel=>!panel.hidden),heights=panels.map(panel=>panel.getBoundingClientRect().height);if(!heights.some(height=>height>0))return;grid.classList.add("masonry");const style=getComputedStyle(grid),gap=parseFloat(style.rowGap),step=parseFloat(style.gridAutoRows)+gap;for(let i=0;i<panels.length;i++)if(heights[i]>0)panels[i].style.gridRowEnd="span "+Math.ceil((heights[i]+gap)/step);});
}
function cards(id,items){$(id).replaceChildren(...items.map(([label,value,info])=>{const el=node("div",null,"card"),amount=node("div",value,"value"),parts=typeof value==="string"?value.match(/^([\d,.]+)\s+(.+)$/):null;if(parts)amount.replaceChildren(document.createTextNode(parts[1]+" "),node("span",parts[2],"value-unit"));el.append(node("div",label,"label"),amount,node("div",info,"detail"));return el;}));}
function countBy(items,key){const counts=Object.create(null);for(const item of items){const value=item[key]||"unknown";counts[value]=(counts[value]||0)+1;}return counts;}
function sorted(counts){return Object.entries(counts||{}).sort((a,b)=>b[1]-a[1]||a[0].localeCompare(b[0]));}
function saveView(){try{preferences={...preferences,tab:document.querySelector('[data-tab][aria-selected="true"]').dataset.tab,inputs:Object.fromEntries(viewInputs.map(id=>[id,$(id).value])),page,appearance,display,charts:Object.fromEntries([...chartViews].map(([id,view])=>[id,view.settings])),settings:data?.settings||preferences.settings};localStorage.setItem(preferenceKey,JSON.stringify(preferences));}catch{}}
function toolCounts(threads,key="tools"){const counts=Object.create(null);for(const t of threads)for(const [name,count]of Object.entries(t[key]||{}))counts[name]=(counts[name]||0)+count;return counts;}
function title(t){return t.thread_name||ui("未命名對話");}
function svgNode(tag,attrs,text){const el=document.createElementNS("http://www.w3.org/2000/svg",tag);for(const [key,value]of Object.entries(attrs||{}))el.setAttribute(key,value);if(text!=null)el.textContent=text;return el;}
const chartTip=node("div",null,"chart-tooltip");chartTip.hidden=true;chartTip.id="chart-tooltip";chartTip.setAttribute("role","tooltip");document.body.append(chartTip);
function hideChartTip(){chartTip.hidden=true;}
function showChartTip(heading,value,event,target){
  chartTip.replaceChildren(node("div",heading),node("strong",value));chartTip.hidden=false;
  const rect=target.getBoundingClientRect(),x=event?.clientX??rect.left+rect.width/2,y=event?.clientY??rect.top;
  chartTip.style.left=Math.max(8,Math.min(x+14,innerWidth-chartTip.offsetWidth-8))+"px";
  chartTip.style.top=Math.max(8,Math.min(y+14,innerHeight-chartTip.offsetHeight-8))+"px";
}
addEventListener("scroll",hideChartTip,true);addEventListener("blur",hideChartTip);
function bounds(id,items=[]){const s=chartViews.get(id)?.settings||{},end=s.range==="custom"?new Date(s.end).getTime():Date.now(),start=s.range==="all"?Math.min(...items.map(e=>new Date(e.timestamp||e.time||e.updated_at).getTime()).filter(Number.isFinite),end-86400000):s.range==="custom"?new Date(s.start).getTime():end-s.length*s.unit;return {start,end};}
function ranged(id,items){const {start,end}=bounds(id,items);return items.filter(item=>{const time=new Date(item.timestamp||item.time||item.updated_at).getTime();return Number.isFinite(time)&&time>=start&&time<=end;});}
function niceMax(value){if(value<=4)return Math.max(4,Math.ceil(value));const power=10**Math.floor(Math.log10(value/4)),step=Math.ceil(value/4/power)*power;return step*4;}
function bars(id,counts,names,onClick,unit=ui("次")){
  const s=chartViews.get(id)?.settings||{},entries=sorted(counts).slice(0,s.top===0?undefined:s.top||Number(display.ranking)||10),peak=Math.max(...entries.map(item=>item[1]),1),max=s.maximum||peak;
  if(s.shape==="donut"){shareChart(id,counts,entries,names,onClick,unit,s.shape);return;}
  $(id).replaceChildren(...entries.map(([key,value])=>{
    const row=node("div",null,"bar-row"),label=names?.[key]||key;
    row.append(onClick?button(label,()=>onClick(key),"bar-label link"):node("span",label,"bar-label"),node("b",fmt(value)));
    const svg=svgNode("svg",{viewBox:"0 0 640 10",preserveAspectRatio:"none",role:"img","aria-label":label+" "+fmt(value)+" "+unit});
    svg.append(svgNode("rect",{width:640,height:10,rx:5,class:"bar-track"}),svgNode("rect",{width:640*Math.min(value,max)/max,height:10,rx:5,class:"bar-fill"}));
    const text=fmt(value)+" "+unit;svg.append(svgNode("title",{},label+": "+text));row.tabIndex=onClick?-1:0;row.setAttribute("aria-label",label+": "+text);
    for(const event of ["pointerenter","pointermove","focusin"])row.addEventListener(event,e=>showChartTip(label,text,e.type==="focusin"?null:e,row));
    for(const event of ["pointerleave","focusout"])row.addEventListener(event,hideChartTip);
    row.append(svg);return row;
  }));
  $(id).append(node("p",unit+(peak>max?ui(" · 超過上限的長條已截短"):""),"chart-unit"));
  if(!entries.length)$(id).append(node("p",ui("尚無資料"),"empty"));
}
function shareChart(id,counts,visible,names,onClick,unit,shape){
  const entries=visible.map(([key,value])=>({key,value,label:names?.[key]||key})),total=Object.values(counts).reduce((sum,value)=>sum+value,0),remaining=total-entries.reduce((sum,item)=>sum+item.value,0);
  if(remaining>0)entries.push({key:null,value:remaining,label:ui("其他項目")});
  const root=$(id);root.replaceChildren();if(!total){root.append(node("p",ui("尚無資料"),"empty"));return;}
  const layout=node("div",null,"pie-layout"),svg=svgNode("svg",{viewBox:"0 0 240 240",role:"img","aria-label":ui("分布圖"),class:"pie-figure"}),legend=node("div",null,"pie-legend");let angle=-Math.PI/2;
  for(const [index,item]of entries.entries()){
    const share=item.value/total,end=angle+share*Math.PI*2,x1=120+96*Math.cos(angle),y1=120+96*Math.sin(angle),x2=120+96*Math.cos(end),y2=120+96*Math.sin(end),color="palette-"+index%8;
    const segment=share===1?svgNode("circle",{cx:120,cy:120,r:96,class:"pie-segment "+color}):svgNode("path",{d:`M 120 120 L ${x1} ${y1} A 96 96 0 ${share>.5?1:0} 1 ${x2} ${y2} Z`,class:"pie-segment "+color});
    const info=fmt(item.value)+" "+unit+" · "+fmt(Math.round(share*1000)/10)+"%";segment.append(svgNode("title",{},item.label+": "+info));segment.tabIndex=0;segment.setAttribute("aria-label",item.label+": "+info);segment.setAttribute("role",onClick&&item.key?"button":"img");
    for(const event of ["pointerenter","pointermove","focusin"])segment.addEventListener(event,e=>showChartTip(item.label,info,e.type==="focusin"?null:e,segment));for(const event of ["pointerleave","focusout"])segment.addEventListener(event,hideChartTip);
    if(onClick&&item.key){segment.addEventListener("click",()=>onClick(item.key));segment.addEventListener("keydown",event=>{if(["Enter"," "].includes(event.key)){event.preventDefault();onClick(item.key);}});}
    svg.append(segment);angle=end;const row=node("div",null,"pie-legend-row"),swatch=node("span",null,"pie-swatch "+color);row.append(swatch,onClick&&item.key?button(item.label,()=>onClick(item.key),"link"):node("span",item.label),node("b",info));legend.append(row);
  }
  if(shape==="donut"){svg.append(svgNode("circle",{cx:120,cy:120,r:58,class:"pie-hole","pointer-events":"none"}),svgNode("text",{x:120,y:118,"text-anchor":"middle",class:"pie-total","pointer-events":"none"},fmt(total)),svgNode("text",{x:120,y:144,"text-anchor":"middle",class:"pie-unit","pointer-events":"none"},unit));}
  layout.append(svg,legend);root.append(layout,node("p",ui("合計 ")+fmt(total)+" "+unit,"chart-unit"));
}
function timeline(id,noteId,series,unit=ui("次"),average=false){
  const svg=$(id);svg.replaceChildren();svg.onpointermove=svg.onpointerleave=svg.onkeydown=svg.onfocus=svg.onblur=null;
  if(!series?.length){$(noteId).textContent=ui("尚無活動紀錄");return;}
  const s=chartViews.get(id)?.settings||{},range=bounds(id,series),end=range.end;
  let interval=Math.max(id==="chart"?3600000:60000,s.interval||3600000);while((end-range.start)/interval>240)interval*=2;
  const start=Math.floor(range.start/interval)*interval,slots=Math.max(1,Math.ceil((end-start)/interval)),values=Array.from({length:slots},(_,i)=>({time:start+interval*i,calls:0,samples:0}));
  for(const item of series){const time=new Date(item.time).getTime(),index=Math.floor((time-start)/interval);if(time>=range.start&&time<=end&&index>=0&&index<slots){values[index].calls+=item.calls;values[index].samples++;}}
  if(average)for(const item of values)item.calls=item.samples?Math.round(item.calls/item.samples*100)/100:null;
  const peak=Math.max(...values.map(item=>item.calls||0),1),max=s.maximum||niceMax(peak),scale=740/(svg.clientWidth||740),labelSize=Math.max(11,parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--font-size"))*.75||11),left=Math.min(300,Math.max(58,(fmt(max).length*labelSize*.66+12)*scale)),plotWidth=710-left,width=plotWidth/slots;
  svg.style.setProperty("--chart-label-size",labelSize*scale+"px");
  for(let i=0;i<=4;i++){const y=190-i*38;svg.append(svgNode("line",{x1:left,x2:710,y1:y,y2:y,class:"grid-line"}),svgNode("text",{x:left-10,y:y+4,"text-anchor":"end"},fmt(max*i/4)));}
  svg.append(svgNode("text",{x:left,y:20},unit));
  const points=[];values.forEach((item,i)=>{if(item.calls===null){points.push(null);return;}const height=Math.min(item.calls,max)/max*152,x=left+i*width;points.push((x+width/2)+","+(190-height));if(s.shape!=="line"){const rect=svgNode("rect",{x,y:190-height,width:Math.max(.5,width-1),height,rx:2});rect.append(svgNode("title",{},when(new Date(item.time).toISOString())+": "+(item.calls===null?ui("尚無資料"):fmt(item.calls)+" "+unit)));svg.append(rect);}});
  if(s.shape==="line"){let segment=[];const draw=()=>{if(segment.length>1)svg.append(svgNode("polyline",{points:segment.join(" "),class:"trend-line",fill:"none"}));else if(segment.length){const [cx,cy]=segment[0].split(",");svg.append(svgNode("circle",{cx,cy,r:3,class:"trend-dot"}));}segment=[];};for(const point of points)if(point===null)draw();else segment.push(point);draw();}
  const cursor=svgNode("g",{class:"chart-cursor",visibility:"hidden","pointer-events":"none"}),guide=svgNode("line",{y1:38,y2:190}),dot=svgNode("circle",{r:5});cursor.append(guide,dot);svg.append(cursor);svg.tabIndex=0;
  let selected=0;
  function showPoint(index,event){
    selected=Math.max(0,Math.min(index,values.length-1));const item=values[selected],x=left+(selected+.5)*width,y=190-Math.min(item.calls,max)/max*152;
    guide.setAttribute("x1",x);guide.setAttribute("x2",x);dot.setAttribute("cx",x);dot.setAttribute("cy",y);dot.setAttribute("visibility",item.calls===null?"hidden":"visible");cursor.setAttribute("visibility","visible");
    const heading=when(new Date(Math.max(item.time,range.start)).toISOString())+" - "+when(new Date(Math.min(item.time+interval,end)).toISOString()),text=(item.calls===null?ui("尚無資料"):fmt(item.calls)+" "+unit);showChartTip(heading,text,event,svg);
    svg.setAttribute("aria-label",ui(chartViews.get(id)?.title||"活動趨勢")+" · "+heading+": "+text);
  }
  svg.onpointermove=event=>{const matrix=svg.getScreenCTM();if(!matrix)return;const point=svg.createSVGPoint();point.x=event.clientX;point.y=event.clientY;const local=point.matrixTransform(matrix.inverse());if(local.x<left||local.x>710||local.y<38||local.y>190){cursor.setAttribute("visibility","hidden");hideChartTip();return;}showPoint(Math.floor((local.x-left)/width),event);};
  svg.onfocus=()=>showPoint(selected);svg.onkeydown=event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();showPoint(event.key==="Home"?0:event.key==="End"?values.length-1:selected+(event.key==="ArrowRight"?1:-1));};
  svg.onpointerleave=svg.onblur=()=>{cursor.setAttribute("visibility","hidden");hideChartTip();};
  const ticks=Math.max(1,Math.min(4,Math.floor(plotWidth/scale/(labelSize*10))));for(let i=0;i<=ticks;i++){const time=start+(end-start)*i/ticks,x=left+plotWidth*i/ticks,label=new Date(time).toLocaleString("zh-TW",{month:"2-digit",day:"2-digit",hour:"2-digit",minute:"2-digit",hour12:false});svg.append(svgNode("line",{x1:x,x2:x,y1:190,y2:196}),svgNode("text",{x,y:218,"text-anchor":i===0?"start":i===ticks?"end":"middle"},label));}
  $(noteId).textContent=ui("每格 ")+fmt(interval/60000)+ui(" 分鐘 · 最高 ")+fmt(Math.max(...values.map(v=>v.calls||0),0))+" "+unit+(average?ui(" · 每格平均"):"")+(peak>max?ui(" · 超過上限的數值已截短"):"");
}
function hourly(events){const counts=Object.create(null);for(const event of events)if(event.timestamp){const key=event.timestamp.slice(0,13)+":00:00Z";counts[key]=(counts[key]||0)+1;}return Object.entries(counts).sort().map(([time,calls])=>({time,calls}));}
function chartControls(id,trend=false){
  const controls=node("div",null,"chart-options"),form=node("div",null,"chart-controls"),saved=preferences.charts?.[id]||{};
  const localDate=time=>new Date(time-new Date(time).getTimezoneOffset()*60000).toISOString().slice(0,16);
  const settings={range:trend?"recent":"all",length:id.startsWith("monitor-")?60:24,unit:id.startsWith("monitor-")?60000:3600000,interval:id.startsWith("monitor-")?60000:3600000,top:display.ranking==="all"?0:display.ranking,maximum:0,shape:"bar",start:localDate(Date.now()-86400000),end:localDate(Date.now())};
  const fields={};
  function copyNode(tag,text){const el=node(tag,ui(text));if(editableCopy(text))copyTargets.push({text:el.firstChild,base:text,before:"",after:""});return el;}
  function select(key,label,items){const el=node("select");for(const [value,text]of items){const option=copyNode("option",text);option.value=value;el.append(option);}fields[key]=el;const wrap=copyNode("label",label);wrap.append(el);form.append(wrap);}
  function input(key,label,type,min,max){const el=node("input");el.type=type;if(min!=null)el.min=min;if(max!=null)el.max=max;fields[key]=el;const wrap=copyNode("label",label);wrap.append(el);form.append(wrap);}
  select("range","時間範圍",[["all","全部紀錄"],["recent","最近一段時間"],["custom","指定起訖"]]);
  input("length","最近","number",1,365);select("unit","時間單位",[[60000,"分鐘"],[3600000,"小時"],[86400000,"天"]]);
  input("start","開始時間","datetime-local");input("end","結束時間","datetime-local");
  if(trend)select("interval","時間間隔",[[60000,"1 分鐘"],[300000,"5 分鐘"],[900000,"15 分鐘"],[3600000,"1 小時"],[21600000,"6 小時"],[86400000,"1 天"]]);
  else select("top","顯示項目",[...display.options.map(value=>[value,"前 "+value+" 項"]),[0,"全部"]]);
  const types=trend?[["bar","長條圖"],["line","折線圖"]]:[["bar","長條圖"],["donut","環圈圖"]],shape=saved.shape==="pie"?"donut":saved.shape;
  if(types.some(([key])=>key===shape))settings.shape=shape;
  input("maximum",trend?"Y 軸上限 (0 = 自動)":"數值上限 (0 = 自動)","number",0,1000000000);
  for(const [key,el]of Object.entries(fields)){const value=saved[key]??settings[key];if(el.tagName==="SELECT"?[...el.options].some(o=>o.value===String(value)):el.type==="number"?Number.isFinite(Number(value))&&Number(value)>=Number(el.min)&&Number(value)<=Number(el.max):typeof value==="string")settings[key]=["length","unit","interval","top","maximum"].includes(key)?Number(value):value;el.value=settings[key];}
  function showFields(){fields.maximum.parentElement.hidden=!trend&&settings.shape!=="bar";for(const key of ["length","unit"])fields[key].parentElement.hidden=settings.range!=="recent";for(const key of ["start","end"])fields[key].parentElement.hidden=settings.range!=="custom";}
  form.addEventListener("change",event=>{const key=Object.entries(fields).find(([,el])=>el===event.target)?.[0];if(!key)return;const next={...settings,[key]:["length","unit","interval","top","maximum"].includes(key)?Number(event.target.value):event.target.value};if(!event.target.checkValidity()||next.range==="custom"&&(!next.start||!next.end||new Date(next.start)>=new Date(next.end))){event.target.setAttribute("aria-invalid","true");feedback("chart-settings-message",ui("請檢查時間起訖或數值"),"error");return;}event.target.removeAttribute("aria-invalid");Object.assign(settings,next);showFields();saveView();if(data)renderCharts();feedback("chart-settings-message",ui("圖表已更新"));});
  showFields();controls.append(form);controls.hidden=true;const heading=$(id).previousElementSibling,head=node("div",null,"panel-head chart-head"),actions=node("div",null,"chart-actions"),open=button("⚙",()=>openChartSettings(id),"chart-setting-button"),cycle=button("",()=>{settings.shape=types[(types.findIndex(([key])=>key===settings.shape)+1)%types.length][0];showFields();saveView();renderCharts();},"chart-type-button");
  function updateType(){const index=types.findIndex(([key])=>key===settings.shape),label=ui(types[index][1]),next=ui(types[(index+1)%types.length][1]);cycle.textContent=label+" ↻";cycle.setAttribute("aria-label",ui(heading.dataset.copyBase||heading.textContent)+ui(" · 切換圖表類型, 目前 ")+label);cycle.title=ui("下一個: ")+next;}
  updateType();cycle.dataset.chartType=id;open.setAttribute("aria-label",heading.textContent+ui(" 圖表設定"));heading.replaceWith(head);actions.append(cycle,open);head.append(heading,actions);$(id).before(controls);chartViews.set(id,{settings,fields,controls,title:heading.dataset.copyBase||heading.textContent,updateType});
}
function openChartSettings(id){$("chart-settings-message").textContent="";chartSelection=id;const view=chartViews.get(id);$("chart-settings-title").textContent=ui(view.title)+ui(" · 圖表設定");view.controls.hidden=false;$("chart-settings-content").replaceChildren(view.controls);if(!$("chart-dialog").open)$("chart-dialog").showModal();}
$("chart-settings-close").addEventListener("click",()=>$("chart-dialog").close());$("chart-dialog").addEventListener("close",()=>{if(chartSelection){const view=chartViews.get(chartSelection);view.controls.hidden=true;$(chartSelection).before(view.controls);chartSelection=null;}});
function renderCharts(){
  if(!data)return;for(const view of chartViews.values())view.updateType();const c=data.codex,threads=c.threads||[],events=data.mcp?.events||[];
  timeline("activity-chart","activity-chart-note",c.activity_series||[]);
  bars("top-tools",ranged("top-tools",c.tool_series||[]).reduce((out,e)=>(out[e.tool]=(out[e.tool]||0)+e.calls,out),Object.create(null)),null,tool=>openDetail({kind:"tool",tool}));
  bars("source-chart",countBy(ranged("source-chart",events),"server"),null,server=>{if(server==="web"){switchTab("web");return;}switchTab("mcp");$("filter-mcp-server").value=server;renderMcp();});
  bars("model-chart",countBy(ranged("model-chart",threads),"model"),{unknown:ui("未知")},null,ui("對話數"));
  bars("environment-chart",countBy(ranged("environment-chart",threads),"environment"),labels.environment,null,ui("對話數"));
  bars("trigger-chart",countBy(ranged("trigger-chart",threads),"trigger"),labels.trigger,null,ui("對話數"));
  bars("mcp-action-chart",countBy(ranged("mcp-action-chart",events.filter(e=>e.server!=="web")),"action"),actionLabels);
  timeline("chart","chart-note",data.jev.series||[]);
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
  for(const view of tableViews.values())adaptTable(view);arrangeOverview();
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
  $("theme-select").value=appearance.theme;$("accent-select").value=appearance.accent;$("font-size").value=appearance.font;if(data)for(const view of tableViews.values())adaptTable(view);
}
function switchTab(name){
  for(const tab of document.querySelectorAll("[data-tab]")){const active=tab.dataset.tab===name;tab.setAttribute("aria-selected",String(active));tab.tabIndex=active?0:-1;$("view-"+tab.dataset.tab).hidden=!active;}
  if(data){renderCharts();saveView();}
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
function eventThreadCell(row,event){const t=threadIndex.get(event.thread_id)||event;cell(row).append(threadLink(t));}
function updateProjects(){
  const select=$("filter-project"),selected=select.value,projects=new Map();
  for(const t of data.codex.threads||[])if(t.project_id)projects.set(t.project_id,t.project_name||t.project_id);
  const base=[["all",ui("全部專案")],["project",ui("專案內")],["none",ui("無專案")],["unknown",ui("未知")]];
  select.replaceChildren(...base.concat([...projects].map(([id,name])=>["id:"+id,name])).map(([value,label])=>{const option=node("option",label);option.value=value;return option;}));
  if([...select.options].some(option=>option.value===selected))select.value=selected;
  if(pendingProject){if([...select.options].some(option=>option.value===pendingProject))select.value=pendingProject;pendingProject=null;}
}
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
    identity.append(threadLink(t));
    cell(row,t.thread_id,"mono thread-id");cell(row,t.project_name||t.project_id||(t.project_scope==="none"?ui("無專案"):"--"));
    for(const value of [labels.type[t.activity_type],labels.environment[t.environment],labels.trigger[t.trigger]])cell(row).append(tag(value||"--"));
    cell(row,t.model||"--","mono");cell(row).append(tag(labels.status[t.status]||t.status||"--",t.status));
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
  if(!Object.keys(tools).length)$("tools").append(node("p",ui("尚無工具紀錄"),"empty"));
  $("nested-tools").replaceChildren(...sorted(nested).map(([tool,count])=>{const el=toolButton(tool,()=>openDetail({kind:"nested-tool",tool}),count);return el;}));
  if(!Object.keys(nested).length)$("nested-tools").append(node("p",ui("尚無內層工具紀錄"),"empty"));
  attachTables();
}
function renderMcp(){
  const source=data.mcp||{servers:[],events:[],categories:{}},m={...source,servers:source.servers.filter(s=>s.server!=="web"),events:source.events.filter(e=>e.server!=="web")},servers=m.servers.filter(s=>s.enabled);
  $("mcp-scope").textContent=fmt(m.servers.length)+ui(" 個來源 · ")+fmt(m.events.length)+ui(" 筆操作紀錄");
  cards("mcp-cards",[[ui("MCP 呼叫"),fmt(servers.reduce((n,s)=>n+s.calls,0)),ui("來源 ")+fmt(servers.length)+ui(" 個")],[ui("exec 辨識"),fmt(servers.reduce((n,s)=>n+s.recognized,0)),ui("程式碼中的呼叫位置")],[ui("已提供結果"),fmt(servers.reduce((n,s)=>n+s.known_status,0)),ui("有結果狀態的紀錄")],[ui("錯誤"),fmt(servers.reduce((n,s)=>n+s.errors,0)),ui("包含失敗與逾時")]]);
  $("mcp-panel").hidden=!m.servers.length;$("mcp-chart-panel").hidden=!m.servers.length;
  selectOptions("filter-mcp-category",[["all",ui("全部分類")],...[...new Set(m.servers.map(s=>s.category).concat(m.events.map(e=>e.category)))].map(key=>[key,ui(m.categories[key])||key])]);
  selectOptions("filter-mcp-server",[["all",ui("全部來源")],...m.servers.map(s=>[s.server,s.server])]);
  $("mcp-source-cards").replaceChildren(...m.servers.map(s=>{const el=button("",()=>{$("filter-mcp-server").value=s.server;mcpPage=1;renderMcp();},"source-card"),tags=node("div",null,"tag-line");tags.append(tag(ui(m.categories[s.category])||s.category),tag(s.enabled?s.origin==="configured"?ui("已設定"):ui("有使用紀錄"):ui("已關閉")));el.append(node("div",s.server,"source-name mono"),tags,node("p",fmt(s.calls)+ui(" 次呼叫 · ")+fmt(s.recognized)+ui(" 處 exec 辨識")),node("p",ui("錯誤 ")+fmt(s.errors)+ui(" · 平均 ")+fmt(s.average_ms)+" ms · P95 "+fmt(s.p95_ms)+" ms","muted"));return el;}));
  const filtered=m.events.filter(e=>($("filter-mcp-category").value==="all"||e.category===$("filter-mcp-category").value)&&($("filter-mcp-server").value==="all"||e.server===$("filter-mcp-server").value)&&($("filter-mcp-result").value==="all"||$("filter-mcp-result").value==="error"&&["error","failed","timeout","cancelled","canceled","missing_dependencies"].includes(e.result?.status)||$("filter-mcp-result").value==="known"&&e.result?.status||$("filter-mcp-result").value==="unknown"&&!e.result?.status));
  const rows=sortRecords("mcp-rows",filtered,[e=>e.timestamp,e=>e.thread_name||e.thread_id,eventProject,e=>e.server,e=>data.mcp.categories[e.category]||e.category,e=>e.tool,e=>actionLabels[e.action],e=>e.nested?ui("exec 辨識"):ui("工具呼叫"),e=>e.result?.status,e=>e.duration_ms],e=>e.timestamp),size=tableSize("mcp-rows"),pages=Math.max(1,Math.ceil(rows.length/size));mcpPage=Math.max(1,Math.min(mcpPage,pages));
  replaceRows("mcp-rows",...rows.slice((mcpPage-1)*size,mcpPage*size).map(e=>{const row=node("tr");cell(row,when(e.timestamp));eventThreadCell(row,e);projectCell(row,e);cell(row,e.server,"mono");cell(row).append(tag(ui(m.categories[e.category])||e.category));cell(row,null,"operation-cell").append(button(e.tool,()=>openDetail({kind:"mcp",event:e}),"link mono"));cell(row).append(tag(actionLabels[e.action]||e.action));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));cell(row).append(tag(labels.status[e.result?.status]||e.result?.status||"--",e.result?.status||""));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"mcp",event:e}));}));
  $("mcp-empty").textContent=rows.length?"":ui("沒有符合條件的操作");$("mcp-page-summary").textContent=ui("第 ")+mcpPage+" / "+pages+ui(" 頁 · ")+fmt(rows.length)+ui(" 筆");$("mcp-prev").disabled=mcpPage===1;$("mcp-next").disabled=mcpPage===pages;syncPageInput($("mcp-page-number"),mcpPage,pages);
  attachTables();

}
function renderWeb(){
  const events=(data.mcp?.events||[]).filter(e=>e.server==="web"),refs=references(events),urls=new Set(refs.map(e=>e.url));
  $("web-scope").textContent=ui("查看網路工具操作與參考頁面");
  cards("web-cards",[[ui("網路工具呼叫"),fmt(events.filter(e=>!e.nested).length),ui("exec 辨識 ")+fmt(events.filter(e=>e.nested).length)+ui(" 處")],[ui("參考網址"),fmt(urls.size),ui("不同參考頁面")],[ui("涉及對話"),fmt(new Set(events.map(e=>e.thread_id)).size),ui("有網路工具紀錄的對話")],[ui("已提供結果"),fmt(events.filter(e=>e.result?.status).length),ui("錯誤 ")+fmt(events.filter(e=>["error","failed","timeout"].includes(e.result?.status)).length)+ui(" 次")]]);
  replaceRows("web-event-rows",...events.map(e=>{const row=node("tr"),links=references([e]);cell(row,when(e.timestamp));eventThreadCell(row,e);projectCell(row,e);cell(row).append(toolButton("web__run",()=>openDetail({kind:"mcp",event:e})));cell(row).append(tag(labels.status[e.result?.status]||e.result?.status||"--"));const count=valueCell(row,links.length);count.replaceChildren(button(fmt(links.length),()=>openDetail({kind:"mcp",event:e}),"count-button"));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"mcp",event:e}));}));
  $("web-event-empty").textContent=events.length?"":ui("尚無網路工具紀錄");
}
function fileLocation(event){return event.workdir&&!/^(?:[A-Za-z]:[\\/]|[\\/])/.test(event.path)?event.workdir.replace(/[\\/]$/u,"")+(event.workdir.includes("\\")?"\\":"/")+event.path:event.path;}
function fileProjectKey(event){const thread=threadIndex.get(event.thread_id)||{};return thread.project_id?"project:"+thread.project_id:thread.project_name?"name:"+thread.project_name:thread.project_scope==="none"?"none":"unknown";}
function filteredFiles(){const query=$("file-search").value.trim().toLowerCase(),operation=$("filter-file-operation").value,method=$("filter-file-method").value,project=$("filter-file-project").value,tool=$("filter-file-tool").value;return (data.codex.file_activity?.events||[]).filter(e=>(!query||fileLocation(e).toLowerCase().includes(query))&&(operation==="all"||operation===e.operation)&&(method==="all"||e.nested===(method==="nested"))&&(project==="all"||fileProjectKey(e)===project)&&(tool==="all"||e.tool===tool));}
function fileRows(events,includeThread=true){return events.map(e=>{const row=node("tr");cell(row,when(e.timestamp));if(includeThread){eventThreadCell(row,e);projectCell(row,e);}cell(row).append(tag(fileLabels[e.operation]||e.operation));cell(row,fileLocation(e),"mono path-cell");const tool=cell(row,null,"operation-cell");tool.append(toolButton(e.tool||"apply_patch",()=>openDetail({kind:e.nested?"nested-tool":"tool",tool:e.tool||"apply_patch",ids:[e.thread_id]})));cell(row).append(tag(e.nested?ui("exec 辨識"):ui("工具呼叫")));cell(row,when(e.completed_at));valueCell(row,e.duration_ms);return clickableRow(row,()=>openDetail({kind:"file",event:e}));});}
function renderFiles(){
  const records=data.codex.file_activity||{events:[],total:0},projects=new Map();for(const event of records.events){const key=fileProjectKey(event);if(!["none","unknown"].includes(key))projects.set(key,eventProject(event)||key);}
  selectOptions("filter-file-project",[["all",ui("全部專案")],["none",ui("無專案")],["unknown",ui("未知")],...[...projects].sort((a,b)=>a[1].localeCompare(b[1]))]);selectOptions("filter-file-tool",[["all",ui("全部工具")],...[...new Set(records.events.map(event=>event.tool).filter(Boolean))].sort().map(tool=>[tool,tool])]);
  for(const [id,value]of [["filter-file-project",pendingFileProject],["filter-file-tool",pendingFileTool]])if(value&&[...$(id).options].some(option=>option.value===value))$(id).value=value;pendingFileProject=pendingFileTool=null;
  const events=filteredFiles();$("files-scope").textContent=ui("從工具參數辨識檔案位置 · 載入 ")+fmt(records.events.length)+" / "+fmt(records.total)+ui(" 筆");
  cards("file-cards",[[ui("符合篩選的紀錄"),fmt(events.length),ui("全部 ")+fmt(records.total)+ui(" 筆")],[ui("讀取"),fmt(events.filter(e=>e.operation==="read").length),ui("有指定檔案位置")],[ui("寫入與變更"),fmt(events.filter(e=>e.operation!=="read").length),ui("新增 / 修改 / 刪除 / 移動")],[ui("不同檔案位置"),fmt(new Set(events.map(fileLocation)).size),ui("目前篩選結果")]]);
  replaceRows("file-rows",...fileRows(events));$("file-empty").textContent=events.length?"":ui("尚無符合條件的檔案操作");
}
function renderJev(){
  const j=data.jev,s=j.summary||{};
  $("jev-scope").textContent=j.health==="paused"?ui("統計觀察已關閉"):j.health==="unavailable"?ui("資料暫時無法讀取"):ui("紀錄開始 ")+when(j.enabled_at);
  if(!recordingBusy){$("jev-recording").checked=!!j.enabled;$("jev-recording").disabled=false;$("jev-recording-state").textContent=j.enabled?ui("已啟用"):ui("已停用");}
  cards("jev-cards",[[ui("操作次數"),fmt(s.calls||0),"Rank / Evaluate / Doctor"],[ui("HTTP 嘗試"),fmt(s.http_attempts||0),ui("重試 ")+fmt(s.retries||0)+ui(" 次")],["Input tokens",fmt(s.input_tokens),ui("未知 ")+fmt(s.input_unknown_calls||0)+ui(" 次")],["Output tokens",fmt(s.output_tokens),ui("未知 ")+fmt(s.output_unknown_calls||0)+ui(" 次")]]);
  $("statuses").replaceChildren(...["ok","fallback","skipped","dry_run"].map(status=>{const row=node("div",null,"status-row");row.append(node("span",labels.status[status],"badge "+status),node("b",fmt(s.statuses?.[status]||0)));return row;}));
  $("bytes").textContent=ui("平均耗時 ")+fmt(s.average_latency_ms||0)+" ms · Request "+fmt(s.request_body_bytes||0)+" bytes · Response "+fmt(s.known_response_bytes||0)+" bytes";
  replaceRows("jev-rows",...(j.recent||[]).map(event=>{const row=node("tr");cell(row,when(event.timestamp));const operation=cell(row);operation.append(node("span",event.operation));cell(row).append(tag(event.source));cell(row,event.model||"--","mono");cell(row).append(tag(labels.status[event.status]||event.status,event.status));valueCell(row,event.input_tokens);valueCell(row,event.output_tokens);valueCell(row,event.latency_ms);valueCell(row,event.http_attempts);cell(row,event.attempts?.map(a=>a.http_status||a.status).join(" → ")||"--");return row;}));
  $("jev-empty").textContent=j.recent?.length?"":j.enabled?ui("尚無 Jev 操作紀錄"):ui("可使用上方開關啟用 Jev 紀錄");
  const calls=(data.codex.threads||[]).flatMap(t=>(t.jev_calls||[]).map(call=>({...call,thread_name:t.thread_name}))).sort((a,b)=>(b.timestamp||"").localeCompare(a.timestamp||""));
  replaceRows("jev-call-rows",...calls.slice(0,200).map(call=>{const row=node("tr");cell(row,when(call.timestamp));eventThreadCell(row,call);cell(row,call.operation);cell(row).append(button(ui("送出 / 回傳"),()=>openJev(call),"link"));return row;}));
  $("jev-call-empty").textContent=calls.length?"":ui("尚無對話中的 Jev 呼叫紀錄");
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
  $("git-empty").textContent=events.length?"":ui("尚無符合條件的 Git 操作");
}
function renderWorkflow(){
  const skills=data.codex.skills||{events:[],counts:{}},checks=data.codex.checks||[];
  replaceRows("skill-rows",...skills.events.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.skill,()=>openDetail({kind:"skill",skill:event.skill}),"link"));return row;}));
  $("skill-empty").textContent=skills.events.length?"":ui("尚無 Skills 讀取紀錄");
  cards("check-cards",[[ui("驗證操作"),fmt(checks.length),ui("目前載入紀錄")],[ui("操作種類"),fmt(new Set(checks.map(e=>e.operation)).size),ui("test / build / lint")],[ui("涉及對話"),fmt(new Set(checks.map(e=>e.thread_id)).size),ui("有驗證操作的對話")],[ui("工具已回傳"),fmt(checks.filter(e=>e.completed_at).length),ui("有回傳時間的紀錄")]]);
  replaceRows("check-rows",...checks.map(event=>{const row=node("tr");cell(row,when(event.timestamp));eventThreadCell(row,event);cell(row).append(button(event.operation,()=>openDetail({kind:"operation",event}),"link mono"));cell(row,when(event.completed_at));cell(row,fmt(event.duration_ms));return row;}));
  $("check-empty").textContent=checks.length?"":ui("尚無驗證操作紀錄");
}
function render(next){
  data=next;threadIndex=new Map((data.codex.threads||[]).map(thread=>[thread.thread_id,thread]));const c=data.codex,j=data.jev,threads=c.threads||[];
  document.documentElement.dataset.revision=data.revision||"";
  $("live").textContent=ui(data.monitor?.health==="error"?data.updated_at?"資料整理失敗, 顯示上次結果":"資料整理失敗, 等待重試":data.updated_at?"本機連線正常":"正在整理資料");
  $("overview-scope").textContent=ui("觀察開始 ")+when(c.started_at||data.started_at);
  const duration=threads.filter(t=>t.task_duration_ms!=null).reduce((n,t)=>n+t.task_duration_ms,0);
  cards("overview-cards",[[ui("對話"),fmt(threads.length),ui("執行中 ")+fmt(threads.filter(t=>t.status==="running").length)+ui(" 個")],[ui("工具呼叫"),fmt(c.observed_tool_calls||0),ui("工具種類 ")+fmt(Object.keys(c.tools||{}).length)],[ui("工作累計時間"),fmt(Math.round(duration/60000))+ui(" 分鐘"),ui("已取得時間的對話 ")+fmt(threads.filter(t=>t.task_duration_ms!=null).length)+ui(" 個")],[ui("觀察來源"),fmt(data.mcp?.servers?.length||0),ui("檔案修改 ")+fmt(threads.reduce((n,t)=>n+(t.file_changes?.length||0),0))+ui(" 筆")]]);
  $("tab-jev").hidden=!data.availability?.jev;if($("tab-jev").hidden&&$("tab-jev").getAttribute("aria-selected")==="true")switchTab("overview");
  for(const [id,available]of [["mcp",data.mcp?.servers?.some(s=>s.server!=="web")],["web",data.mcp?.servers?.some(s=>s.server==="web")]]){$("tab-"+id).hidden=!available;if(!available&&$("tab-"+id).getAttribute("aria-selected")==="true")switchTab("overview");}
  $("overview-source-panel").hidden=!data.mcp?.servers?.length;
  renderMonitor();renderErrors();
  updateProjects();renderCodex();renderJev();renderGit();renderWorkflow();renderTools();renderMcp();renderWeb();renderFiles();renderCharts();renderHighlights();
  $("updated").textContent=data.updated_at?ui("更新 ")+new Date(data.updated_at).toLocaleTimeString("zh-TW",{hour12:false}):ui("尚未更新");$("updated").title=when(data.updated_at);$("updated").dateTime=data.updated_at||"";
  if(!settingsBusy&&data.settings){if(document.activeElement!==$("refresh-interval"))$("refresh-interval").value=data.settings.interval;schedule(data.settings.interval);}
  if(detail&&detail.kind!=="jev")renderDetail();attachTables();
}
const errorCategories=editableLabels({conversation:"對話",mcp:"MCP",tool:"工具",codex:"Codex",monitor:"觀察程式"});
const errorSources=editableLabels({codex_desktop:"Codex App",codex_core:"Codex Core",session:"對話紀錄",tool_result:"工具回傳",jev_telemetry:"Jev 紀錄",monitor:"觀察程式"});
const errorReasons=editableLabels({message_submit_failed:"對話送出失敗",stream_interrupted:"回應連線中斷 / 重試",mcp_diagnostic:"MCP 診斷事件",tool_diagnostic:"工具診斷事件",codex_diagnostic:"Codex 診斷事件",process_exit:"命令回傳非零代碼",file_not_found:"找不到檔案",http_error:"HTTP 請求失敗",network_unavailable:"網路無法連線",invalid_response:"回傳格式無法辨識",response_too_large:"回傳內容過大",tool_error:"工具回報錯誤",refresh_failed:"資料整理失敗",http_response_error:"HTTP 回應錯誤",error:"對話回報錯誤",turn_failed:"對話執行失敗",turn_aborted:"對話中止"});
function errorReason(e){return errorReasons[e.reason||e.code]||labels.status[e.code]||ui("來源回報錯誤");}
function errorRows(events){return events.map(e=>{const row=node("tr");cell(row,when(e.timestamp));cell(row).append(tag(ui(e.severity==="warning"?"警告":"錯誤")));cell(row,errorCategories[e.category]||e.category);if(e.thread_id)eventThreadCell(row,e);else cell(row,"--");projectCell(row,e);cell(row,errorSources[e.source]||e.source);if(e.tool)cell(row).append(toolButton(e.server?"mcp__"+e.server+"__"+e.tool:e.tool,()=>openDetail({kind:"tool",tool:e.server?"mcp__"+e.server+"__"+e.tool:e.tool})));else cell(row,"--");cell(row).append(button(errorReason(e),()=>openDetail({kind:"error",event:e}),"link"));cell(row,e.code||"--","mono");return clickableRow(row,()=>openDetail({kind:"error",event:e}));});}
function renderErrors(){
  const errors=data.errors||{},events=errors.events||[],failed=events.filter(e=>e.severity==="error"),warnings=events.filter(e=>e.severity==="warning"),recent=failed.filter(e=>{const time=new Date(e.timestamp).getTime();return time>=Date.now()-86400000&&time<=Date.now()+60000;});
  $("errors-quick").textContent=ui("錯誤 ")+fmt(recent.length);$("errors-quick").classList.toggle("has-errors",!!recent.length);$("errors-quick").title=ui("最近 24 小時 · ")+fmt(recent.length)+ui(" 筆錯誤")+(failed[0]?ui(" · 最新 ")+when(failed[0].timestamp):"");
  const health=errors.diagnostics?.health||{},names={ok:ui("正常"),missing:ui("未找到"),unsupported:ui("格式未支援"),unavailable:ui("無法讀取"),partly_unavailable:ui("部分無法讀取"),disabled:ui("未啟用"),waiting:ui("等待中")};
  $("error-scope").textContent=(errors.enabled?ui("診斷紀錄: App ")+(names[health.desktop]||ui("未知"))+" · Core "+(names[health.core]||ui("未知")):ui("對話與工具錯誤觀察已停用"))+ui(" · 保留最近 ")+fmt(errors.limit||1000)+ui(" 筆事件");
  cards("error-cards",[[ui("最近 24 小時錯誤"),fmt(recent.length),ui("點右上角可快速查看")],[ui("目前載入的錯誤"),fmt(failed.length),ui("包含所有已載入的時間")],[ui("警告"),fmt(warnings.length),ui("包含連線重試與診斷警告")],[ui("涉及對話"),fmt(new Set(events.map(e=>e.thread_id).filter(Boolean)).size),ui("可點整列查看明細")]]);
  replaceRows("error-rows",...errorRows(events));$("error-empty").textContent=events.length?"":ui("尚無錯誤或警告紀錄");
}
$("errors-quick").addEventListener("click",()=>{switchTab("errors");const view=tableViews.get("error-rows");if(view){tableStates["error-rows"]={...tableStates["error-rows"],page:1,filters:{"等級":ui("錯誤")}};renderTableFilters("error-rows");paginateTable("error-rows");persistTables();}$("view-errors").scrollIntoView({block:"start"});});
function renderMonitor(){
  const m=data.monitor||{},c=data.codex,enabled=Object.values(data.settings?.observations||{}).filter(Boolean).length;
  $("monitor-scope").textContent=ui("啟動 ")+when(data.started_at)+ui(" · 每 ")+(data.settings?.interval||10)+ui(" 秒整理");
  cards("monitor-cards",[[ui("整理狀態"),ui(m.health==="error"?"整理失敗":m.health==="ok"?"正常":"啟動中"),ui("累計錯誤 ")+fmt(m.errors||0)+ui(" 次")],[ui("執行時間"),fmt(Math.floor((m.uptime_seconds||0)/60))+ui(" 分鐘"),ui("已整理 ")+fmt(m.refreshes||0)+ui(" 次")],[ui("本輪整理耗時"),fmt(m.refresh_ms)+" ms",ui("CPU 時間 ")+fmt(m.cpu_ms)+" ms"],[ui("保留工具紀錄"),fmt(m.retained_calls||0),ui("上限 ")+fmt(m.call_limit)+ui(" 筆")]]);
  $("monitor-runtime").replaceChildren(metadataList([[ui("程式版本"),m.version||ui("未知")],[ui("Python 版本"),m.python||ui("未知")],[ui("系統"),m.platform||ui("未知")],[ui("處理器架構"),m.architecture||ui("未知")],["PID",m.pid||ui("未知")],[ui("啟用的觀察項目"),fmt(enabled)],[ui("HTTP 請求"),fmt(m.requests||0)],[ui("HTTP 錯誤回應"),fmt(m.http_errors||0)],[ui("上次整理錯誤"),m.last_error_at?when(m.last_error_at):ui("尚無錯誤")],[ui("目前錯誤類型"),m.error_type||ui("無")]]));
  $("monitor-storage").replaceChildren(metadataList([[ui("追蹤 session 檔案"),fmt(c.files||0)+" / "+fmt(m.file_limit)],[ui("保留效能樣本"),fmt(m.history?.length||0)+" / "+fmt(m.history_limit)],[ui("保留狀態事件"),fmt(m.events?.length||0)+" / "+fmt(m.event_limit)],[ui("待完成的紀錄片段 (bytes)"),fmt(m.buffer_bytes)],[ui("片段保留上限 (bytes)"),fmt(m.buffer_limit)],[ui("累計移除的舊工具紀錄"),fmt(m.trimmed_calls||0)],[ui("累計 session 讀取量 (bytes)"),fmt(c.bytes_read)],[ui("損壞紀錄 (行)"),fmt(c.malformed_lines)],[ui("上次資料大小 (bytes)"),fmt(m.snapshot_bytes)],[ui("上次傳輸大小 (bytes)"),fmt(m.transfer_bytes)],[ui("Jev 資料庫大小 (bytes)"),fmt(m.jev_database_bytes)]]));
  $("monitor-retention").textContent=ui("效能樣本與狀態事件保存在記憶體, 超過上限會移除舊資料, 重新啟動後重新累積");
  const names=editableLabels({started:"程式啟動",settings_applied:"設定已套用",recording_enabled:"Jev 紀錄已啟用",recording_disabled:"Jev 紀錄已停用",refresh_failed:"資料整理失敗",recovered:"資料整理恢復",http_response_error:"HTTP 回應錯誤"});
  replaceRows("monitor-event-rows",...(m.events||[]).map(event=>{const row=node("tr");cell(row,when(event.timestamp));cell(row,names[event.kind]||event.kind);cell(row,event.error_type||"--","mono");return row;}));
}
function renderHighlights(){
  const c=data.codex,m=data.mcp||{events:[],categories:{}};
  const items=[[ui("工具"),"tools",fmt(c.observed_tool_calls||0)+ui(" 次呼叫"),fmt(Object.keys(c.tools||{}).length)+ui(" 種工具")],[ui("Git 操作"),"git",fmt(c.git?.total||0)+ui(" 筆"),Object.entries(c.git?.operations||{}).sort((a,b)=>b[1]-a[1]).slice(0,3).map(([key,n])=>key+" "+n).join(" · ")],[ui("Skills"),"workflow",fmt(Object.keys(c.skills?.counts||{}).length)+ui(" 個 Skills"),fmt(c.skills?.events?.length||0)+ui(" 筆讀取紀錄")],[ui("驗證"),"checks",fmt(c.checks?.length||0)+ui(" 筆"),ui("test / build / lint")],[ui("檔案"),"files",fmt(c.file_activity?.total||0)+ui(" 筆"),ui("讀取 ")+fmt(c.file_activity?.operations?.read||0)+ui(" 次")],[ui("網路"),"web",fmt(m.events.filter(e=>e.server==="web").length)+ui(" 筆"),ui("參考網址 ")+fmt(new Set(references(m.events).map(e=>e.url)).size)+ui(" 個")]];
  items.push([ui("錯誤紀錄"),"errors",fmt(data.errors?.severities?.error||0)+ui(" 筆錯誤"),fmt(data.errors?.severities?.warning||0)+ui(" 筆警告")]);
  if(data.availability?.jev)items.push(["Jev","jev",fmt(data.jev.summary?.calls||0)+ui(" 次操作"),ui("HTTP 重試 ")+fmt(data.jev.summary?.retries||0)+ui(" 次")]);
  for(const category of [...new Set((data.mcp?.servers||[]).filter(s=>s.enabled).map(s=>s.category).concat(m.events.map(e=>e.category)))]){
    if(category==="jev")continue;const events=m.events.filter(e=>e.category===category),results=events.map(e=>e.result||{}),pieces=[];
    const metric=(key,label)=>{const values=results.map(r=>r[key]).filter(v=>typeof v==="number");if(values.length)pieces.push(label+" "+fmt(values.reduce((a,b)=>a+b,0)));};
    if(category==="documents"){metric("ocr_processed_items",ui("OCR 項目"));metric("ocr_errors",ui("OCR 錯誤"));pieces.push(ui("寫入 ")+results.filter(r=>r.written===true).length);}
    if(category==="workspace"){metric("changed_files",ui("差異檔案"));metric("checks_failed",ui("驗證失敗"));}
    if(category==="review")metric("findings",ui("發現項目"));
    if(category==="web")pieces.push(ui("參考網址 ")+new Set(references(events).map(e=>e.url)).size);
    if(!pieces.length)pieces.push(ui("已提供結果 ")+results.filter(r=>r.status).length);
    items.push([ui(m.categories[category])||category,category==="web"?"web":"mcp",events.filter(e=>!e.nested).length+ui(" 次呼叫 · ")+events.filter(e=>e.nested).length+ui(" 處 exec 辨識"),pieces.join(" · "),category]);
  }
  $("overview-highlights").replaceChildren(...items.map(([name,tab,value,info,category])=>{const el=button(null,()=>{switchTab(tab);if(category&&tab==="mcp"){$("filter-mcp-category").value=category;$("filter-mcp-server").value="all";renderMcp();}},"highlight-card");el.append(node("h3",name),node("div",value,"highlight-value"),node("p",info,"muted"));return el;}));
}
function metadataList(items){const list=node("dl",null,"metadata-grid");for(const [label,value]of items)list.append(node("dt",label),node("dd",value));return list;}
function table(headers,rows,title){const wrap=node("div",null,"table-wrap"),el=node("table"),head=node("thead"),tr=node("tr"),body=node("tbody");el.dataset.tableKey="detail:"+detail.kind+":"+headers.join("|");if(title)el.dataset.tableTitle=title;for(const label of headers)tr.append(node("th",label));head.append(tr);body.append(...rows);el.append(head,body);wrap.append(el);return wrap;}

function toolEventRows(events){return events.slice(0,100).map(event=>{const row=node("tr");cell(row,when(event.timestamp));const outer=cell(row,null,"tool-column");outer.append(toolButton(event.tool,()=>openDetail({kind:"tool",tool:event.tool})));const inner=cell(row,null,"nested-tool-column"),list=node("div",null,"tools");for(const [tool,count]of sorted(event.nested_tools))list.append(toolButton(tool,()=>openDetail({kind:"nested-tool",tool}),count));inner.append(list.childElementCount?list:node("span","--"));cell(row,when(event.completed_at));valueCell(row,event.duration_ms);return row;});}
const toolPurposes={exec:"執行 JavaScript, 可在一次操作中呼叫其他工具並整理回傳結果",exec_command:"執行終端機命令, 例如讀取檔案, Git 操作或測試",write_stdin:"向已啟動的命令送入資料, 或取得後續執行結果",apply_patch:"依 patch 新增, 修改或刪除檔案",web__run:"搜尋網路, 開啟參考頁面, 或取得天氣與其他網路資料",view_image:"檢視本機圖片",js:"在持續存在的 JavaScript runtime 中執行操作",js_reset:"重設 JavaScript runtime 與其中的變數",document_status:"檢查文件處理套件與 OCR 是否就緒",extract_document:"取得文件文字與位置資訊, 必要時執行 OCR, 可預覽或寫出 Markdown",inspect_markdown:"讀取 Markdown 的標題, 指定段落與 SHA-256",locate_markdown_extracts:"尋找與原始文件相符的 Markdown 抽出檔",update_markdown:"檢查目前 hash 後, 預覽或寫入 Markdown 更新",workspace_status:"查看環境檢查工具的版本與可讀取範圍",compare_environment:"比對兩個目錄的檔案與 SHA-256, 找出環境差異",validation_evidence:"整理既有驗證紀錄與結果",jev_rank:"依問題將候選內容排序, 決定優先閱讀順序",jev_evaluate:"依明確的評分項目評估指定內容",jev_status:"檢查 Jev 設定, 可選擇測試 API 連線",pull_requests_checks:"讀取 PR / MR 的 CI 檢查結果",create_worktree:"建立供目前工作使用的 Git worktree",archive_worktree:"保存 worktree 的工作快照並封存",restore_worktree:"還原已封存的 worktree",fork_thread:"從既有對話建立保留脈絡的分支",handoff_thread:"移轉工作與 Git 狀態",automation_update:"建立或調整排程工作",read_thread:"讀取指定工作的狀態與摘要",list_threads:"列出目前工作與對話",open_in_codex:"在 Codex 面板開啟檔案, 頁面或其他成果"};
function defaultPurpose(tool){const suffix=tool.split("__").at(-1).split(".").at(-1);return toolPurposes[tool]||toolPurposes[suffix]||ui("尚未提供用途說明");}
function purpose(tool){return data?.settings?.tool_descriptions?.[tool]||defaultPurpose(tool);}
function purposeBlock(content,tool){const section=node("div",null,"purpose-block");section.append(node("p",purpose(tool),"tool-purpose"),button(ui("編輯工具說明"),()=>openToolDocs(tool),"link"));content.append(section);}
function appendJevCalls(content,t){if(!data.availability?.jev&&!t.jev_calls?.length)return;content.append(node("h4",ui("Jev 呼叫")));if(!t.jev_calls?.length){content.append(node("p",ui("尚無 Jev 呼叫紀錄"),"empty"));return;}for(const call of t.jev_calls){const row=node("div",null,"detail-row");row.append(node("span",when(call.timestamp)),button(call.operation+ui(" · 送出 / 回傳"),()=>openJev(call,t),"link"));content.append(row);}}
const detailHistory=[];
function rememberDetail(){if(detail&&$("detail-dialog").open){detailHistory.push({view:detail,scroll:$("detail-content").scrollTop});if(detailHistory.length>20)detailHistory.shift();}}
function clearDetailTabs(){const nav=$("detail-content").previousElementSibling;if(nav?.classList.contains("modal-tabs"))nav.remove();}
function updateDetailBack(){$("detail-back").disabled=!detailHistory.length;}
function openDetail(value,remember=true){if(remember)rememberDetail();detail=value;renderDetail();$("detail-content").scrollTop=0;updateDetailBack();if(!$("detail-dialog").open)$("detail-dialog").showModal();}
function renderDetail(){
  clearDetailTabs();
  if(!detail||!data)return;
  const content=$("detail-content");content.replaceChildren();
  if(detail.kind==="thread"||detail.kind==="thread-tools"){
    const t=data.codex.threads.find(item=>item.thread_id===detail.id)||detail.thread;detail.thread=t;
    $("detail-title").textContent=(detail.kind==="thread-tools"?ui("工具明細 · "):"")+title(t);
    if(detail.kind==="thread"){
      content.append(metadataList([["Thread ID",t.thread_id],[ui("對話建立時間"),when(t.created_at)],[ui("最近活動時間"),when(t.updated_at)],[ui("Token 更新時間"),when(t.token_updated_at)],[ui("類型"),labels.type[t.activity_type]||ui("未知")],[ui("執行位置"),labels.environment[t.environment]||ui("未知")],[ui("專案"),t.project_name||t.project_id||(t.project_scope==="none"?ui("無專案"):ui("未知"))],[ui("活動來源"),labels.trigger[t.trigger]||ui("未知")],[ui("排程綁定"),t.has_schedule?ui("有"):ui("未知")],[ui("狀態"),labels.status[t.status]||t.status||ui("未知")],["Model",t.model||ui("未知")],[ui("Reasoning 等級"),t.reasoning_effort||ui("未知")],[ui("Model 設定更新時間"),when(t.context_updated_at)]]));
      content.append(metadataList([[ui("資料來源"),t.metadata_only?ui("本機對話目錄"):ui("Codex session 與本機目錄")],[ui("狀態來源"),t.status_source==="session_lifecycle"?ui("工作開始 / 完成事件"):t.status_source==="catalog_status"?ui("本機目錄狀態"):t.status_backfill_pending?ui("正在回查較早的工作事件"):ui("來源未提供狀態")]]));
      if(t.metadata_only)content.append(node("p",ui(t.activity_type==="chat"?"這筆雲端對話的資料來自本機目錄; 來源未提供的欄位顯示未知":"目前只取得對話目錄資料, 尚未載入對應的 session"),"muted"));
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
    appendJevCalls(content,t);
    content.append(node("h4",ui("檔案讀寫紀錄")));const files=[...(t.file_reads||[]),...(t.file_changes||[])];
    if(files.length)content.append(table([ui("時間"),ui("操作"),ui("檔案位置"),ui("工具"),ui("紀錄方式"),ui("工具回傳時間"),ui("耗時 (ms)")],fileRows(files,false)));else content.append(node("p",ui("尚無檔案讀寫紀錄"),"empty"));
    const threadErrors=(data.errors?.events||[]).filter(e=>e.thread_id===t.thread_id);if(threadErrors.length)content.append(node("h4",ui("錯誤與警告")),table([ui("時間"),ui("等級"),ui("類型"),ui("對話"),ui("專案"),ui("來源"),ui("工具"),ui("錯誤說明"),ui("代碼")],errorRows(threadErrors)));
    const mcp=(data.mcp?.events||[]).filter(e=>e.thread_id===t.thread_id);
    if(mcp.length){content.append(node("h4",ui("MCP / Web 操作")));for(const e of mcp.slice(0,50)){const row=node("div",null,"detail-row");row.append(tag(e.server),button(e.tool,()=>openDetail({kind:"mcp",event:e}),"link"),tag(labels.status[e.result?.status]||e.result?.status||"--"));content.append(row);}}
    const refs=references(mcp);if(refs.length){content.append(node("h4",ui("網路參考")));for(const url of [...new Set(refs.map(e=>e.url))])content.append(referenceLink(url));}
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
    content.append(metadataList([[ui("檔案位置"),fileLocation(e)],[ui("紀錄中的路徑"),e.path],[ui("工作目錄"),e.workdir||ui("未知")],[ui("紀錄方式"),e.nested?ui("exec 程式碼中的呼叫位置"):ui("工具呼叫")],[ui("呼叫時間"),when(e.timestamp)],[ui("工具回傳時間"),when(e.completed_at)],[ui("耗時 (ms)"),fmt(e.duration_ms)],[ui("exec 整次耗時 (ms)"),fmt(e.container_duration_ms)],["Call ID",e.call_id]]));
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
    content.append(metadataList([[ui("時間"),when(e.timestamp)],[ui("等級"),ui(e.severity==="warning"?"警告":"錯誤")],[ui("類型"),errorCategories[e.category]||e.category],[ui("來源"),errorSources[e.source]||e.source],[ui("代碼"),e.code||ui("未知")],[ui("錯誤類型"),e.error_type||ui("未知")],[ui("來源模組"),e.module||ui("未知")],[ui("操作"),e.method||ui("未知")],["HTTP status",fmt(e.http_status)],[ui("命令回傳代碼"),fmt(e.exit_code)],["Thread ID",e.thread_id||ui("未知")],["Call ID",e.call_id||ui("未知")]]));
    if(e.tool){const full=e.server?"mcp__"+e.server+"__"+e.tool:e.tool;content.append(node("h4",ui("工具用途")));purposeBlock(content,full);}
    const t=threadIndex.get(e.thread_id);if(t)content.append(threadLink(t,ui("查看對話 · ")+title(t)));
    const mcp=(data.mcp?.events||[]).find(item=>item.thread_id===e.thread_id&&item.call_id===e.call_id&&item.server===e.server);if(mcp)content.append(button(ui("查看 MCP 操作"),()=>openDetail({kind:"mcp",event:mcp})));
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
  if(!$("detail-dialog").open)$("detail-dialog").showModal();
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
  for(const key of ["mcp_sources","mcp_categories","tool_descriptions"]){const entries=Object.entries(saved[key]||{}).filter(([name,value])=>key==="tool_descriptions"?/^[a-zA-Z0-9_.:-]{1,160}$/.test(name)&&typeof value==="string"&&value.length<=400:/^[a-zA-Z0-9_.-]{1,80}$/.test(name)&&(key==="mcp_sources"?typeof value==="boolean":typeof value==="string"&&Object.hasOwn(categories,value))).slice(0,64);if(entries.length)next[key]=Object.fromEntries(entries);}
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
  settingsBusy=true;version++;$("action-message").textContent=ui("更新設定中");feedback("settings-message",ui("正在套用設定"),"pending");
  try{const next=await post("/api/settings",value);data.settings=next;schedule(next.interval);$("refresh-interval").value=next.interval;$("session-count").disabled=next.track_all;saveView();$("action-message").textContent=ui("設定已更新");feedback("settings-message",ui("設定已套用"));}
  catch(error){$("action-message").textContent=error.message;feedback("settings-message",error.message,"error");throw error;}
  finally{settingsBusy=false;await refresh();}
}
$("jev-recording").addEventListener("change",async()=>{
  if(recordingBusy)return;const requested=$("jev-recording").checked;recordingBusy=true;version++;$("jev-recording").disabled=true;$("jev-recording-message").textContent=ui("更新設定中");
  try{const result=await post("/api/jev/recording",{enabled:requested});data.jev.enabled=result.enabled;data.jev.enabled_at=result.enabled_at;$("jev-recording-message").textContent=result.enabled?ui("Jev 紀錄已啟用"):ui("Jev 紀錄已停用");}
  catch(error){$("jev-recording").checked=!requested;$("jev-recording-message").textContent=error.message;}
  finally{recordingBusy=false;$("jev-recording").disabled=false;renderJev();await refresh();}
});
async function applyFrequency(){
  if(settingsBusy)return;
  const input=$("refresh-interval"),seconds=Number(input.value);if(!input.checkValidity()||!Number.isInteger(seconds)){$("action-message").textContent=ui("更新頻率請輸入 1 - 3600 秒的整數");input.value=refreshSeconds;return;}
  input.disabled=true;try{await changeSettings({interval:seconds});}catch{input.value=refreshSeconds;}finally{input.disabled=false;}
}
$("refresh-interval").addEventListener("change",applyFrequency);
$("refresh-interval").addEventListener("keydown",event=>{if(event.key==="Enter"){event.preventDefault();applyFrequency();}});
$("apply-frequency").addEventListener("click",applyFrequency);
$("track-all").addEventListener("change",async()=>{const input=$("track-all"),value=input.checked;input.disabled=true;try{await changeSettings({track_all:value});}catch{input.checked=!value;}finally{input.disabled=false;}});
function openSettings(){
  $("display-options").value=display.options.join(", ");fillDisplayOptions($("default-ranking"),display.ranking);fillDisplayOptions($("default-table"),display.table);
  if(!data)return;applyAppearance();$("refresh-interval").value=data.settings.interval;$("session-count").value=data.settings.max_files;$("session-count").disabled=data.settings.track_all;$("track-all").checked=data.settings.track_all;
  const rows=[];for(const [key,enabled]of Object.entries(data.settings.observations)){if((key==="jev"||key==="jev_calls")&&!data.availability?.jev)continue;const row=node("label",null,"setting-row"),input=node("input");input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=!!enabled;input.name=key;input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({observations:{[key]:requested}});}catch{input.checked=!requested;}finally{input.disabled=false;}});row.append(node("span",observationLabels[key]||key),input);rows.push(row);}$("observation-options").replaceChildren(...rows);
  const sources=data.mcp?.servers||[];$("source-settings").hidden=!sources.length;$("mcp-settings").replaceChildren(...sources.map(s=>{const row=node("div",null,"setting-row source-setting"),name=node("label",s.server),enabled=node("input");enabled.type="checkbox";enabled.className="switch";enabled.setAttribute("role","switch");enabled.checked=s.enabled;enabled.setAttribute("aria-label",s.server+ui(" 觀察開關"));const select=node("select");select.setAttribute("aria-label",s.server+ui(" 分類"));for(const [key,label]of Object.entries(data.mcp.categories)){const option=node("option",ui(label));option.value=key;select.append(option);}select.value=s.category;enabled.addEventListener("change",async()=>{const requested=enabled.checked;enabled.disabled=true;try{await changeSettings({mcp_sources:{[s.server]:requested}});}catch{enabled.checked=!requested;}finally{enabled.disabled=false;}});select.addEventListener("change",async()=>{select.disabled=true;try{await changeSettings({mcp_categories:{[s.server]:select.value}});}catch{select.value=s.category;}finally{select.disabled=false;}});name.prepend(enabled);row.append(name,select);return row;}));
  renderTabSettings();const section=node("section");section.append(node("h4",ui("介面與說明")),button(ui("介面與說明設定"),()=>openToolDocs(),""));section.className="document-actions";section.id="tool-purpose-settings";$("tool-purpose-settings")?.remove();$("source-settings").after(section);
  $("settings-message").textContent="";if(!$("settings-dialog").open)$("settings-dialog").showModal();
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
const defaultHiddenColumns={"error-rows":["專案","工具"],"codex-rows":["Thread ID","類型","位置","活動來源","Cached input","Reasoning output"],"mcp-rows":["分類","紀錄方式"],"file-rows":["紀錄方式"],"jev-rows":["HTTP 結果"],"web-event-rows":["紀錄方式"]};
let tableSettingsKey=null;

const sortCollator=new Intl.Collator("zh-TW",{numeric:true,sensitivity:"base"});
function comparable(value){if(value==null||value===""||value==="--"||value===ui("未知")||value===ui("尚無紀錄"))return null;if(typeof value==="number")return value;const text=String(value).trim();if(/^-?[\d,]+(?:\.\d+)?$/.test(text))return Number(text.replaceAll(",",""));const date=text.match(/^(\d{4})[/-](\d{1,2})[/-](\d{1,2})[ T](\d{1,2}):(\d{2})(?::(\d{2}))?/);if(date)return new Date(Number(date[1]),Number(date[2])-1,Number(date[3]),Number(date[4]),Number(date[5]),Number(date[6]||0)).getTime();return text;}
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
function renderTableFilters(key){
  const view=tableViews.get(key);if(view.table.closest("dialog")||specialTables.has(key)||key==="file-rows")return;
  if(!view.filters){
    const wrap=view.table.closest(".table-wrap"),controls=node("div",null,"filters table-filters"),fields=new Map(),existing=[...wrap.closest(".panel").querySelectorAll(".filters label")].map(label=>label.firstChild?.textContent.trim());
    const search=node("input"),label=node("label",ui("搜尋紀錄"));search.type="search";search.value=tableStates[key]?.filters?.search||"";label.append(search);controls.append(label);search.addEventListener("input",()=>update("search",search.value));
    for(const [index,header]of view.headers.entries())if(/對話|專案|來源|工具$|操作$|結果$|Skill$|類型$|等級$|事件$|Model$|紀錄方式|目錄名稱/.test(header)&&!existing.includes(ui(header))){const select=node("select"),label=node("label",ui(header));select.setAttribute("aria-label",view.title+" · "+ui(header));select.addEventListener("change",()=>update(header,select.value));fields.set(header,{index,select});label.append(select);controls.append(label);}
    function update(header,value){tableStates[key]={...tableStates[key],page:1,filters:{...tableStates[key]?.filters,[header]:value}};paginateTable(key);persistTables();}
    wrap.previousElementSibling.before(controls);view.filters={controls,fields,search};
  }
  for(const [header,{index,select}]of view.filters.fields){const values=[...new Set(view.rows.map(row=>rowValue(row,index)))].sort(sortCollator.compare),signature=JSON.stringify(values),selected=tableStates[key]?.filters?.[header]||"";if(select.dataset.signature!==signature){select.replaceChildren(...["",...values].map(value=>{const option=node("option",value||ui("全部"));option.value=value;return option;}));select.dataset.signature=signature;}if(!values.includes(selected)&&selected){tableStates[key].filters[header]="";select.value="";}else select.value=selected;}
}
function adaptTable(view){if(view.table.closest("[hidden]")||view.table.closest("dialog")&&!view.table.closest("dialog").open)return;const width=view.table.closest(".table-wrap").clientWidth,visible=view.headerCells.filter(cell=>!cell.hidden).length,font=Number(document.documentElement.dataset.font)||14,stacked=width<760||visible*88*font/14>width;view.table.classList.add("table-fluid");view.table.classList.toggle("table-stacked",stacked);for(const th of view.headerCells)if(th.querySelector("button"))th.querySelector("button").tabIndex=stacked?-1:0;}
function columnOrder(key){const view=tableViews.get(key),saved=tableStates[key]?.columns;return [...new Set((Array.isArray(saved)?saved:[]).filter(header=>view.headers.includes(header)).concat(view.headers))].map(header=>view.headers.indexOf(header));}
function applyTableColumns(key){
  const view=tableViews.get(key);if(!view)return;const hidden=Array.isArray(tableStates[key]?.hidden)?tableStates[key].hidden:defaultHiddenColumns[key]||[],order=columnOrder(key),signature=order.join(",");
  for(const [index,th]of view.headerCells.entries())th.hidden=hidden.includes(view.headers[index]);
  if(view.orderSignature!==signature){view.headerCells[0]?.parentElement.append(...order.map(index=>view.headerCells[index]));view.orderSignature=signature;}
  for(const row of specialTables.has(key)?view.body.children:view.rows){const cells=rowCells(row);for(const [index,td]of cells.entries()){td.hidden=hidden.includes(view.headers[index]);td.dataset.label=ui(view.headers[index]||"");}if(row.dataset.columnOrder!==signature){row.append(...order.map(index=>cells[index]).filter(Boolean));row.dataset.columnOrder=signature;}}
  adaptTable(view);
}
function moveColumn(key,header,target,after=false){
  if(header===target)return;const view=tableViews.get(key),columns=columnOrder(key).map(index=>view.headers[index]).filter(name=>name!==header),index=columns.indexOf(target);if(index<0)return;columns.splice(index+(after?1:0),0,header);tableStates[key]={...tableStates[key],columns};applyTableColumns(key);persistTables();if($("table-dialog").open&&tableSettingsKey===key){renderTableColumns(key);feedback("table-settings-message",ui("欄位順序已更新"));}$("action-message").textContent=ui("欄位順序已更新");
}
function dragColumn(handle,key,header,isHeader=false){
  let drag=null,blockClick=false,ghost=null;const scope=isHeader?tableViews.get(key).table.querySelector("thead"):$("table-columns"),clear=()=>{for(const el of scope.querySelectorAll("[data-column-key]"))el.classList.remove("column-dragging","column-drop-before","column-drop-after");};
  const stop=()=>{ghost?.remove();ghost=null;window.removeEventListener("pointermove",move);window.removeEventListener("pointerup",finish);window.removeEventListener("pointercancel",cancel);};
  function cancel(){drag=null;clear();stop();}
  function move(event){if(!drag||drag.pointer!==event.pointerId)return;if(!drag.moved&&Math.hypot(event.clientX-drag.x,event.clientY-drag.y)<5)return;if(!drag.moved){drag.moved=true;handle.setPointerCapture(event.pointerId);ghost=node("div",null,"column-drag-preview");ghost.setAttribute("aria-hidden","true");(handle.closest("dialog")||document.body).append(ghost);}event.preventDefault();clear();handle.closest("[data-column-key]").classList.add("column-dragging");const el=document.elementFromPoint(event.clientX,event.clientY)?.closest("[data-column-key]");drag.target=el&&scope.contains(el)&&el.dataset.columnKey!==header?el.dataset.columnKey:null;if(drag.target){const rect=el.getBoundingClientRect();drag.after=isHeader?event.clientX>rect.left+rect.width/2:event.clientY>rect.top+rect.height/2;el.classList.add(drag.after?"column-drop-after":"column-drop-before");}ghost.textContent=ui(header)+(drag.target?ui(" → 放在 ")+ui(drag.target)+ui(drag.after?" 後面":" 前面"):ui(" · 選擇插入位置"));ghost.style.left=Math.max(8,Math.min(event.clientX+18,innerWidth-ghost.offsetWidth-8))+"px";ghost.style.top=Math.max(8,Math.min(event.clientY+18,innerHeight-ghost.offsetHeight-8))+"px";}
  function finish(event){if(!drag||drag.pointer!==event.pointerId)return;const current=drag;drag=null;clear();stop();blockClick=current.moved;if(current.moved&&current.target)moveColumn(key,header,current.target,current.after);}
  handle.addEventListener("pointerdown",event=>{if(event.button!==0||event.target.closest("input"))return;drag={pointer:event.pointerId,x:event.clientX,y:event.clientY,moved:false,target:null,after:false};window.addEventListener("pointermove",move);window.addEventListener("pointerup",finish);window.addEventListener("pointercancel",cancel);});
  handle.addEventListener("lostpointercapture",()=>{if(drag)cancel();});
  handle.addEventListener("click",event=>{if(blockClick){event.preventDefault();event.stopPropagation();blockClick=false;}},true);
  handle.addEventListener("keydown",event=>{if(isHeader&&!event.altKey||!["ArrowLeft","ArrowRight","ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const view=tableViews.get(key),columns=columnOrder(key).map(index=>view.headers[index]),index=columns.indexOf(header),after=["ArrowRight","ArrowDown","End"].includes(event.key),target=event.key==="Home"?columns[0]:event.key==="End"?columns.at(-1):columns[index+(after?1:-1)];if(target){moveColumn(key,header,target,after);if(isHeader)view.headerCells[view.headers.indexOf(header)].querySelector("button").focus();else [...$("table-columns").querySelectorAll("[data-column-key]")].find(el=>el.dataset.columnKey===header)?.querySelector(".column-drag-handle").focus();}});
}
function renderTableColumns(key){
  const view=tableViews.get(key),hidden=Array.isArray(tableStates[key]?.hidden)?tableStates[key].hidden:defaultHiddenColumns[key]||[];
  $("table-columns").replaceChildren(...columnOrder(key).map(index=>{const header=view.headers[index],option=node("div",null,"column-option"),label=node("label"),input=node("input"),handle=button("⠿",()=>{},"column-drag-handle");option.dataset.columnKey=header;handle.setAttribute("aria-label",ui(header)+ui(" 欄位拖曳排序"));handle.title=ui("拖曳排序, 或用方向鍵移動");input.type="checkbox";input.checked=!hidden.includes(header);input.addEventListener("change",()=>{const next=new Set(tableStates[key]?.hidden||defaultHiddenColumns[key]||[]);if(input.checked)next.delete(header);else next.add(header);if(view.headers.every(name=>next.has(name))){input.checked=true;feedback("table-settings-message",ui("至少保留一個欄位"),"error");return;}tableStates[key]={...tableStates[key],hidden:[...next]};applyTableColumns(key);persistTables();feedback("table-settings-message",ui("顯示欄位已更新"));});label.append(input,node("span",ui(header)));option.append(handle,label);dragColumn(handle,key,header);return option;}));
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
    for(const [index,th]of view.headerCells.entries()){th.setAttribute("aria-sort",index===active?sort.descending?"descending":"ascending":"none");if(!th.querySelector("button")){const control=button("",()=>{const current=tableStates[key]?.sort||{column:-1,descending:true},selected=sortColumn(key,current),column=selected<0?view.headers.findIndex(label=>/時間|日期/.test(label)&&!label.includes("耗時")):selected;changeTableSort(key,{column:index,descending:column===index?!current.descending:false});},"sort-header");th.replaceChildren(control);th.dataset.columnKey=view.headers[index];th.title=ui("拖曳欄位調整順序; Alt + 方向鍵也可移動");dragColumn(th,key,view.headers[index],true);}th.querySelector("button").textContent=ui(view.headers[index])+(index===active?sort.descending?" ↓":" ↑":" ↕");}
    adaptTable(view);

  }
}
function paginateTable(key){
  const view=tableViews.get(key);if(!view)return;const state=tableStates[key]||{size:display.table,page:1},size=tableSize(key),rows=matchingRows(key,view.rows),pages=Math.max(1,Math.ceil(rows.length/size));state.page=Math.max(1,Math.min(state.page,pages));tableStates[key]=state;
  const start=(state.page-1)*size;view.body.replaceChildren(...sortRows(key,view,rows).slice(start,start+size));view.first=view.body.firstChild;syncPageInput(view.input,state.page,pages);view.total.textContent="/ "+pages;view.summary.textContent=ui("共 ")+fmt(rows.length)+ui(" 筆")+(rows.length!==view.rows.length?ui(" · 全部 ")+fmt(view.rows.length)+ui(" 筆"):"");view.prev.disabled=state.page===1;view.next.disabled=state.page===pages;view.footer.hidden=!view.rows.length;applyTableColumns(key);
}
function persistTables(){preferences.tables=tableStates;saveView();}
function moveTablePage(key,delta){const state=tableStates[key]||{size:display.table,page:1};state.page+=delta;tableStates[key]=state;paginateTable(key);persistTables();}
function openTableSettings(key){tableSettingsKey=key;const view=tableViews.get(key);$("table-settings-title").textContent=(view?.title||ui("表格"))+ui(" · 表格設定");$("table-size").value=key==="codex-rows"?$("page-size").value:String(tableStates[key]?.size||display.table);$("table-settings-message").textContent="";fillDisplayOptions($("table-size"),$("table-size").value);renderTableSort(key);renderTableColumns(key);if(!$("table-dialog").open)$("table-dialog").showModal();}
$("table-size").addEventListener("change",()=>{const key=tableSettingsKey,value=$("table-size").value;tableStates[key]={...tableStates[key],size:value==="all"?"all":Number(value),page:1};if(key==="codex-rows"){$("page-size").value=value;page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);persistTables();feedback("table-settings-message",ui("顯示數量已更新"));});
$("table-settings-close").addEventListener("click",()=>$("table-dialog").close());

let docsMode="tools",docsBusy=false,confirmAction=null;
function openToolDocs(tool){
  if(!data)return;docsMode="tools";docTools=[...new Set([tool,...Object.keys(data.codex.tools||{}),...Object.keys(data.codex.nested_tools||{}),...Object.keys(data.settings.tool_descriptions||{})].filter(Boolean))].sort();
  $("docs-search").value=tool||"";docsPage=1;renderToolDocs();$("docs-message").textContent="";
  if(!$("docs-dialog").open)$("docs-dialog").showModal();
}
function visibleDocTools(){const query=$("docs-search").value.trim().toLowerCase(),items=docsMode==="tools"?docTools:[...copyCatalog].sort(),times=new Map();if(docsMode==="tools")for(const thread of data.codex.threads||[])for(const event of thread.tool_events||[])for(const tool of [event.tool,...Object.keys(event.nested_tools||{})])if(event.timestamp>(times.get(tool)||""))times.set(tool,event.timestamp);const filtered=items.filter(key=>(key+" "+(docsMode==="copy"?ui(key):purpose(key))).toLowerCase().includes(query));return sortRecords("docs-rows",filtered,[key=>key,key=>docsMode==="copy"?ui(key):purpose(key)],key=>times.get(key));}
function docKey(key){return docsMode+":"+key;}
function renderToolDocs(){
  const tools=visibleDocTools(),size=tableSize("docs-rows"),pages=Math.max(1,Math.ceil(tools.length/size));docsPage=Math.max(1,Math.min(docsPage,pages));
  for(const [mode,id]of [["tools","docs-tools-mode"],["copy","docs-copy-mode"]]){const selected=docsMode===mode;$(id).setAttribute("aria-selected",String(selected));$(id).tabIndex=selected?0:-1;}$("docs-panel").setAttribute("aria-labelledby",docsMode==="tools"?"docs-tools-mode":"docs-copy-mode");
  $("docs-column-key").textContent=ui(docsMode==="tools"?"工具":"原始文案");$("docs-column-text").textContent=ui(docsMode==="tools"?"用途說明":"顯示文字");$("docs-search").placeholder=ui(docsMode==="tools"?"工具名稱":"原始文案或顯示文字");
  $("docs-column-key").dataset.copyBase=docsMode==="tools"?"工具":"原始文案";$("docs-column-text").dataset.copyBase=docsMode==="tools"?"用途說明":"顯示文字";const view=tableViews.get("docs-rows");if(view){view.headers[0]=$("docs-column-key").dataset.copyBase;view.headers[1]=$("docs-column-text").dataset.copyBase;}
  replaceRows("docs-rows",...tools.slice((docsPage-1)*size,docsPage*size).map(key=>{const row=node("tr");cell(row,key,"path-cell"+(docsMode==="tools"?" mono":""));const input=node("textarea"),draft=docKey(key);input.rows=3;input.maxLength=400;input.value=docDrafts.has(draft)?docDrafts.get(draft):docsMode==="tools"?purpose(key):ui(key);input.setAttribute("aria-label",key+ui(docsMode==="tools"?" 用途說明":" 顯示文字"));input.addEventListener("input",()=>docDrafts.set(draft,input.value));cell(row).append(input);const actions=cell(row,null,"doc-actions");actions.append(button(ui("儲存"),()=>saveDocuments({[key]:input.value.trim()})),button(ui("還原預設"),()=>saveDocuments({[key]:""}),"link"));return row;}));
  $("docs-page-summary").textContent=ui("第 ")+docsPage+" / "+pages+ui(" 頁 · ")+tools.length+(docsMode==="tools"?" 個工具":" 項介面文字");$("docs-prev").disabled=docsPage===1;$("docs-next").disabled=docsPage===pages;syncPageInput($("docs-page-number"),docsPage,pages);attachTables();
}
function setDocsBusy(value){docsBusy=value;for(const el of $("docs-dialog").querySelectorAll("button:not(#docs-close),input,textarea"))el.disabled=value;}
async function saveDocuments(changes){
  if(docsBusy)return;setDocsBusy(true);feedback("docs-message",ui("正在儲存"),"pending");const mode=docsMode;
  try{
    if(mode==="tools")await changeSettings({tool_descriptions:changes});
    else{const next={...preferences.copy,...changes};for(const [key,value]of Object.entries(next))if(!value.trim())delete next[key];if(Object.keys(next).length>500)throw new Error(ui("自訂文案最多 500 項"));preferences.copy=next;saveView();applyCopy();render(data);if($("settings-dialog").open)openSettings();}
    for(const key of Object.keys(changes))docDrafts.delete(mode+":"+key);feedback("docs-message",ui(mode==="tools"?"工具說明已儲存":"介面設定已儲存"));
  }catch(error){feedback("docs-message",error.message,"error");}finally{setDocsBusy(false);renderToolDocs();}
}
function confirmChange(title,text,action,accept=ui("確認還原")){
  $("confirm-accept").textContent=accept;$("confirm-title").textContent=ui(title);$("confirm-description").textContent=ui(text);confirmAction=action;$("confirm-dialog").showModal();}
async function resetDocuments(){
  if(docsBusy)return;setDocsBusy(true);feedback("docs-message",ui("正在還原預設"),"pending");
  try{const changes=Object.fromEntries(Object.keys(data.settings.tool_descriptions||{}).map(key=>[key,""]));if(Object.keys(changes).length)await changeSettings({tool_descriptions:changes});preferences.copy={};docDrafts.clear();saveView();applyCopy();render(data);if($("settings-dialog").open)openSettings();feedback("docs-message",ui("介面文字與工具說明已全部還原預設"));}
  catch(error){feedback("docs-message",error.message,"error");}finally{setDocsBusy(false);renderToolDocs();}
}
$("docs-reset-all").addEventListener("click",()=>confirmChange("還原全部預設?","所有自訂介面文字與工具說明都會還原, 尚未儲存的修改也會清除",resetDocuments));
$("confirm-accept").addEventListener("click",()=>{const action=confirmAction;$("confirm-dialog").close();if(action)action();});for(const id of ["confirm-cancel","confirm-close"])$(id).addEventListener("click",()=>$("confirm-dialog").close());$("confirm-dialog").addEventListener("close",()=>{confirmAction=null;});
$("docs-search").addEventListener("input",()=>{docsPage=1;renderToolDocs();});$("docs-save-page").addEventListener("click",()=>{const size=tableSize("docs-rows"),tools=visibleDocTools().slice((docsPage-1)*size,docsPage*size),changes=Object.fromEntries(tools.filter(key=>docDrafts.has(docKey(key))).map(key=>[key,docDrafts.get(docKey(key)).trim()]));if(Object.keys(changes).length)saveDocuments(changes);else $("docs-message").textContent=ui("本頁沒有修改");});
for(const [mode,id]of [["tools","docs-tools-mode"],["copy","docs-copy-mode"]]){$(id).addEventListener("click",()=>{docsMode=mode;docsPage=1;$("docs-search").value="";$("docs-message").textContent="";renderToolDocs();});$(id).addEventListener("keydown",event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?"docs-tools-mode":event.key==="End"?"docs-copy-mode":id==="docs-tools-mode"?"docs-copy-mode":"docs-tools-mode";$(next).click();$(next).focus();});}
for(const [id,delta]of [["docs-prev",-1],["docs-next",1]])$(id).addEventListener("click",()=>{docsPage+=delta;renderToolDocs();});$("docs-close").addEventListener("click",()=>$("docs-dialog").close());

function fillDisplayOptions(select,value,ranking=false){
  const current=value==="all"||String(value)==="0"?"all":String(value),values=[...display.options.map(String),"all"];
  if(!values.includes(current)&&/^\d+$/.test(current))values.splice(-1,0,current);
  select.replaceChildren(...values.map(item=>{const option=node("option",item==="all"?ui("全部"):item+(ranking?ui(" 項"):ui(" 筆")));option.value=ranking&&item==="all"?"0":item;return option;}));select.value=ranking&&current==="all"?"0":current;
}
function redrawTable(key){if(key==="codex-rows"){page=1;lazyLimit=50;renderCodex();}else if(key==="mcp-rows"){mcpPage=1;renderMcp();}else if(key==="docs-rows"){docsPage=1;renderToolDocs();}else paginateTable(key);}
$("display-options").addEventListener("change",()=>{const options=$("display-options").value.split(/[,\s]+/).filter(Boolean).map(Number);if(options.length&&options.length<=8&&options.every(n=>Number.isInteger(n)&&n>=1&&n<=200)&&new Set(options).size===options.length){for(const id of ["default-ranking","default-table"]){const previous=$(id).value;$(id).replaceChildren(...[...options,"all"].map(value=>{const option=node("option",value==="all"?ui("全部"):String(value));option.value=value;return option;}));$(id).value=[...options,"all"].map(String).includes(previous)?previous:String(options[0]);}}});
$("apply-display").addEventListener("click",()=>{
  const options=$("display-options").value.split(/[,\s]+/).filter(Boolean).map(Number),ranking=$("default-ranking").value,table=$("default-table").value,next={options,ranking:ranking==="all"?"all":Number(ranking),table:table==="all"?"all":Number(table)};
  if(!validDisplay(next)){feedback("display-message",ui("請輸入 1 到 200 的整數, 最多 8 個選項, 以逗號分開"),"error");return;}
  display=next;for(const view of chartViews.values())if(view.fields.top){view.settings.top=display.ranking==="all"?0:display.ranking;fillDisplayOptions(view.fields.top,display.ranking,true);}
  fillDisplayOptions($("page-size"),display.table);for(const key of tableViews.keys()){tableStates[key]={...tableStates[key],size:display.table,page:1};redrawTable(key);}persistTables();renderCharts();attachTables();feedback("display-message",ui("顯示數量已套用到所有排行榜與表格"));
});
function modalTabs(container,groups,id,selected,onSelect){
  const nav=node("div",null,"document-modes modal-tabs"),entries=[];nav.setAttribute("role","tablist");nav.setAttribute("aria-label",ui("明細分類"));
  for(const [index,[key,label,panel]]of groups.entries()){const tab=button(label,()=>select(key)),tabId=id+"-tab-"+index,panelId=id+"-panel-"+index;tab.id=tabId;tab.setAttribute("role","tab");tab.setAttribute("aria-controls",panelId);panel.id=panelId;panel.setAttribute("role","tabpanel");panel.setAttribute("aria-labelledby",tabId);entries.push({key,tab,panel});nav.append(tab);}
  function select(key){for(const entry of entries){const active=entry.key===key;entry.tab.setAttribute("aria-selected",String(active));entry.tab.tabIndex=active?0:-1;entry.panel.hidden=!active;}onSelect?.(key);}
  for(const [index,entry]of entries.entries())entry.tab.addEventListener("keydown",event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const next=event.key==="Home"?0:event.key==="End"?entries.length-1:(index+(event.key==="ArrowRight"?1:entries.length-1))%entries.length;select(entries[next].key);entries[next].tab.focus();});
  const previous=container.previousElementSibling;if(previous?.classList.contains("modal-tabs"))previous.remove();container.before(nav);container.replaceChildren(...entries.map(entry=>entry.panel));select(entries.some(entry=>entry.key===selected)?selected:entries[0].key);
}
function groupThreadDetails(content){
  const groups=new Map(),headingGroup=new Map([[ui("工具使用 (次)"),"tools"],[ui("exec 內辨識到的工具"),"tools"],[ui("工具呼叫紀錄"),"tools"],[ui("Jev 呼叫"),"jev"],[ui("檔案讀寫紀錄"),"files"],[ui("MCP / Web 操作"),"mcp"],[ui("網路參考"),"mcp"],[ui("錯誤與警告"),"errors"]]),names={summary:ui("對話資訊"),tools:ui("工具"),jev:"Jev",files:ui("檔案"),mcp:ui("MCP / 網路"),errors:ui("錯誤紀錄")};let key="summary";
  for(const child of [...content.children]){if(child.tagName==="H4"&&headingGroup.has(child.textContent))key=headingGroup.get(child.textContent);if(!groups.has(key))groups.set(key,node("section"));groups.get(key).append(child);}
  modalTabs(content,[...groups].map(([id,panel])=>[id,names[id]||id,panel]),"thread-detail",detail.section,key=>detail.section=key);
}
function groupSettings(){const content=$("settings-dialog").querySelector(".settings-content"),status=$("settings-message"),groups=[];status.remove();let panel;for(const child of [...content.children]){if(child.dataset.modalSection){panel=node("section");groups.push([child.dataset.modalSection,ui(child.dataset.modalSection),panel]);}panel?.append(child);}modalTabs(content,groups,"settings-sections","外觀");status.className="settings-feedback";content.before(status);}
let currentTabSettings=null;
function openTabSettings(name){
  currentTabSettings=name;const scope=$("view-"+name),charts=[...chartViews].filter(([id])=>scope.contains($(id))),tables=[...tableViews].filter(([,view])=>scope.contains(view.table));
  $("tab-settings-title").textContent=$("tab-"+name).textContent+ui(" · Tab 設定");$("tab-chart-controls").hidden=!charts.length;$("tab-table-controls").hidden=!tables.length;fillDisplayOptions($("tab-table-size"),display.table);$("tab-settings-message").textContent="";
  const keys={overview:[],codex:["codex","metadata"],tools:["tool_events"],mcp:["mcp"],web:["web"],files:["files"],jev:["jev","jev_calls"],git:["git"],workflow:["skills"],checks:["checks"],monitor:[],errors:["errors"]}[name]||[];
  $("tab-observations").replaceChildren(...keys.filter(key=>data.settings.observations[key]!=null).map(key=>{const row=node("label",null,"setting-row"),input=node("input");input.type="checkbox";input.className="switch";input.setAttribute("role","switch");input.checked=data.settings.observations[key];input.addEventListener("change",async()=>{const requested=input.checked;input.disabled=true;try{await changeSettings({observations:{[key]:requested}});feedback("tab-settings-message",ui("觀察設定已套用"));}catch(error){input.checked=!requested;feedback("tab-settings-message",error.message,"error");}finally{input.disabled=false;}});row.append(node("span",observationLabels[key]||key),input);return row;}));
  $("tab-observations-heading").hidden=!keys.length;$("tab-dialog").showModal();
}
$("apply-tab-charts").addEventListener("click",()=>{const name=currentTabSettings,scope=$("view-"+name),range=$("tab-chart-range").value,length=Number($("tab-chart-length").value),unit=Number($("tab-chart-unit").value);if(!Number.isInteger(length)||length<1||length>365){feedback("tab-settings-message",ui("請輸入 1 到 365 的整數"),"error");return;}for(const [id,view]of chartViews)if(scope.contains($(id))){Object.assign(view.settings,{range,length,unit});for(const key of ["range","length","unit"])view.fields[key].value=view.settings[key];view.fields.length.parentElement.hidden=view.fields.unit.parentElement.hidden=range!=="recent";view.fields.start.parentElement.hidden=view.fields.end.parentElement.hidden=true;}saveView();renderCharts();feedback("tab-settings-message",ui("時間範圍已套用到此 Tab 的圖表"));});
$("apply-tab-tables").addEventListener("click",()=>{const scope=$("view-"+currentTabSettings),value=$("tab-table-size").value;for(const [key,view]of tableViews)if(scope.contains(view.table)){tableStates[key]={...tableStates[key],size:value==="all"?"all":Number(value),page:1};if(key==="codex-rows")fillDisplayOptions($("page-size"),value);redrawTable(key);}persistTables();attachTables();feedback("tab-settings-message",ui("每頁筆數已套用到此 Tab 的表格"));});
$("tab-settings-close").addEventListener("click",()=>$("tab-dialog").close());
function addTabSettings(){for(const tab of document.querySelectorAll("[data-tab]")){const head=$("view-"+tab.dataset.tab).querySelector(".section-head"),gear=button("⚙",()=>openTabSettings(tab.dataset.tab),"chart-setting-button tab-settings-button");gear.setAttribute("aria-label",tab.textContent+ui(" Tab 設定"));gear.title=ui("Tab 設定");const actions=head.querySelector(".section-actions")||node("div",null,"section-actions");actions.append(gear);if(!actions.parentElement)head.append(actions);}}


function configuration(){
  saveView();preferences.tables=tableStates;preferences.tabOrder=tabOrder;const keys=["tab","inputs","page","appearance","display","charts","tables","tableSchema","tabOrder","copy","settings"];
  return {application:"local-activity-monitor",kind:"settings",version:1,exported_at:new Date().toISOString(),preferences:Object.fromEntries(keys.filter(key=>preferences[key]!=null).map(key=>[key,preferences[key]])),recording:!!data.jev?.enabled};
}
function readConfiguration(value){
  const object=item=>item&&typeof item==="object"&&!Array.isArray(item),bounded=(items,max,check)=>object(items)&&Object.keys(items).length<=max&&Object.entries(items).every(([key,item])=>key.length<=400&&!["__proto__","constructor","prototype"].includes(key)&&check(item,key));
  if(!object(value)||value.application!=="local-activity-monitor"||value.kind!=="settings"||value.version!==1||!object(value.preferences)||typeof value.recording!=="boolean")throw new Error(ui("設定檔格式或版本不支援"));
  const p=value.preferences,a=p.appearance,s=p.settings;
  if(!object(a)||!["auto","light","dark"].includes(a.mode)||!["slate","neutral"].includes(a.theme)||!["green","blue","orange"].includes(a.accent)||!Number.isInteger(a.font)||a.font<12||a.font>18||!validDisplay(p.display)||!object(s)||!Number.isInteger(s.interval)||s.interval<1||s.interval>3600||!Number.isInteger(s.max_files)||s.max_files<1||s.max_files>5000||typeof s.track_all!=="boolean")throw new Error(ui("外觀, 更新頻率或顯示數量不在可用範圍"));
  if(!bounded(s.observations,64,item=>typeof item==="boolean")||!bounded(s.mcp_sources||{},64,item=>typeof item==="boolean")||!bounded(s.mcp_categories||{},64,item=>typeof item==="string"&&item.length<=80)||!bounded(s.tool_descriptions||{},64,item=>typeof item==="string"&&item.length<=400)||!bounded(p.copy||{},500,item=>typeof item==="string"&&item.length<=400)||!bounded(p.inputs||{},100,item=>typeof item==="string"&&item.length<=2000))throw new Error(ui("觀察設定或介面文字格式無效"));
  if(!bounded(p.charts||{},100,item=>object(item)&&["all","recent","custom"].includes(item.range)&&Number.isInteger(item.length)&&item.length>=1&&item.length<=365&&[60000,3600000,86400000].includes(item.unit)&&[60000,300000,900000,3600000,21600000,86400000].includes(item.interval)&&Number.isInteger(item.top)&&item.top>=0&&item.top<=200&&Number.isFinite(item.maximum)&&item.maximum>=0&&item.maximum<=1e9&&["bar","line","pie","donut"].includes(item.shape)&&typeof item.start==="string"&&typeof item.end==="string"&&Number.isFinite(new Date(item.start).getTime())&&Number.isFinite(new Date(item.end).getTime())&&(item.range!=="custom"||new Date(item.start)<new Date(item.end)))||!bounded(p.tables||{},500,item=>object(item)&&(item.size==="all"||Number.isInteger(item.size)&&item.size>=1&&item.size<=200)&&Number.isInteger(item.page)&&item.page>=1&&(!item.hidden||Array.isArray(item.hidden)&&item.hidden.length<=100&&item.hidden.every(label=>typeof label==="string"&&label.length<=400))&&(!item.columns||Array.isArray(item.columns)&&item.columns.length<=100&&item.columns.every(label=>typeof label==="string"&&label.length<=400))&&(!item.filters||bounded(item.filters,100,value=>typeof value==="string"&&value.length<=2000))&&(!item.sort||object(item.sort)&&Number.isInteger(item.sort.column)&&item.sort.column>=-1&&item.sort.column<=100&&typeof item.sort.descending==="boolean")))throw new Error(ui("圖表或表格設定格式無效"));
  if(typeof p.tab!=="string"||p.tab.length>80||!Number.isInteger(p.page)||p.page<1||!Array.isArray(p.tabOrder)||p.tabOrder.length>100||!p.tabOrder.every(key=>typeof key==="string"&&key.length<=80)||!Number.isInteger(p.tableSchema||1)||(p.tableSchema||1)>6)throw new Error(ui("Tab 或頁碼設定格式無效"));
  const backend=compatibleSettings(s,data.settings,data.mcp?.categories||{});backend.mcp_sources??={};backend.mcp_categories??={};backend.tool_descriptions??={};backend.replace_customizations=true;backend.recording=value.recording;
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
    confirmChange("匯入設定?",ui("將套用外觀, 觀察開關, 顯示數量, 圖表, 表格與介面文字; Jev 紀錄將設為 ")+ui(imported.recording?"啟用":"停用")+" · "+Object.keys(p.charts||{}).length+ui(" 個圖表 · ")+Object.keys(p.tables||{}).length+ui(" 個表格"),async()=>{
      const control=$("import-configuration");control.disabled=true;feedback("configuration-message",ui("正在匯入設定"),"pending");
      try{await post("/api/settings",imported.backend);const preferencesText=JSON.stringify(imported.preferences);localStorage.setItem(preferenceKey,preferencesText);feedback("configuration-message",ui("設定已匯入, 正在重新載入"));location.reload();}
      catch(error){feedback("configuration-message",error.message,"error");}finally{control.disabled=false;}
    },ui("確認匯入"));
  }catch(error){feedback("configuration-message",error instanceof SyntaxError?ui("設定內容不是有效的 JSON"):error.message,"error");}
});

const defaultTabOrder=[...document.querySelectorAll("[data-tab]")].map(tab=>tab.dataset.tab);
let tabOrder=[...new Set((Array.isArray(preferences.tabOrder)?preferences.tabOrder:[]).filter(id=>defaultTabOrder.includes(id)).concat(defaultTabOrder))];
function applyTabOrder(){const nav=document.querySelector(".tabs");for(const id of tabOrder)nav.append($("tab-"+id));}
function moveTab(id,target,after=false){
  if(id===target)return;const next=tabOrder.filter(key=>key!==id),index=next.indexOf(target);if(index<0)return;next.splice(index+(after?1:0),0,id);tabOrder=next;preferences.tabOrder=tabOrder;applyTabOrder();saveView();renderTabSettings();feedback("settings-message",ui("Tab 順序已更新"));
}
function dragTab(handle,id){
  let drag=null;
  const clear=()=>{for(const row of $("tab-order-rows").children)row.classList.remove("dragging","drop-before","drop-after");};
  handle.addEventListener("pointerdown",event=>{if(event.button!==0)return;drag={pointer:event.pointerId,start:event.clientY,target:null,after:false,moved:false};handle.setPointerCapture(event.pointerId);handle.focus();});
  handle.addEventListener("pointermove",event=>{
    if(!drag||drag.pointer!==event.pointerId)return;if(!drag.moved&&Math.abs(event.clientY-drag.start)<5)return;drag.moved=true;event.preventDefault();clear();handle.closest(".tab-order-row").classList.add("dragging");
    const content=handle.closest(".settings-content"),bounds=content.getBoundingClientRect();if(event.clientY<bounds.top+40)content.scrollTop-=12;else if(event.clientY>bounds.bottom-40)content.scrollTop+=12;
    const row=document.elementFromPoint(event.clientX,event.clientY)?.closest(".tab-order-row");drag.target=row&&row.dataset.tabOrder!==id?row.dataset.tabOrder:null;if(drag.target){const rect=row.getBoundingClientRect();drag.after=event.clientY>rect.top+rect.height/2;row.classList.add(drag.after?"drop-after":"drop-before");}
  });
  handle.addEventListener("pointerup",event=>{if(!drag||drag.pointer!==event.pointerId)return;const current=drag;drag=null;clear();if(current.moved&&current.target)moveTab(id,current.target,current.after);});
  for(const event of ["pointercancel","lostpointercapture"])handle.addEventListener(event,()=>{drag=null;clear();});
  handle.addEventListener("keydown",event=>{if(!["ArrowUp","ArrowDown","Home","End"].includes(event.key))return;event.preventDefault();const visible=tabOrder.filter(key=>!$("tab-"+key).hidden),index=visible.indexOf(id),target=event.key==="Home"?visible[0]:event.key==="End"?visible.at(-1):visible[index+(event.key==="ArrowUp"?-1:1)];if(target){moveTab(id,target,event.key==="ArrowDown"||event.key==="End");$("tab-order-rows").querySelector('[data-tab-order="'+id+'"] .drag-handle').focus();}});
}
function renderTabSettings(){
  const visible=tabOrder.filter(id=>!$("tab-"+id).hidden);
  replaceRows("tab-order-rows",...visible.map(id=>{const row=node("div",null,"setting-row tab-order-row"),handle=button("⠿",()=>{},"drag-handle");row.dataset.tabOrder=id;handle.setAttribute("aria-label",$("tab-"+id).textContent+" "+ui("拖曳調整順序"));handle.title=ui("拖曳調整順序, 或用方向鍵上下移動");row.append(handle,node("span",$("tab-"+id).textContent));dragTab(handle,id);return row;}));
}
$("tab-order-reset").addEventListener("click",()=>confirmChange("還原 Tab 順序?","全部 Tab 將回到預設順序",()=>{tabOrder=[...defaultTabOrder];preferences.tabOrder=tabOrder;applyTabOrder();saveView();renderTabSettings();feedback("settings-message",ui("Tab 順序已還原"));}));
$("observation-settings").addEventListener("click",openSettings);
$("settings-close").addEventListener("click",()=>$("settings-dialog").close());
$("dark-toggle").addEventListener("click",()=>{appearance.mode=document.documentElement.dataset.mode==="dark"?"light":"dark";applyAppearance();saveView();});
for(const el of document.querySelectorAll("[data-mode]"))el.addEventListener("click",()=>{appearance.mode=el.dataset.mode;applyAppearance();saveView();});
for(const [id,key]of [["theme-select","theme"],["accent-select","accent"],["font-size","font"]])$(id).addEventListener("change",()=>{if(!$(id).checkValidity()){applyAppearance();return;}appearance[key]=key==="font"?Number($(id).value):$(id).value;applyAppearance();saveView();feedback("settings-message",ui("外觀已更新"));});
systemDark.addEventListener("change",applyAppearance);
$("session-count").addEventListener("change",async()=>{const input=$("session-count");if(!input.checkValidity()||!Number.isInteger(Number(input.value))){input.value=data.settings.max_files;$("settings-message").textContent=ui("session 數量請輸入 1 - 5000 的整數");return;}input.disabled=true;try{await changeSettings({max_files:Number(input.value)});}catch{input.value=data.settings.max_files;}finally{input.disabled=data.settings.track_all;}});
$("detail-close").addEventListener("click",()=>$("detail-dialog").close());
$("detail-dialog").addEventListener("close",()=>{detail=null;detailHistory.length=0;const nav=$("detail-content").previousElementSibling;if(nav?.classList.contains("modal-tabs"))nav.remove();$("detail-content").replaceChildren();for(const key of tableViews.keys())if(key.startsWith("detail:"))tableViews.delete(key);updateDetailBack();});
$("detail-back").addEventListener("click",async()=>{const previous=detailHistory.pop();if(!previous)return;if(previous.view.kind==="jev")await openJev(previous.view.call,previous.view.thread,false,previous.scroll);else{openDetail(previous.view,false);$("detail-content").scrollTop=previous.scroll;}updateDetailBack();});
for(const dialog of document.querySelectorAll("dialog"))dialog.addEventListener("click",event=>{if(event.target!==dialog)return;const rect=dialog.getBoundingClientRect();if(event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom)dialog.close();});
for(const tab of document.querySelectorAll("[data-tab]")){
  tab.addEventListener("click",()=>switchTab(tab.dataset.tab));
  tab.addEventListener("keydown",event=>{if(!["ArrowLeft","ArrowRight","Home","End"].includes(event.key))return;event.preventDefault();const tabs=[...document.querySelectorAll("[data-tab]")].filter(t=>!t.hidden),index=tabs.indexOf(tab),next=event.key==="Home"?0:event.key==="End"?tabs.length-1:(index+(event.key==="ArrowRight"?1:tabs.length-1))%tabs.length;switchTab(tabs[next].dataset.tab);tabs[next].focus();});
}
for(const id of ["filter-type","filter-environment","filter-project","filter-trigger","filter-status","filter-reasoning","page-size"])$(id).addEventListener("change",()=>{page=1;lazyLimit=50;if(data){renderCodex();attachTables();}saveView();});
$("thread-search").addEventListener("input",()=>{page=1;lazyLimit=50;if(data){renderCodex();attachTables();}saveView();});
$("filter-git").addEventListener("change",()=>{if(data){renderGit();renderCharts();attachTables();}saveView();});
for(const id of ["filter-file-operation","filter-file-method","filter-file-project","filter-file-tool"])$(id).addEventListener("change",()=>{if(data){renderFiles();renderCharts();attachTables();}saveView();});
$("file-search").addEventListener("input",()=>{if(data){renderFiles();renderCharts();attachTables();}saveView();});
for(const id of ["filter-mcp-category","filter-mcp-server","filter-mcp-result"])$(id).addEventListener("change",()=>{mcpPage=1;if(data)renderMcp();attachTables();saveView();});
for(const [id,change]of [["mcp-prev",-1],["mcp-next",1]])$(id).addEventListener("click",()=>{mcpPage+=change;renderMcp();});
function loadMore(){if(!data||$("load-more").hidden)return;lazyLimit+=50;renderCodex();}
$("load-more").addEventListener("click",loadMore);
const lazyObserver=new IntersectionObserver(entries=>{if(entries.some(e=>e.isIntersecting))loadMore();},{rootMargin:"150px"});lazyObserver.observe($("load-more"));
pageInput($("page-number"),()=>{page=Number($("page-number").value);if(data){renderCodex();attachTables();}saveView();});
for(const [id,action]of [["page-first",()=>1],["page-prev",()=>page-1],["page-next",()=>page+1],["page-last",()=>pageCount]])$(id).addEventListener("click",()=>{page=action();renderCodex();saveView();});
$("window").addEventListener("change",()=>{version++;saveView();refresh();});
for(const id of viewInputs){const value=preferences.inputs?.[id],input=$(id);if(typeof value==="string"&&!['filter-project','filter-git'].includes(id)&&(input.tagName!=="SELECT"||[...input.options].some(option=>option.value===value)))input.value=value;}
page=Number.isInteger(preferences.page)?preferences.page:1;
if([...document.querySelectorAll("[data-tab]")].some(tab=>tab.dataset.tab===preferences.tab))switchTab(preferences.tab);
discoverCopy();applyCopy();applyTabOrder();groupSettings();fillDisplayOptions($("page-size"),preferences.inputs?.["page-size"]||display.table);
for(const id of ["activity-chart","chart","git-time-chart","web-time-chart","file-time-chart","monitor-refresh-chart","monitor-cpu-chart","monitor-read-chart","error-time-chart"])chartControls(id,true);
for(const id of ["top-tools","source-chart","model-chart","environment-chart","trigger-chart","mcp-action-chart","git-chart","skill-chart","check-chart","file-operation-chart","error-source-chart"])chartControls(id);
pageInput($("mcp-page-number"),()=>{mcpPage=Number($("mcp-page-number").value);renderMcp();});
pageInput($("docs-page-number"),()=>{docsPage=Number($("docs-page-number").value);renderToolDocs();});
addTabSettings();applyAppearance();schedule(10);refresh();
if(typeof ResizeObserver==="function"){const overviewObserver=new ResizeObserver(arrangeOverview);for(const panel of document.querySelector(".overview-panels").children)overviewObserver.observe(panel);}
let chartResizeTimer;addEventListener("resize",()=>{clearTimeout(chartResizeTimer);chartResizeTimer=setTimeout(()=>{if(data)renderCharts();},150);});
