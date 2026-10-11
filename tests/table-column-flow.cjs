async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);},errors=[],onError=error=>errors.push(error.message);
  page.on('pageerror',onError);
  const columns=()=>page.evaluate(()=>columnOrder('codex-rows').map(index=>tableViews.get('codex-rows').headers[index]));
  const option=key=>page.locator('#table-columns [data-column-key="'+key+'"]');
  const header=key=>page.locator('table:has(#codex-rows) th[data-column-key="'+key+'"]');
  const drag=async(source,target,horizontal=false,release=true)=>{
    await source.scrollIntoViewIfNeeded();const from=await source.boundingBox(),to=await target.boundingBox();
    await source.evaluate(el=>el.addEventListener('pointerdown',event=>el.testPointerId=event.pointerId,{once:true}));
    await page.mouse.move(from.x+from.width/2,from.y+from.height/2);await page.mouse.down();
    await page.mouse.move(horizontal?to.x+8:to.x+to.width/2,horizontal?to.y+to.height/2:to.y+2,{steps:12});
    check(await page.locator('.column-drag-preview').count()===1,'drag preview shown');
    if(!horizontal)check(await page.locator('#table-dialog>.column-drag-preview').count()===1,'settings preview stays in dialog');
    if(release)await page.mouse.up();
  };
  try{
    await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length&&!busy);
    check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'synthetic fixture required');
    await page.evaluate(()=>localStorage.removeItem(preferenceKey));await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length&&!busy);
    await page.evaluate(()=>{stopEventStream();switchTab('codex');});
    check(await page.evaluate(()=>{const view=tableViews.get('codex-rows'),sort=tableSort('codex-rows');return view.headerRow.firstElementChild.dataset.columnKey==='最近活動時間'&&view.headers[sortColumn('codex-rows',sort)]==='最近活動時間'&&sort.descending;}),'default recent activity is first and sorted newest first');
    check(!await page.locator('#layout-drag-toggle').isChecked(),'layout drag is off');
    const table=page.locator('#codex-rows').locator('xpath=ancestor::section[contains(@class,"panel")][1]'),open=()=>table.locator('.chart-setting-button').click();
    await open();check(await page.locator('#table-dialog .column-options>p').count()===0,'operation copy removed');
    const before=await columns(),checked=await option('Total').locator('input').isChecked();
    await drag(option('Total'),option('最近活動時間'));
    check((await columns())[0]==='Total'&&await option('Total').locator('input').isChecked()===checked,'settings drag persists order without toggling visibility');
    check(!await page.locator('#layout-drag-toggle').isChecked(),'settings drag independent of global toggle');
    await option('Total').focus();await option('Total').press('ArrowDown');
    check((await columns())[1]==='Total'&&await option('Total').evaluate(el=>el===document.activeElement),'keyboard reorder retains focus');
    await option('Total').press('Home');check((await columns())[0]==='Total','Home moves to first');
    await option('Thread ID').focus();await option('Thread ID').press('Home');
    check((await columns())[0]==='Thread ID'&&!await option('Thread ID').locator('input').isChecked()&&await header('Thread ID').count()===0,'hidden column reorder preserves visibility');
    await option('Thread ID').press('End');check((await columns()).at(-1)==='Thread ID','End moves hidden column to last');
    const cancelled=await columns();await drag(option('狀態'),option('Total'),false,false);await page.keyboard.press('Escape');
    check(await page.locator('.column-drag-preview').count()===0&&await page.locator('#table-dialog').evaluate(el=>el.open),'Escape cancels drag without closing settings');
    await page.mouse.up();check(JSON.stringify(await columns())===JSON.stringify(cancelled),'cancelled settings drag does not commit');
    await drag(option('狀態'),option('Total'),false,false);await page.locator('#table-dialog').evaluate(dialog=>dialog.close());await page.mouse.up();
    const closedOrder=await columns(),closedPreviews=await page.locator('.column-drag-preview').count();check(JSON.stringify(closedOrder)===JSON.stringify(cancelled)&&closedPreviews===0,'closing settings cancels drag '+JSON.stringify({closedOrder,cancelled,closedPreviews}));await open();
    const shown=await option('狀態').locator('input').isChecked();await option('狀態').locator('label').click();
    check(await option('狀態').locator('input').isChecked()!==shown,'label click still toggles visibility');await option('狀態').locator('input').check();
    await page.locator('#table-settings-close').click();await page.locator('#layout-drag-toggle').check();
    const sort=await page.evaluate(()=>JSON.stringify(tableSort('codex-rows'))),cancelledHeader=await columns();
    await drag(header('對話名稱'),header('Total'),true,false);await page.keyboard.press('Escape');await page.mouse.up();
    check(JSON.stringify(await columns())===JSON.stringify(cancelledHeader)&&await page.evaluate(()=>JSON.stringify(tableSort('codex-rows')))===sort,'Escape cancels header order and sort');
    await drag(header('對話名稱'),header('Total'),true);
    check((await columns())[0]==='對話名稱'&&await page.evaluate(()=>JSON.stringify(tableSort('codex-rows')))===sort,'header drop reorders without sorting');
    const capturedOrder=await columns();await drag(header('Total'),header('對話名稱'),true,false);
    await header('Total').evaluate(el=>el.releasePointerCapture(el.testPointerId));await page.mouse.up();
    check(JSON.stringify(await columns())===JSON.stringify(capturedOrder)&&await page.locator('.column-drag-preview').count()===0,'lost pointer capture cancels header drag');
    await header('Total').locator('button').click();check(await page.evaluate(()=>JSON.stringify(tableSort('codex-rows')))!==sort,'next intentional click sorts');
    await page.mouse.move(10,10);await page.waitForFunction(()=>JSON.stringify([...$('codex-rows').children].filter(row=>!row.classList.contains('wb-motion-exit')).map(row=>row.dataset.recordKey))===JSON.stringify(tableRecords.get('codex-rows').slice(0,Number($('page-size').value)).map(thread=>thread.thread_id)));
    await header('Total').locator('button').press('Alt+End');
    check(await page.evaluate(()=>tableViews.get('codex-rows').headerRow.lastElementChild.dataset.columnKey==='Total'),'header keyboard uses visible columns');
    const persisted=await columns(),ids=await page.locator('#codex-rows>tr').evaluateAll(rows=>rows.map(row=>row.dataset.recordKey));check(ids.length>0&&ids.every(Boolean)&&new Set(ids).size===ids.length,'row IDs are present and unique');
    await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length&&!busy);await page.evaluate(()=>{stopEventStream();switchTab('codex');});
    await page.mouse.move(10,10);await page.waitForFunction(()=>!tableViews.get('codex-rows')?.pendingUpdate);
    check(JSON.stringify(await columns())===JSON.stringify(persisted),'column order restored after reload');
    const restoredIds=await page.locator('#codex-rows>tr').evaluateAll(rows=>rows.map(row=>row.dataset.recordKey));check(JSON.stringify(restoredIds)===JSON.stringify(ids),'row identities retained '+JSON.stringify({ids,restoredIds}));
    check(await page.locator('.column-drag-preview,.column-dragging,.column-drop-before,.column-drop-after').count()===0,'drag visuals cleaned');
    const layouts=[];await open();
    for(const width of [1366,900,390,320]){
      await page.setViewportSize({width,height:900});const layout=await page.evaluate(()=>{const root=document.getElementById('table-columns'),dialog=document.getElementById('table-dialog'),rows=[...root.children].map(el=>el.getBoundingClientRect());return {overflow:document.documentElement.scrollWidth>innerWidth,inside:rows.every(row=>row.left>=0&&row.right<=innerWidth+1),overlap:rows.some((row,index)=>index&&row.top<rows[index-1].bottom-1),scroll:dialog.querySelector('.settings-content').scrollHeight>dialog.querySelector('.settings-content').clientHeight};});
      check(!layout.overflow&&layout.inside&&!layout.overlap,'column settings layout '+width+' '+JSON.stringify(layout));layouts.push({width,...layout});
    }
    await page.locator('#table-settings-close').click();
    check(errors.length===0,'browser errors '+errors.join('|'));
    const fallback=await page.evaluate(()=>{
      const results=[];
      for(const present of [true,false]){
        const key='synthetic-recent-'+present,headers=['對話','建立時間',...(present?['最近活動時間']:[])],host=node('section',null,'panel'),rows=latest=>[['A','2026/10/11 08:00:00',latest?'2026/10/11 09:00:00':'--'],['B','2026/10/10 08:00:00',latest?'2026/10/11 12:00:00':'--']].map(values=>{const row=node('tr');row.dataset.recordKey=values[0];for(const value of values.slice(0,headers.length))cell(row,value);return row;});
        host.append(node('h3','Synthetic activity'),table(headers,rows(present),'Synthetic activity',key));document.body.append(host);attachTables(false,host);
        const view=tableViews.get(key),initialSort=tableSort(key),initial={header:view.headers[sortColumn(key,initialSort)],descending:initialSort.descending,first:view.headerRow.firstElementChild.dataset.columnKey,rows:targetTableRows(view.body).map(row=>row.dataset.recordKey)};
        if(present)replaceRows(key,...rows(false));
        const missingSort=tableSort(key),missing={header:view.headers[sortColumn(key,missingSort)],descending:missingSort.descending,rows:targetTableRows(view.body).map(row=>row.dataset.recordKey)};
        results.push({present,initial,missing});view.updateGuard?.destroy();view.disclosure?.destroy();tableViews.delete(key);delete tableStates[key];host.remove();
      }
      return results;
    });
    check(fallback[0].initial.header==='最近活動時間'&&fallback[0].initial.first==='最近活動時間'&&fallback[0].initial.descending&&fallback[0].initial.rows.join('|')==='B|A','recent activity preferred over creation time');
    check(fallback.every(item=>item.missing.header==='建立時間'&&item.missing.descending&&item.missing.rows.join('|')==='A|B'),'missing field or records use existing time sort');
    check(fallback[1].initial.first==='對話','missing field keeps existing column order');
    return {columns:before.length,recentDefault:true,fallback,settingsPointer:true,settingsKeyboard:true,headerPointer:true,escape:true,closeCancel:true,captureCancel:true,sortClick:true,persistence:true,rowIds:true,layouts};
  }finally{page.off('pageerror',onError);}
}
