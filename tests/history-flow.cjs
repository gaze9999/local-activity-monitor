async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  if(!await page.evaluate(()=>data.codex.threads.every(t=>t.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  const api=await page.evaluate(async()=>{
    const values=[],first=await fetch('/api/history?namespace=activity&section=mcp&limit=2').then(r=>r.json());let part=first;
    while(true){values.push(...part.items);if(!part.next_cursor)break;part=await fetch('/api/history?namespace=activity&section=mcp&limit=2&cursor='+encodeURIComponent(part.next_cursor)).then(r=>r.json());}
    const invalid=await fetch('/api/history?namespace=activity&section=mcp&cursor=bad');
    return {total:first.total,count:values.length,unique:new Set(values.map(v=>JSON.stringify([v.server,v.thread_id,v.call_id,v.index,v.timestamp]))).size,invalid:invalid.status};
  });
  if(!api.total||api.count!==api.total||api.unique!==api.total||api.invalid!==400)throw Error(JSON.stringify(api));
  await page.evaluate(()=>{display.table=2;openHistory('activity','mcp');});
  await page.waitForFunction(()=>detail.historyPage&&!detail.historyLoading);
  const first=await page.locator('#detail-content tbody').textContent();
  await page.locator('#detail-content').getByRole('button',{name:'下一頁',exact:true}).click();
  await page.waitForFunction(()=>detail.historyIndex===1&&!detail.historyLoading);
  if(await page.locator('#detail-content tbody').textContent()===first)throw Error('Page did not advance');
  await page.locator('#detail-content').getByRole('button',{name:'上一頁',exact:true}).click();
  await page.waitForFunction(()=>detail.historyIndex===0&&!detail.historyLoading);
  if(await page.locator('#detail-content tbody').textContent()!==first)throw Error('Page did not restore');
  await page.locator('#detail-content').getByRole('button',{name:'最後一頁',exact:true}).click();
  await page.waitForFunction(()=>detail.historyIndex===Math.ceil(detail.historyPage.total/detail.historyLimit)-1&&!detail.historyLoading);
  if(!await page.locator('#detail-content').getByRole('button',{name:'下一頁',exact:true}).isDisabled())throw Error('Last page boundary');
  await page.locator('#detail-content').getByRole('button',{name:'上一頁',exact:true}).click();await page.waitForFunction(()=>!detail.historyLoading);
  await page.locator('#detail-content').getByRole('button',{name:'第一頁',exact:true}).click();await page.waitForFunction(()=>detail.historyIndex===0&&!detail.historyLoading);
  if(await page.locator('#detail-content tbody').textContent()!==first)throw Error('First page after last');
  await page.locator('#detail-dialog').evaluate(el=>el.close());
  await page.route('**/api/history?**',route=>route.fulfill({status:503,body:'Unavailable'}));
  await page.evaluate(()=>openHistory('activity','mcp'));await page.waitForFunction(()=>detail.historyError&&!detail.historyLoading);
  await page.unroute('**/api/history?**');await page.locator('#detail-dialog').getByRole('button',{name:'重新整理',exact:true}).click();
  await page.waitForFunction(()=>detail.historyPage&&!detail.historyLoading);await page.locator('#detail-dialog').evaluate(el=>el.close());
  if(errors.length)throw Error(JSON.stringify(errors));return {api,paging:true,errorRetry:true,errors};
}
