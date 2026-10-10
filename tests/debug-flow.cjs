async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&eventStreamReady&&!busy&&!queuedSnapshot);
  if(!await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  const check=(value,message)=>{if(!value)throw Error(message);};
  await page.evaluate(()=>{locale='zh-TW';applyLanguage();switchTab('monitor');openSettings();});
  await page.locator('#settings-dialog').getByRole('tab',{name:'更新與追蹤',exact:true}).click();
  const control=page.getByRole('switch',{name:'Debug 模式',exact:true});
  check(!await control.isChecked(),'Debug defaults off');
  try{
    await control.check();await page.waitForFunction(()=>!settingsBusy);await page.locator('#settings-close').click();await page.mouse.move(0,0);await page.locator('[data-tab="monitor"]').focus();
    await page.waitForFunction(()=>data.settings.debug_mode&&!settingsBusy&&data.monitor.debug.records.some(record=>record.kind==='collection'));
    const recorded=await page.evaluate(()=>({enabled:data.monitor.debug.enabled,bytes:data.monitor.debug.bytes,limit:data.monitor.debug.byte_limit,count:data.monitor.debug.records.length,kinds:[...new Set(data.monitor.debug.records.map(record=>record.kind))],rss:data.monitor.debug.records.find(record=>record.rss_bytes)?.rss_bytes,exported:configuration().preferences.settings.debug_mode}));
    check(recorded.enabled&&recorded.bytes>0&&recorded.limit===32*1024*1024&&recorded.rss>0&&recorded.exported,'Debug records visible and exportable '+JSON.stringify(recorded));
    check(await page.evaluate(()=>data.monitor.debug.path?.endsWith('performance-debug.jsonl')&&data.monitor.log_path?.endsWith('monitor.jsonl')&&!$('monitor-debug-panel').hidden),'owned diagnostic file locations visible');
    await page.waitForFunction(()=>data.monitor.debug.records.some(record=>record.kind==='stream_frame'));
    const collections=await page.evaluate(()=>data.monitor.debug.records.filter(record=>record.kind==='collection').length);
    await page.waitForFunction(()=>data.monitor.debug.records.some(record=>record.kind==='heartbeat'),null,{timeout:20000});
    check(await page.evaluate(()=>data.monitor.debug.records.filter(record=>record.kind==='collection').length)===collections,'Heartbeat samples without source collection');
    await page.waitForFunction(()=>targetTableRows(document.getElementById('debug-rows')).length>0);
    await page.evaluate(()=>{const row=targetTableRows(document.getElementById('debug-rows')).find(row=>row.textContent.includes('資料整理'));if(!row)throw Error('Collection record missing');row.dataset.fixtureDebug='true';});
    await page.locator('#debug-rows tr[data-fixture-debug="true"]').click();
    await page.waitForFunction(()=>detail?.kind==='performance'&&document.getElementById('detail-dialog').open);
    check(await page.locator('#detail-content').textContent().then(text=>text.includes('程序累計 CPU')&&text.includes('緩衝區')),'Debug detail metrics');
    await page.evaluate(()=>document.getElementById('detail-dialog').close());
    const layouts=[];
    for(const language of ['zh-TW','en','ja'])for(const width of [1366,390,320]){
      await page.setViewportSize({width,height:900});
      const sample=await page.evaluate(language=>{locale=language;applyLanguage();renderMonitor();return {language,width:innerWidth,overflow:document.documentElement.scrollWidth-innerWidth,visible:!document.getElementById('monitor-debug-panel').hidden,rows:targetTableRows(document.getElementById('debug-rows')).length,records:data.monitor.debug.records.length,viewRows:tableViews.get('debug-rows').rows.length,state:tableStates['debug-rows']};},language);
      check(sample.overflow<=1&&sample.visible&&sample.rows>0,'Debug layout '+JSON.stringify(sample));layouts.push(sample);
    }
    await page.setViewportSize({width:1366,height:900});await page.evaluate(()=>{locale='zh-TW';applyLanguage();renderMonitor();});
    await page.waitForFunction(()=>targetTableRows(document.getElementById('debug-rows')).every(row=>!row.inert&&row.isConnected&&!row.getAnimations({subtree:true}).some(animation=>animation.playState==='running')));
    check(await page.evaluate(()=>targetTableRows(document.getElementById('debug-rows')).some(row=>row.textContent.includes('資料整理'))),'Current debug row language');
    await page.locator('#monitor-debug-panel').screenshot({path:'.local/source-sse-debug.png'});
    await page.evaluate(()=>openSettings());await page.locator('#settings-dialog').getByRole('tab',{name:'更新與追蹤',exact:true}).click();await control.uncheck();await page.waitForFunction(()=>!settingsBusy);await page.locator('#settings-close').click();await page.mouse.move(0,0);await page.locator('[data-tab="monitor"]').focus();
    await page.waitForFunction(()=>!data.settings.debug_mode&&!settingsBusy&&!data.monitor.debug.enabled&&data.monitor.debug.records.some(record=>record.kind==='disabled'));
    const stopped=await page.evaluate(()=>data.monitor.debug.records.length);
    const previous=await page.evaluate(()=>eventVersion);
    await page.evaluate(()=>post('/api/refresh',{}));await page.waitForFunction(()=>!busy&&!queuedSnapshot);
    await page.waitForFunction(previous=>eventVersion>previous&&!busy&&!queuedSnapshot,previous);
    check(await page.evaluate(()=>data.monitor.debug.records.length)===stopped,'Debug off stops recording');
    check(await page.evaluate(()=>$('monitor-debug-panel').hidden),'Debug off hides diagnostics');
    return {recorded,detail:true,layouts,heartbeatWithoutCollection:true,stoppedRecording:true};
  }finally{
    await page.evaluate(async()=>{if(data?.settings?.debug_mode)await changeSettings({debug_mode:false});});
  }
}
