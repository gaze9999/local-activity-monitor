async page=>{
  const check=(ok,message)=>{if(!ok)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic fixture required');
  await page.evaluate(()=>{stopEventStream();switchTab('monitor');data.monitor.history=[{time:new Date(Date.now()-1000).toISOString(),cpu_ms:0,refresh_ms:1}];renderCharts();});
  await page.locator('#monitor-cpu-chart').scrollIntoViewIfNeeded();await page.waitForFunction(()=>$('monitor-cpu-chart').children.length&&!$('monitor-cpu-chart').classList.contains('chart-is-empty'));
  check(await page.locator('#monitor-cpu-note').textContent().then(text=>text.includes('0 ms')),'confirmed CPU zero renders as data');
  await page.evaluate(()=>{data.monitor.history=[{time:new Date(Date.now()-1000).toISOString(),cpu_ms:null}];renderCharts();});await page.waitForFunction(()=>$('monitor-cpu-chart').classList.contains('chart-is-empty'));
  check(await page.locator('#monitor-cpu-note').textContent().then(text=>text.includes('此範圍尚無活動紀錄')),'unknown CPU is not converted to zero');
  const colors=await page.evaluate(()=>{appearance.theme='sand';appearance.mode='dark';applyAppearance();const style=getComputedStyle(document.documentElement);return {theme:appearance.theme,surface:style.getPropertyValue('--surface').trim(),shared:style.getPropertyValue('--wb-surface').trim()};});check(colors.theme==='sand'&&colors.surface===colors.shared&&colors.surface==='#302a23','shared brown tokens');
  await page.evaluate(()=>openTabSettings('monitor'));await page.locator('#tab-dialog').getByRole('tab',{name:'卡片庫',exact:true}).click();await page.locator('#tab-dialog').screenshot({path:'.local/settings-cards-dark.png'});
  await page.setViewportSize({width:900,height:1600});await page.locator('#tab-dialog').getByRole('tab',{name:'摘要卡',exact:true}).click();await page.locator('#tab-dialog').screenshot({path:'.local/settings-cards-portrait.png'});
  return {zero:true,unknown:true,colors,portrait:true};
}
