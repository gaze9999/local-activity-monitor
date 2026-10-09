async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  await page.emulateMedia({reducedMotion:'no-preference'});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.evaluate(()=>{
    if(!data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');
    appearance.reduceMotion=false;applyAppearance();display.table=2;display.options=[2,...display.options.filter(value=>value!==2)];fillDisplayOptions($('page-size'),2);tableStates['codex-rows']={size:2,page:1};$('page-size').value='2';page=1;navigateToTab('codex','view-codex');renderCodex();attachTables();
    window.pageAnimationLog=[];window.originalPageAnimate=Element.prototype.animate;
    Element.prototype.animate=function(frames,options){if(this.tagName==='TBODY')pageAnimationLog.push({id:this.id,frames});return originalPageAnimate.call(this,frames,options);};
  });
  const move=id=>page.evaluate(id=>document.getElementById(id).click(),id);
  const settle=()=>page.waitForFunction(()=>![...document.querySelectorAll('tbody')].some(body=>body.getAnimations().length));
  const direction=id=>page.evaluate(id=>document.getElementById(id).dataset.wbPageMotion,id);
  const layouts=[];
  try{
    await move('page-next');if(await direction('codex-rows')!=='1')throw Error('Conversation next slide');
    await move('page-prev');if(await direction('codex-rows')!=='-1')throw Error('Conversation previous slide');
    await page.evaluate(()=>{for(const id of ['page-next','page-next','page-prev','page-last','page-first','page-next'])$(id).click();});await settle();
    const current=await page.evaluate(()=>({page,ids:targetTableRows($('codex-rows')).map(row=>row.querySelector('.thread-id').textContent),source:data.codex.threads.map(thread=>thread.thread_id)}));if(current.page!==2||current.ids.length!==2||new Set(current.ids).size!==2||!current.ids.every(id=>current.source.includes(id)))throw Error('Rapid paging '+JSON.stringify(current));
    for(const width of [1440,900,390,320]){
      await page.setViewportSize({width,height:1100});await move('page-last');await settle();
      const layout=await page.evaluate(()=>{
        const body=$('codex-rows'),wrap=body.closest('.table-wrap'),footer=$('page-first').closest('.pagination'),controls=footer.querySelector('div'),summary=footer.querySelector('span'),a=controls.getBoundingClientRect(),b=summary.getBoundingClientRect(),c=footer.getBoundingClientRect(),buttons=[...controls.querySelectorAll('button')];
        const centers=buttons.map(button=>{const rect=button.getBoundingClientRect(),icon=button.querySelector('svg').getBoundingClientRect();return {x:rect.x+rect.width/2-icon.x-icon.width/2,y:rect.y+rect.height/2-icon.y-icon.height/2,height:rect.height};});
        return {summaryOverlap:a.left<b.right&&a.right>b.left&&a.top<b.bottom&&a.bottom>b.top,controlsOverflow:a.left<c.left-1||a.right>c.right+1,svg:buttons.every(button=>button.querySelector('svg')&&button.getAttribute('aria-label')),documentOverflow:document.documentElement.scrollWidth>innerWidth+1,clip:wrap.style.clipPath,centers};
      });if(layout.summaryOverlap||layout.controlsOverflow||!layout.svg||layout.documentOverflow||layout.clip||layout.centers.some(center=>Math.abs(center.x)>=.5||Math.abs(center.y)>=.5||Math.abs(center.height-layout.centers[0].height)>=.5))throw Error('Pagination layout '+JSON.stringify({width,...layout}));layouts.push({width,...layout});await move('page-first');await settle();
    }
    await page.setViewportSize({width:1440,height:1000});
    await page.evaluate(()=>{const root=node('section',null,'panel');root.id='generic-paging-test';const rows=Array.from({length:7},(_,index)=>{const row=node('tr');cell(row,'Item '+index);cell(row,index);return row;});root.append(table(['項目','數值'],rows,'分頁測試','test:paging'));$('view-codex').append(root);attachTables();});
    const generic=await page.evaluate(()=>{const root=$('generic-paging-test'),body=root.querySelector('tbody'),key=root.querySelector('table').dataset.tableKey;tableStates[key]={...tableStates[key],size:2,page:1};paginateTable(key);const controls=tableViews.get(key);controls.next.click();return {key,direction:body.dataset.wbPageMotion,svg:controls.footer.querySelectorAll('button>svg').length};});if(generic.direction!=='1'||generic.svg!==4)throw Error('Generic paging '+JSON.stringify(generic));
    await page.evaluate(()=>{const view=tableViews.get($('generic-paging-test').querySelector('table').dataset.tableKey);view.prev.click();});await settle();
    await page.evaluate(()=>openHistory('activity','mcp'));await page.waitForFunction(()=>detail.historyPage&&!detail.historyLoading);
    await page.locator('#detail-content').getByRole('button',{name:'下一頁',exact:true}).click();await page.waitForFunction(()=>detail.historyIndex===1&&!detail.historyLoading);await settle();
    if(!await page.locator('#detail-content').getByRole('button',{name:'下一頁',exact:true}).evaluate(el=>el===document.activeElement))throw Error('History paging lost focus');
    await page.locator('#detail-content').getByRole('button',{name:'上一頁',exact:true}).click();await page.waitForFunction(()=>detail.historyIndex===0&&!detail.historyLoading);await settle();
    const remote=await page.evaluate(()=>pageAnimationLog.filter(item=>!item.id).map(item=>item.frames[0].transform));if(!remote.includes('translateX(100%)')||!remote.includes('translateX(-100%)'))throw Error('History slides '+JSON.stringify(remote));
    const remoteSvg=await page.locator('#detail-content .pagination [data-wb-icon]>svg').count();if(remoteSvg!==4)throw Error('History SVG controls');
    await page.locator('#detail-dialog').evaluate(el=>el.close());
    await page.evaluate(()=>{docsMode='help';docsPage=1;tableStates['docs-rows']={size:2,page:1};openSettings();document.querySelector('[data-modal-key="介面與說明"]').click();renderToolDocs();});
    await move('docs-next');if(await direction('docs-rows')!=='1')throw Error('Documentation slide');await move('docs-prev');await settle();await page.locator('#settings-dialog').evaluate(el=>el.close());
    await page.emulateMedia({reducedMotion:'reduce'});await move('page-next');if(await direction('codex-rows'))throw Error('Reduced motion ignored');
    if(errors.length)throw Error(JSON.stringify(errors));return {passed:true,conversation:true,generic:true,history:true,documentation:true,reducedMotion:true,layouts,errors};
  }finally{await page.evaluate(()=>{Element.prototype.animate=originalPageAnimate;delete window.originalPageAnimate;delete window.pageAnimationLog;for(const [key,view]of tableViews)if(view.table.closest('#generic-paging-test')){WorkbenchUI.cancelChildMotion(view.body);view.disclosure?.destroy();tableViews.delete(key);}$('generic-paging-test')?.remove();});await page.emulateMedia({reducedMotion:'no-preference'});}
}
