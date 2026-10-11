async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);},requests=[];let fail=false;
  await page.route('**/api/codex/cloud*',async route=>{const cursor=new URL(route.request().url()).searchParams.get('cursor');requests.push(cursor);await route.fulfill({status:200,json:fail?{health:'error'}:{health:'ok',checked_at:new Date().toISOString(),items:[{id:cursor?'task_older':'task_recent',title:cursor?'Older cloud chat':'Recent cloud chat',status:'completed',updated_at:new Date().toISOString(),environment_label:'Demo environment'}],cursor:cursor?null:'page_2'}});});
  await page.evaluate(async()=>{await changeSettings({observations:{codex_cloud:false}});stopEventStream();switchTab('codex');selectConversationSource('cloud');});
  const root=page.locator('#cloud-content');check(await root.getByRole('button',{name:'查詢',exact:true}).isVisible(),'manual query available while automatic is off');
  await root.getByRole('button',{name:'查詢',exact:true}).click();await page.waitForFunction(()=>$('cloud-task-rows').textContent.includes('Recent cloud chat'));
  check(!await page.locator('#detail-dialog').evaluate(el=>el.open),'cloud records shown directly');
  await root.getByRole('button',{name:'較早紀錄',exact:true}).click();await page.waitForFunction(()=>cloudView.cloudPage===1&&!cloudView.toolLoading);
  await page.locator('#cloud-task-rows').getByText('Older cloud chat',{exact:true}).waitFor({state:'visible'});
  check(await page.locator('#cloud-task-rows').getByText('Older cloud chat',{exact:true}).count()===1,'older cursor metadata');check(await root.getByRole('button',{name:'較早紀錄',exact:true}).isDisabled(),'cursor end');
  fail=true;await root.getByRole('button',{name:'較新紀錄',exact:true}).click();await page.waitForFunction(()=>cloudView.cloudError&&!cloudView.toolLoading);check(await page.locator('#cloud-task-rows').getByText('Older cloud chat',{exact:true}).count()===1,'failure preserves current page');
  fail=false;await root.getByRole('button',{name:'較新紀錄',exact:true}).click();await page.waitForFunction(()=>cloudView.cloudPage===0&&!cloudView.toolLoading);
  const layouts=[];for(const lang of ['zh-TW','en','ja'])for(const width of [1366,390,320]){await page.setViewportSize({width,height:900});await page.evaluate(lang=>{locale=lang;applyLanguage();appearance.font=24;applyAppearance();},lang);check(!await root.evaluate(el=>el.scrollWidth>el.clientWidth+1),'cloud layout '+lang+' '+width);layouts.push({lang,width});}
  check(requests.length===4,'no browser polling');await page.evaluate(()=>{locale='zh-TW';applyLanguage();appearance.font=15;applyAppearance();});
  const cloud=await page.evaluate(()=>{const identity=data.codex.threads[0].thread_id;data.codex.dots={events:[],computers:[],activity:[{thread_id:identity,environment:'cloud',activity_type:'dot',timestamp:new Date().toISOString()}]};renderDots();data.codex.schedules={health:'ok',items:[],remote_activities:[{thread_id:identity,environment:'cloud',kind:'schedule',last_run_at:new Date().toISOString(),status:'completed'}]};renderSchedules();return {dots:$('dot-activity-rows').textContent,schedules:$('remote-schedule-rows').textContent,heading:$('remote-schedule-rows').closest('.panel').querySelector('h3').textContent};});check(cloud.dots.includes('雲端')&&cloud.schedules.includes('雲端')&&cloud.heading==='雲端與遠端排程','cloud dot/schedule source');
  return {requests,layouts,cloud};
}
