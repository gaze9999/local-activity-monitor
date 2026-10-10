async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&eventStreamReady&&!busy);
  if(!await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  const check=(value,message)=>{if(!value)throw Error(message);};
  const push=async values=>{
    await page.evaluate(values=>{
      const snapshot=structuredClone(data);
      if(values.thread){const thread=snapshot.codex.threads.find(thread=>thread.thread_id===values.thread);thread.tokens.total_tokens=values.total;}
      if(values.sql){snapshot.codex.sqlite={events:values.sql,total:values.sql.length};}
      eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:(eventVersion??0)+1,window:activeSourceWindow(),snapshot,logs:logData})}));
    },values);
    await page.waitForFunction(()=>!busy&&!queuedSnapshot);
  };
  try{
    await page.evaluate(()=>{locale='zh-TW';applyLanguage();switchTab('codex');changeTableSort('codex-rows',{column:tableViews.get('codex-rows').headers.indexOf('Total'),descending:true});window.fixtureOpenDetail=openDetail;openDetail=selected=>{window.fixtureClicked=selected.thread?.thread_id;};});
    const before=await page.evaluate(()=>{const rows=targetTableRows(document.getElementById('codex-rows'));window.fixtureRow=rows[0];fixtureRow.dataset.fixtureTarget="true";return {first:rowCells(rows[0])[1].textContent,last:rowCells(rows.at(-1))[1].textContent};});
    await page.locator('#codex-rows tr[data-fixture-target="true"]').hover();
    await push({thread:before.last,total:9_000_000});
    check(await page.evaluate(()=>targetTableRows(document.getElementById('codex-rows'))[0]===fixtureRow&&tableViews.get('codex-rows').pendingUpdate),'mouse hover retains row identity '+JSON.stringify(await page.evaluate(()=>({same:targetTableRows(document.getElementById('codex-rows'))[0]===fixtureRow,pending:tableViews.get('codex-rows').pendingUpdate,hovered:tableViews.get('codex-rows').hovered,root:tableViews.get('codex-rows').interactionRoot?.className,actual:document.getElementById('codex-rows').closest('.table-disclosure')?.className}))));
    await page.locator('#codex-rows tr[data-fixture-target="true"]').click();
    check(await page.evaluate(()=>fixtureClicked)===before.first,'click opens the displayed record');
    await page.mouse.move(0,0);await page.locator('[data-tab="codex"]').focus();
    await page.waitForFunction(id=>rowCells(targetTableRows(document.getElementById('codex-rows'))[0])[1].textContent===id,before.last);
    await page.waitForFunction(()=>{const row=targetTableRows(document.getElementById('codex-rows'))[0];row.focus();return document.activeElement===row;});
    const focused=await page.evaluate(()=>{window.fixtureRow=targetTableRows(document.getElementById('codex-rows'))[0];return data.codex.threads.find(thread=>thread.thread_id!==rowCells(fixtureRow)[1].textContent).thread_id;});
    await push({thread:focused,total:10_000_000});
    check(await page.evaluate(()=>targetTableRows(document.getElementById('codex-rows'))[0]===fixtureRow&&document.activeElement===fixtureRow),'keyboard focus retains row identity '+JSON.stringify(await page.evaluate(()=>({same:targetTableRows(document.getElementById('codex-rows'))[0]===fixtureRow,active:document.activeElement.tagName,tabIndex:fixtureRow.tabIndex,connected:fixtureRow.isConnected,contains:tableViews.get('codex-rows').interactionRoot.contains(document.activeElement),pending:tableViews.get('codex-rows').pendingUpdate}))));
    await page.locator('[data-tab="codex"]').focus();
    await page.waitForFunction(id=>rowCells(targetTableRows(document.getElementById('codex-rows'))[0])[1].textContent===id,focused);
    await page.evaluate(()=>switchTab('data'));await page.locator('#tab-sqlite').click();
    const sql=[1,2,3].map(index=>({id:'synthetic-'+index,timestamp:new Date(Date.now()+index*1000).toISOString(),statement:'SELECT',operation:'read',engine:'SQLite',recognition:'diagnostic_log',record_id:index}));
    await push({sql});
    await page.locator('#sqlite-rows tr').first().hover();
    const sqlBefore=await page.evaluate(()=>{window.fixtureSqlRow=targetTableRows(document.getElementById('sqlite-rows'))[0];return fixtureSqlRow.textContent;});
    await push({sql:[...sql,{...sql[0],id:'synthetic-latest',timestamp:new Date(Date.now()+10000).toISOString(),statement:'INSERT',operation:'write'}]});
    check(await page.evaluate(()=>targetTableRows(document.getElementById('sqlite-rows'))[0]===fixtureSqlRow)&&await page.locator('#sqlite-rows tr').first().textContent()===sqlBefore,'generic table retains mouse target');
    await page.mouse.move(0,0);await page.locator('[data-tab="data"]').focus();
    await page.waitForFunction(()=>targetTableRows(document.getElementById('sqlite-rows'))[0].textContent.includes('INSERT'));
    check(!await page.locator('#resume-refresh').count(),'no manual refresh button');
    return {mouseRetained:true,clickIdentity:true,keyboardRetained:true,latestOnLeave:true,sqlTableRetained:true,noRefreshButton:true};
  }finally{
    await page.evaluate(()=>{if(window.fixtureOpenDetail){openDetail=fixtureOpenDetail;delete window.fixtureOpenDetail;}});
  }
}
