async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);};
  await page.evaluate(()=>{
    switchTab('codex');const snapshot=structuredClone(data),sample=snapshot.codex.threads[0];snapshot.codex.threads=Array.from({length:12},(_,index)=>({...structuredClone(sample),thread_id:'partial-'+index,title:'Partial '+index,tokens:{...sample.tokens,total_tokens:1000-index},updated_at:sample.updated_at}));renderSnapshot(snapshot);
    tableStates['codex-rows']={...tableStates['codex-rows'],size:5,page:1};document.getElementById('page-size').value='5';changeTableSort('codex-rows',{column:tableViews.get('codex-rows').headers.indexOf('Total'),descending:true});page=2;renderCodex();attachTables();
    window.partialRows=targetTableRows(document.getElementById('codex-rows'));window.partialKeys=partialRows.map(row=>row.dataset.recordKey);window.partialPush=change=>{const snapshot=structuredClone(data);change(snapshot);eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:(eventVersion??0)+1,window:activeSourceWindow(),snapshot,logs:logData})}));};
  });
  await page.locator('#codex-rows tr').first().hover();
  await page.evaluate(()=>partialPush(snapshot=>{snapshot.codex.threads.find(thread=>thread.thread_id===partialKeys[0]).tokens.total_tokens=9000000;snapshot.codex.threads=snapshot.codex.threads.filter(thread=>thread.thread_id!==partialKeys[1]);snapshot.codex.threads.unshift({...structuredClone(snapshot.codex.threads[0]),thread_id:'partial-added',tokens:{total_tokens:10000000}});}));
  await page.waitForFunction(()=>!queuedSnapshot&&!snapshotFrame);
  const hovered=await page.evaluate(()=>{const rows=targetTableRows(document.getElementById('codex-rows')),view=tableViews.get('codex-rows');return {same:rows.length===partialRows.length&&rows.every((row,index)=>row===partialRows[index]),keys:rows.map(row=>row.dataset.recordKey),value:rowCells(rows[0])[view.headers.indexOf('Total')].textContent,page,held:view.updateGuard.held,contentHeld:view.updateGuard.contentHeld};});
  check(hovered.same&&hovered.value==='9,000,000'&&hovered.page===2&&hovered.held&&!hovered.contentHeld,'hover updates numeric cells and retains membership, page and DOM: '+JSON.stringify(hovered));
  await page.evaluate(()=>partialPush(snapshot=>snapshot.codex.threads.push({...structuredClone(snapshot.codex.threads.find(thread=>thread.thread_id===partialKeys[0])),tokens:{total_tokens:8000000}})));await page.waitForFunction(()=>!queuedSnapshot&&!snapshotFrame);
  check(await page.evaluate(()=>rowCells(partialRows[0])[tableViews.get('codex-rows').headers.indexOf('Total')].textContent==='9,000,000'),'ambiguous identities retain the last displayed value');
  await page.evaluate(()=>partialPush(snapshot=>snapshot.codex.threads=snapshot.codex.threads.filter((thread,index,rows)=>rows.findIndex(item=>item.thread_id===thread.thread_id)===index)));await page.waitForFunction(()=>!queuedSnapshot&&!snapshotFrame);
  await page.waitForFunction(()=>{partialRows[0].focus();return document.activeElement===partialRows[0];});await page.evaluate(()=>partialPush(snapshot=>snapshot.codex.threads.find(thread=>thread.thread_id===partialKeys[0]).tokens.total_tokens=9000001));await page.waitForFunction(()=>!queuedSnapshot&&!snapshotFrame);
  const focused=await page.evaluate(()=>({value:rowCells(partialRows[0])[tableViews.get('codex-rows').headers.indexOf('Total')].textContent,focused:document.activeElement===partialRows[0],held:tableViews.get('codex-rows').updateGuard.contentHeld,connected:partialRows[0].isConnected}));
  check(focused.value==='9,000,000'&&focused.focused,'focused row protects content: '+JSON.stringify(focused));
  await page.mouse.move(1,1);await page.locator('[data-tab="codex"]').focus();await page.waitForFunction(()=>!tableViews.get('codex-rows').pendingUpdate);
  check(await page.evaluate(()=>page===2&&targetTableRows(document.getElementById('codex-rows')).every(row=>row.dataset.recordKey!==partialKeys[1])),'resume reapplies latest membership and preserves requested page');
  await page.evaluate(()=>{window.dispatchEvent(new Event('blur'));setSnapshotState('refreshing');});
  check(await page.evaluate(()=>document.documentElement.dataset.wbMotion==='false'&&!document.querySelector('#codex-rows .wb-refresh-text')&&snapshotDisplayState==='refreshing'),'blur retains state without decoration');
  await page.evaluate(()=>{window.dispatchEvent(new Event('focus'));setSnapshotState('ready');});
  check(await page.evaluate(()=>document.documentElement.dataset.wbMotion==='true'&&appearance.reduceMotion===false),'focus restores effective motion without changing preference');
  await page.emulateMedia({reducedMotion:'reduce'});await page.evaluate(()=>setSnapshotState('refreshing'));check(await page.locator('#codex-rows .wb-refresh-text').count()===0,'system reduced motion retains plain text');await page.evaluate(()=>setSnapshotState('ready'));await page.emulateMedia({reducedMotion:'no-preference'});
  return {hovered,numericUpdates:true,membershipAndPageRetained:true,focusProtection:true,resumeLatest:true,backgroundMotion:true,systemReducedMotion:true};
}
