async page=>{
  const check=(ok,message)=>{if(!ok)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic fixture required');
  await page.route('**/api/codex/file?*',route=>route.fulfill({json:{health:'ok',bytes:0,modified_at:'2026-10-10T00:00:00Z'}}));
  const first=await page.evaluate(async()=>{
    stopEventStream();switchTab('overview');const thread=data.codex.threads[0],at=data.updated_at;
    data.codex.file_activity={total:1000,events:Array.from({length:1000},(_,i)=>({thread_id:thread.id,call_id:'file-'+i,path:'file-'+i+'.js',workdir:'D:/synthetic/src',timestamp:at,completed_at:at,operation:i%2?'write':'read',tool:'exec_command',nested:true,read_bytes:i%2?null:0,write_bytes:i%2?i:null,submitted_utf8_bytes:null,duration_ms:i}))};
    const selected=activeSourceWindow();fileSnapshotAttempts.set(selected,Date.now());fileSnapshots.set(selected,{files:[],total:1000,checked_at:at});tableStates['file-rows']={size:10,page:1};
    const body=$('file-rows'),added=[];const observer=new MutationObserver(records=>{for(const record of records)for(const row of record.addedNodes)if(row.nodeType===1&&row.tagName==='TR')added.push(row);});observer.observe(body,{childList:true});
    renderFiles();await Promise.resolve();const hiddenMounted=body.children.length;const start=performance.now();switchTab('files');const switchMs=performance.now()-start;await Promise.resolve();observer.disconnect();
    return {hiddenMounted,visibleMounted:body.children.length,addedRows:added.length,sourceRows:tableViews.get('file-rows').rows.length,switchMs,rankSource:tableViews.get('file-rank-rows').rows.length,rankMounted:$('file-rank-rows').children.length};
  });
  check(first.sourceRows===1000&&first.rankSource===1000,'all source records remain available');check(first.hiddenMounted===10&&first.visibleMounted===10&&first.rankMounted===10&&first.addedRows<=20,'initial and hidden preparation mounts only the page');
  await page.evaluate(()=>moveTablePage('file-rows',1));await page.waitForFunction(()=>!$('file-rows').dataset.wbPageMotion);check(await page.evaluate(()=>tableStates['file-rows'].page===2&&$('file-rows').children.length===10),'next page remains bounded');
  const selected=await page.evaluate(()=>{const row=$('file-rows').firstElementChild;row.dispatchEvent(new KeyboardEvent('keydown',{key:'Enter',bubbles:true}));return detail.event.call_id;});
  check(!!selected,'Enter opens current record');await page.waitForFunction(()=>detail.fileMetadata?.bytes===0);check(await page.evaluate(selected=>detail.event.call_id===selected&&detail.fileMetadata.bytes===0,selected),'record identity and confirmed zero survive metadata read');
  await page.evaluate(()=>$('detail-dialog').close());
  const filters=page.locator('.filter-section').filter({has:page.locator('#file-search')});if(!await filters.evaluate(section=>section.open))await filters.locator(':scope>summary').click();
  await page.locator('#file-search').fill('file-999.js');await page.waitForFunction(()=>$('file-rows').children.length===1);check(await page.evaluate(()=>filteredFiles().length===1&&tableViews.get('file-rows').rows.length===1&&tableStates['file-rows'].page===1),'search clamps page without losing source records');
  await page.evaluate(()=>$('file-rows').firstElementChild.dispatchEvent(new KeyboardEvent('keydown',{key:' ',bubbles:true})));check(await page.evaluate(()=>detail.event.call_id==='file-999'),'Space opens the filtered data ID');await page.evaluate(()=>$('detail-dialog').close());
  await page.locator('#file-search').fill('');await page.waitForFunction(()=>$('file-rows').children.length===10);
  const retained=await page.evaluate(()=>{const row=$('file-rows').firstElementChild;renderFiles();attachTables(true);return $('file-rows').firstElementChild===row;});check(retained,'unchanged records retain visible row DOM');
  const sizes=[];for(const width of [900,390,320]){await page.setViewportSize({width,height:width===900?1600:900});const value=await page.evaluate(()=>{const root=document.documentElement;return {width:innerWidth,overflow:root.scrollWidth>root.clientWidth+1,rows:$('file-rows').children.length};});check(!value.overflow&&value.rows===10,'responsive file view remains bounded');sizes.push(value);}
  return {first,enter:true,space:true,zero:true,search:true,retained,sizes};
}
