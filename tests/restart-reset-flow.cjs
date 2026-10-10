async page=>{
  await page.setViewportSize({width:1366,height:900});
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  await page.evaluate(()=>{locale='zh-TW';applyLanguage();});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const initial=await page.evaluate(()=>{if(!data.codex.threads.every(t=>t.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');window.fixtureDocument=document.documentElement;preferences.settings={...data.settings,max_files:17,interval:17};document.getElementById('thread-search').value='preserved filter';const settings=data.settings;window.fixtureNativeEventSource=EventSource;window.EventSource=class extends fixtureNativeEventSource{addEventListener(type,listener,options){super.addEventListener(type,type==='snapshot'?event=>{const value=JSON.parse(event.data);value.snapshot.backend_revision='synthetic-restarted';value.snapshot.revision='synthetic-restarted-'+value.snapshot.frontend_revision;listener(new MessageEvent(type,{data:JSON.stringify(value)}));}:listener,options);}};return {settings,revision:data.revision};});
  try{
  await page.evaluate(()=>refresh());await page.evaluate(()=>refresh());
  const restarted=await page.evaluate(()=>({sameDocument:window.fixtureDocument===document.documentElement,filter:document.getElementById('thread-search').value,maxFiles:data.settings.max_files,legacyInterval:data.settings.interval,pending:pendingRevision}));
  if(!restarted.sameDocument||restarted.filter!=='preserved filter'||restarted.maxFiles!==17||restarted.legacyInterval!==undefined||restarted.pending)throw Error(JSON.stringify(restarted));
  await page.evaluate(()=>{loadedBackendRevision=null;preferences.copy={'工具':'CUSTOM_COPY'};appearance.font=18;display.table=20;tableStates['sqlite-rows']={size:20,page:2};chartViews.get('overview-tools-chart').settings.top=99;overviewOrder.reverse();applyOverview();saveView();});
  await page.getByRole('button',{name:'設定',exact:true}).click();
  await page.locator('#settings-dialog').getByRole('tab',{name:'設定檔',exact:true}).click();
  await page.locator('#reset-all-configuration').click();await page.locator('#confirm-accept').click();
  await page.waitForFunction(()=>!document.getElementById('reset-all-configuration').disabled&&!busy&&appearance.font===14&&display.table===10);
  const reset=await page.evaluate(()=>({sameDocument:window.fixtureDocument===document.documentElement,appearance:appearance.font,table:display.table,copies:Object.keys(preferences.copy).length,tableSize:tableStates['sqlite-rows']?.size??display.table,tablePage:tableStates['sqlite-rows']?.page??1,chartTop:chartViews.get('overview-tools-chart').settings.top,defaultTop:chartViews.get('overview-tools-chart').defaults.top,scope:sourceWindows.global,settingsOpen:document.getElementById('settings-dialog').open}));
  if(!reset.sameDocument||reset.copies||reset.tableSize!==10||reset.tablePage!==1||reset.chartTop!==reset.defaultTop||reset.scope!=='24h'||!reset.settingsOpen||errors.length)throw Error(JSON.stringify({reset,errors}));
  await page.locator('#settings-dialog').evaluate(el=>el.close());await page.evaluate(settings=>post('/api/settings',settings),initial.settings);await page.evaluate(()=>refresh());return {restarted,reset,errors};
  }finally{await page.evaluate(()=>{EventSource=fixtureNativeEventSource;delete window.fixtureNativeEventSource;});}
}
