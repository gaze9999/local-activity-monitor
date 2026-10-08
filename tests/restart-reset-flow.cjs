async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const initial=await page.evaluate(()=>{if(!data.codex.threads.every(t=>t.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');window.fixtureDocument=document.documentElement;preferences.settings={...data.settings,interval:17};document.getElementById('thread-search').value='preserved filter';return {settings:data.settings,revision:data.revision};});
  let requests=0;
  await page.route('**/api/snapshot?**',async route=>{const response=await route.fetch(),value=await response.json();value.backend_revision='synthetic-restarted';value.frontend_revision=initial.revision.split('-').at(-1);value.revision='synthetic-restarted-'+value.frontend_revision;if(requests++===0)value.settings.interval=10;await route.fulfill({response,json:value});});
  await page.evaluate(()=>refresh());await page.evaluate(()=>refresh());
  const restarted=await page.evaluate(()=>({sameDocument:window.fixtureDocument===document.documentElement,filter:document.getElementById('thread-search').value,interval:data.settings.interval,pending:pendingRevision}));
  if(!restarted.sameDocument||restarted.filter!=='preserved filter'||restarted.interval!==17||restarted.pending)throw Error(JSON.stringify(restarted));
  await page.unroute('**/api/snapshot?**');
  await page.evaluate(()=>{loadedBackendRevision=null;preferences.copy={'工具':'CUSTOM_COPY'};appearance.font=18;display.table=20;tableStates['sqlite-rows']={size:20,page:2};chartViews.get('overview-tools-chart').settings.top=99;overviewOrder.reverse();applyOverview();saveView();});
  await page.getByRole('button',{name:'設定',exact:true}).click();
  await page.locator('#settings-dialog').getByRole('tab',{name:'設定檔',exact:true}).click();
  await page.locator('#reset-all-configuration').click();await page.locator('#confirm-accept').click();
  await page.waitForFunction(()=>!document.getElementById('reset-all-configuration').disabled&&!busy&&appearance.font===14&&display.table===10);
  const reset=await page.evaluate(()=>({sameDocument:window.fixtureDocument===document.documentElement,appearance:appearance.font,table:display.table,copies:Object.keys(preferences.copy).length,tableSize:tableStates['sqlite-rows']?.size??display.table,tablePage:tableStates['sqlite-rows']?.page??1,chartTop:chartViews.get('overview-tools-chart').settings.top,defaultTop:chartViews.get('overview-tools-chart').defaults.top,scope:sourceWindows.global,settingsOpen:document.getElementById('settings-dialog').open}));
  if(!reset.sameDocument||reset.copies||reset.tableSize!==10||reset.tablePage!==1||reset.chartTop!==reset.defaultTop||reset.scope!=='24h'||!reset.settingsOpen||errors.length)throw Error(JSON.stringify({reset,errors}));
  await page.locator('#settings-dialog').evaluate(el=>el.close());await page.evaluate(settings=>post('/api/settings',settings),initial.settings);await page.evaluate(()=>refresh());return {restarted,reset,errors};
}
