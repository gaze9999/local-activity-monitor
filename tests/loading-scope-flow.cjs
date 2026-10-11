async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);},origin=new URL(page.url()).origin;
  const snapshot=await page.evaluate(()=>{stopEventStream();return structuredClone(data);});
  check(snapshot.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')),'Synthetic fixture required');
  await page.addInitScript(()=>{window.fixtureEventSource=EventSource;window.EventSource=class extends fixtureEventSource{constructor(...args){super(...args);super.addEventListener('snapshot',()=>this.received=true);}set onerror(handler){super.onerror=event=>{if(!this.received)handler?.call(this,event);};}get onerror(){return super.onerror;}};});
  let release;const gate=new Promise(resolve=>release=resolve);
  const handler=async route=>{await gate;const body={version:1,window:new URL(route.request().url()).searchParams.get('window'),snapshot,logs:{events:[]}};await route.fulfill({contentType:'text/event-stream',body:'id: 1\nevent: snapshot\ndata: '+JSON.stringify(body)+'\n\n'});};
  await page.route(origin+'/api/events?*',handler);
  try{
    await page.evaluate(()=>localStorage.removeItem(preferenceKey));await page.goto(origin);
    await page.waitForFunction(()=>snapshotLoadingScopes.size>0);
    const initial=await page.evaluate(()=>{const root=$('view-overview'),visible=el=>getComputedStyle(el).visibility!=='hidden'&&el.getBoundingClientRect().height>0;return {scope:root.dataset.loadingScope,busy:root.getAttribute('aria-busy'),loaders:[...root.querySelectorAll('.wb-data-state:not([hidden])')].filter(visible).length,headerVisible:visible(root.querySelector('.section-head')),unknown:data?.updated_at??null};});
    check(initial.scope==='container'&&initial.busy==='true'&&initial.loaders===1&&initial.headerVisible&&!initial.unknown,'One initial container loader '+JSON.stringify(initial));
    await page.screenshot({path:'.local/loading-container-final.png'});
    const retry=await page.evaluate(()=>{setSnapshotState('error');const root=$('view-overview'),failed=root.querySelector(':scope>.wb-data-state').dataset.state;setLoadingScope('cards');const cards=root.dataset.loadingScope;setLoadingScope('container');setSnapshotState('loading');return {failed,cards,busy:root.getAttribute('aria-busy')};});
    check(retry.failed==='error'&&retry.cards==='cards'&&retry.busy==='true','Initial failure and retry '+JSON.stringify(retry));
    release();await page.waitForFunction(()=>data?.updated_at&&snapshotLoadingScope==='cards');await page.evaluate(()=>stopEventStream());
    const states=[];
    for(const width of [1366,390,320]){
      await page.setViewportSize({width,height:900});
      const result=await page.evaluate(()=>{const root=$('view-overview'),panel=$('activity-chart').closest('.panel'),button=panel.querySelector('button'),graph=$('activity-chart');button?.focus();const child=graph.firstElementChild;setLoadingScope('container');setSnapshotState('refreshing');const container=root.dataset.loadingScope;setLoadingScope('cards');const cards=root.dataset.loadingScope,busy=panelDataStates.get(panel).boundary.getAttribute('aria-busy');setSnapshotState('ready');return {container,cards,busy,identity:child===graph.firstElementChild,focus:!button||document.activeElement===button,rootBusy:root.getAttribute('aria-busy'),overflow:document.documentElement.scrollWidth>innerWidth};});
      check(result.container==='container'&&result.cards==='cards'&&result.busy==='true'&&result.rootBusy==='false'&&result.identity&&result.focus&&!result.overflow,'Scope switches preserve content/focus '+JSON.stringify(result));states.push({width,...result});
    }
    await page.setViewportSize({width:1366,height:900});
    await page.screenshot({path:'.local/loading-cards-final.png'});
    const metrics=await page.evaluate(()=>{
      data.monitor.history=[1,2,3].map((value,index)=>({time:new Date(Date.now()-(3-index)*60000).toISOString(),refresh_ms:value*1000,cpu_ms:value*100}));chartDrawing=true;try{renderCharts();}finally{chartDrawing=false;}
      const timings=['monitor-refresh-chart','monitor-cpu-chart','overview-refresh-chart','overview-cpu-chart'].map(id=>({id,total:chartViews.get(id)?.renderData?.args[4],values:[...chartViews.get(id).statisticsRoot.querySelectorAll('.wb-statistic-value')].map(cell=>cell.textContent)}));
      const root=$('view-tools'),host=node('section',null,'panel'),table=node('table'),rows=[['2026/10/11 12:02:00','50%','20','30','2','200','2000','--'],['2026/10/11 12:01:00','25%','10','15','1','100','1000','8']].map(values=>{const row=node('tr');values.forEach((value,index)=>{const cell=node('td',value);cell.dataset.columnIndex=index;row.append(cell);});table.append(row);return row;}),id='metric-rule-rows',headers=['最近活動','使用率','平均耗時 (ms)','P99 (ms)','平均 Token / 秒','資料整理耗時 (ms)','耗時 (ms)','呼叫次數'];
      host.append(table);root.append(host);tableViews.set(id,{table,rows,headers,title:'Fixture'});const values=tableMetrics(id);syncMetricSources();const excluded=[1,2,3,4,5].every(index=>!metricSources.has('table:'+id+':'+index+':sum')),execution=metricSources.get('table:'+id+':6:sum')?.[2](),count=values[7],missing=metricValue('table:'+id+':1:sum')[1];
      const bytes=statisticLabels(statistics([100,200]),'bytes','Fixture'),percent=statisticLabels(statistics([25,50]),'%','Fixture');
      tableViews.delete(id);host.remove();syncMetricSources();return {timings,excluded,execution,count,missing,bytes:bytes.querySelectorAll('.wb-statistic-label')[2]?.textContent,percent:percent.children.length,slots:[...percent.children].map(item=>item.dataset.statisticSlot)};
    });
    check(metrics.excluded&&metrics.execution===3000&&metrics.count.mean===8&&metrics.count.latest===null&&metrics.count.sum===8&&metrics.missing==='--'&&metrics.bytes==='樣本數'&&metrics.percent===3&&metrics.slots.join(',')==='0,1,3','Semantic aggregation '+JSON.stringify(metrics));
    check(metrics.timings.every(item=>item.total===false&&item.values.length===3&&item.values[2]==='3 個樣本'),'Performance and overview statistics '+JSON.stringify(metrics.timings));
    return {initial,retry,states,metrics};
  }finally{release();await page.unroute(origin+'/api/events?*',handler);await page.evaluate(()=>stopEventStream());}
}
