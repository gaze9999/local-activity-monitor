async page => {
  const check=(value,message)=>{if(!value)throw Error(message);},errors=[],failures=[];
  const onError=error=>errors.push(error.message),onResponse=response=>{if(response.url().startsWith(new URL(page.url()).origin)&&response.status()>=400)failures.push(response.status());};
  page.on('pageerror',onError);page.on('response',onResponse);
  try {
    await page.evaluate(()=>localStorage.removeItem(preferenceKey));await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'synthetic fixture required');
    await page.getByRole('tab',{name:'錯誤與 Log',exact:true}).click();await page.getByRole('tab',{name:'模型 API',exact:true}).click();
    const summary=await page.evaluate(()=>({requests:data.model_api.requests,connections:data.model_api.connections,retries:data.model_api.retry_events,failed:data.model_api.failed_requests}));
    check(JSON.stringify(summary)===JSON.stringify({requests:3,connections:1,retries:1,failed:1}),'separate API phase counts '+JSON.stringify(summary));
    const view=page.locator('#view-model-api');check(await page.locator('#model-api-cards').isHidden(),'child summary hidden by default');
    check(await page.locator('#model-api-rows tr').count()===5,'five observed records');
    check(await page.evaluate(()=>[...document.querySelectorAll('#model-api-rows tr')].some(row=>{const cells=[...row.cells].map(cell=>cell.textContent);return cells.includes('req_demo')&&cells[7]==='0'&&cells[8]==='0';})),'confirmed zero duration and input tokens');
    check(await page.evaluate(()=>[...document.querySelectorAll('#model-api-rows tr')].some(row=>{const cells=[...row.cells].map(cell=>cell.textContent);return cells.includes('連線事件')&&cells[7]==='--'&&cells[8]==='--';})),'connection has no invented duration or tokens');
    await page.locator('#model-api-rows tr').filter({hasText:'req_demo_failed'}).click();await page.locator('#detail-dialog').waitFor({state:'visible'});
    const detail=await page.locator('#detail-content').innerText();check(detail.includes('429')&&detail.includes('req_demo_failed')&&!detail.includes('PRIVATE'),'metadata detail without raw error body');
    await page.keyboard.press('Escape');
    await page.getByRole('button',{name:'模型 API Tab 設定',exact:true}).click();
    const dialog=page.locator('#tab-dialog');await dialog.getByRole('tab',{name:'監測項目',exact:true}).click();
    const toggle=dialog.getByRole('switch',{name:'模型 API 呼叫監測',exact:true});await toggle.uncheck();
    await page.waitForFunction(()=>data.model_api.enabled===false);check(await page.locator('#model-api-rows tr').count()===0,'disabled records hidden');
    await toggle.check();await page.waitForFunction(()=>data.model_api.enabled&&data.model_api.requests===3);await dialog.getByRole('tab',{name:'摘要卡',exact:true}).click();await dialog.getByRole('combobox',{name:'摘要卡數量',exact:true}).selectOption('4');await page.locator('#tab-settings-close').click();await dialog.waitFor({state:'hidden'});check(await page.locator('#model-api-cards').isVisible(),'child summary can be enabled');
    const layouts=[];
    for(const width of [1600,820,390,320]){
      await page.setViewportSize({width,height:1000});await page.waitForTimeout(100);
      layouts.push(await page.evaluate(()=>({width:innerWidth,overflow:document.documentElement.scrollWidth>innerWidth,tableWidth:document.querySelector('#model-api-rows').closest('.table-wrap').getBoundingClientRect().width})));
    }
    check(layouts.every(item=>!item.overflow&&item.tableWidth>0),'responsive API table '+JSON.stringify(layouts));
    await page.screenshot({path:'.local/model-api-mobile.png',fullPage:true});
    await page.setViewportSize({width:1600,height:1000});await page.screenshot({path:'.local/model-api-desktop.png',fullPage:true});
    for(const [language,label]of [['en','Model API records'],['ja','モデル API 記録']]){
      await page.getByRole('button',{name:language==='en'?'設定':'Settings',exact:true}).click();
      const settings=page.locator('#settings-dialog');await settings.getByRole('tab',{name:language==='en'?'外觀':'Appearance',exact:true}).click();
      await page.locator('#language-select').selectOption(language);await page.locator('#settings-close').click();await settings.waitFor({state:'hidden'});
      check((await view.innerText()).includes(label),'localized API view '+language);
    }
    await page.getByRole('button',{name:'設定',exact:true}).click();await page.locator('#settings-dialog').getByRole('tab',{name:'外観',exact:true}).click();await page.locator('#language-select').selectOption('zh-TW');await page.locator('#settings-close').click();
    check(!errors.length,'page errors '+errors.join(';'));check(!failures.length,'failed requests '+failures.join(','));
    return {summary,layouts,pageErrors:errors,failedRequests:failures};
  }finally{page.off('pageerror',onError);page.off('response',onResponse);}
}
