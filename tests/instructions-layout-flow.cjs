async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);},errors=[],onError=error=>errors.push(error.message);
  page.on('pageerror',onError);let mode='documents',requests=0;
  const documents=[{file:'synthetic/global/AGENTS.md',text:'# Demo instructions\n\n- Keep stable IDs\n',modified_at:'2026-10-11T00:00:00Z',read_bytes:0},{file:'synthetic/project/AGENTS.md',text:'# Project instructions',modified_at:'2026-10-11T00:00:00Z',read_bytes:22}];
  const route=async request=>{requests++;await request.fulfill({status:mode==='error'?503:200,json:{documents:mode==='empty'?[]:documents}});};
  await page.route('**/api/codex/instructions?*',route);
  try{
    await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length&&!busy);
    check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'synthetic fixture required');
    await page.evaluate(()=>stopEventStream());await page.locator('#tab-projects').click();
    const button=page.locator('#global-instructions'),dialog=page.locator('#detail-dialog');
    const layouts=[];
    for(const width of [1366,900,390,320]){
      await page.setViewportSize({width,height:900});await button.scrollIntoViewIfNeeded();
      const layout=await page.evaluate(()=>{const button=document.getElementById('global-instructions').getBoundingClientRect(),next=document.getElementById('project-cards').nextElementSibling.getBoundingClientRect();return {gap:next.top-button.bottom,overflow:document.documentElement.scrollWidth>innerWidth};});
      check(layout.gap>=12&&!layout.overflow,'instructions action spacing '+width+' '+JSON.stringify(layout));layouts.push({width,...layout});
    }
    await button.click();await page.waitForFunction(()=>detail?.instructionsLoaded);
    check(await dialog.locator(':scope>.modal-tabs').count()===0,'instructions open directly without summary tab');
    check(await dialog.locator('#detail-content>h4').count()===2,'document paths shown once per document');
    check(await dialog.innerText().then(text=>text.includes('Demo instructions')&&text.includes('最後更新時間')&&text.includes('讀取大小 (bytes)')&&!text.includes('檢視目前來源的指示文件')),'content and metadata merged');
    check(await dialog.locator('#detail-content .metadata-grid').first().innerText().then(text=>text.includes('0')),'known zero bytes retained');
    await dialog.getByRole('button',{name:'原文',exact:true}).first().click();check(await dialog.innerText().then(text=>text.includes('# Demo instructions')),'raw mode available');
    await dialog.getByRole('button',{name:'預覽',exact:true}).first().click();check(await dialog.getByRole('heading',{name:'Demo instructions',exact:true}).isVisible(),'preview mode available');
    await page.locator('#detail-close').click();mode='empty';await button.click();await page.waitForFunction(()=>detail?.instructionsLoaded);
    check(await dialog.innerText().then(text=>text.includes('目前來源沒有可讀取的 AGENTS.md')),'empty document state');
    await page.locator('#detail-close').click();mode='error';await button.click();await page.waitForFunction(()=>detail?.instructionsLoaded);
    check(await dialog.innerText().then(text=>text.includes('指示文件讀取失敗')),'failed document state');
    await page.locator('#detail-close').click();mode='documents';await button.click();await page.waitForFunction(()=>detail?.instructionsLoaded);
    check(await dialog.getByRole('heading',{name:'Demo instructions',exact:true}).isVisible(),'next open recovers');
    await page.locator('#detail-close').click();await page.waitForFunction(()=>!detail&&[...payloadViews.keys(),...payloadObservers.keys()].every(el=>el.isConnected&&!el.closest('#detail-content')));check(await page.locator('#detail-content').evaluate(el=>!el.childNodes.length&&el.scrollTop===0),'document controllers released');
    const openProject=async()=>page.evaluate(()=>{const project=projectRows()[0];openDetail({kind:'project',project,projectLoaded:true,projectContent:{folders:['synthetic/demo']}});return project.id;});
    const projectId=await openProject(),beforeInline=requests,fold=dialog.locator('.project-instructions'),summary=fold.locator(':scope>summary');
    check(requests===beforeInline&&!await fold.evaluate(el=>el.open),'project instructions defer reading');
    await summary.focus();await page.keyboard.press('Enter');await page.waitForFunction(()=>detail?.instructionsLoaded);
    check(await page.evaluate(id=>detail.kind==='project'&&detail.project.id===id,projectId),'instructions stay in project detail');
    check(await dialog.locator(':scope>.modal-tabs button').count()===2&&await fold.getByRole('heading',{name:'Demo instructions',exact:true}).isVisible(),'project tabs retained with inline document');
    await fold.getByRole('button',{name:'原文',exact:true}).first().click();
    const original=await fold.locator('.document-preview').first().elementHandle();
    await summary.click();await summary.click();
    check(requests===beforeInline+1&&await original.evaluate(el=>el.isConnected)&&await fold.locator('button[aria-pressed="true"]').first().innerText().then(text=>text==='原文'),'reopening retains document mode and avoids extra request');
    await page.locator('#detail-close').click();mode='empty';await openProject();await summary.click();await page.waitForFunction(()=>detail?.instructionsLoaded);check(await fold.innerText().then(text=>text.includes('目前來源沒有可讀取的 AGENTS.md')),'inline empty state');
    await page.locator('#detail-close').click();mode='error';await openProject();await summary.click();await page.waitForFunction(()=>detail?.instructionsLoaded);check(await fold.innerText().then(text=>text.includes('指示文件讀取失敗')),'inline error state');
    mode='documents';await fold.getByRole('button',{name:'重新讀取',exact:true}).click();await page.waitForFunction(()=>detail?.instructionsLoaded&&!detail.instructionsError);check(await fold.getByRole('heading',{name:'Demo instructions',exact:true}).isVisible(),'inline error recovery');
    for(const width of [1366,900,390,320]){await page.setViewportSize({width,height:900});check(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1&&$('detail-dialog').scrollWidth<=$('detail-dialog').clientWidth+1),'inline instructions width '+width);}
    await dialog.screenshot({path:'.local/project-instructions-inline.png',animations:'disabled'});await page.locator('#detail-close').click();await page.waitForFunction(()=>!detail&&[...payloadViews.keys(),...payloadObservers.keys()].every(el=>el.isConnected&&!el.closest('#detail-content')));check(await page.locator('#detail-content').evaluate(el=>!el.childNodes.length&&el.scrollTop===0),'inline controllers released');
    check(errors.length===0,'browser errors '+errors.join('|'));return {layouts,requests,merged:true,rawPreview:true,empty:true,errorRecovery:true,inlineProject:true};
  }finally{await page.unroute('**/api/codex/instructions?*',route);page.off('pageerror',onError);}
}
