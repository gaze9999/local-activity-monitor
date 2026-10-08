async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  if(!await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  const errors=[],requests=[];page.on('pageerror',error=>errors.push(error.message));
  const original=await page.evaluate(()=>({locale,mask:contentMasked(),threadId:data.codex.threads[0].thread_id,messages:data.codex.threads[0].agent_messages}));
  await page.route('**/api/codex/agent-message?**',route=>{const query=new URL(route.request().url()).searchParams;requests.push({kind:'agent',mask:query.get('mask')});return route.fulfill({contentType:'application/json',body:JSON.stringify({request:{message:query.get('mask')==='0'?'ORIGINAL-MESSAGE':'MASKED-MESSAGE'},response:{status:'recorded'},text:{context_state:'not_recorded'},context_state:'not_recorded',masked:query.get('mask')!=='0'})});});
  await page.route('**/api/codex/context?**',route=>{const query=new URL(route.request().url()).searchParams;requests.push({kind:'context',mask:query.get('mask'),call:query.get('call_id')});return route.fulfill({contentType:'application/json',body:JSON.stringify({text:[{role:'assistant',text:'PUBLIC-CONTEXT-SUMMARY'}],scope:'recent_available_messages',context_state:'recorded',truncated:true,masked:query.get('mask')!=='0'})});});
  try{
    await page.evaluate(()=>{locale='zh-TW';applyLanguage();setContentMasking(true);const thread=data.codex.threads[0];thread.agent_messages=[{action:'send_message',tool:'collaboration.send_message',target:'/root/worker',sender:'/root',direction:'outgoing',nested:false,index:0,call_id:'call_fixture_communication',thread_id:thread.thread_id,timestamp:data.updated_at}];openDetail({kind:'thread',id:thread.thread_id,thread});});
    await page.locator('#detail-dialog').getByRole('tab',{name:'代理訊息',exact:true}).click();
    await page.locator('#detail-content tbody tr').filter({hasText:'傳送訊息'}).click();
    await page.waitForFunction(()=>detail?.agentLoaded&&detail.agentContent?.request?.message==='MASKED-MESSAGE');
    await page.evaluate(()=>openSettings());
    await page.locator('#settings-dialog').getByRole('tab',{name:'內容與隱私',exact:true}).click();
    await page.locator('#content-masking').uncheck();
    await page.locator('#settings-close').click();
    await page.waitForFunction(()=>detail?.agentLoaded&&detail.agentContent?.request?.message==='ORIGINAL-MESSAGE');
    const settings=await page.evaluate(()=>{const value=configuration(),legacy=JSON.parse(JSON.stringify(value));delete legacy.preferences.contentMasking;const imported=readConfiguration(legacy);let invalidRejected=false;try{readConfiguration({...value,preferences:{...value.preferences,contentMasking:'invalid'}});}catch{invalidRejected=true;}return {mask:value.preferences.contentMasking,legacyMask:imported.preferences.contentMasking,sqlMask:value.preferences.sqlMasking,invalidRejected};});
    if(settings.mask!==false||settings.legacyMask!==false||settings.sqlMask!==false||!settings.invalidRejected)throw Error(JSON.stringify(settings));
    await page.evaluate(()=>openDetail({kind:'context',thread:data.codex.threads[0],incoming:true}));
    await page.waitForFunction(()=>detail?.contextLoaded);
    const context=await page.evaluate(()=>({state:detail.contextContent.context_state,summary:JSON.stringify(detail.contextContent.text).includes('PUBLIC-CONTEXT-SUMMARY'),incomingNote:document.getElementById('detail-dialog').textContent.includes('沒有 Call ID'),hasHiddenReasoning:JSON.stringify(detail.contextContent).includes('HIDDEN-REASONING')}));
    if(!context.summary||!context.incomingNote||context.hasHiddenReasoning)throw Error(JSON.stringify(context));
    await page.evaluate(()=>{setContentMasking(true);});
    await page.waitForFunction(()=>detail?.contextLoaded&&detail.contextContent.masked===true);
    if(!requests.some(item=>item.kind==='agent'&&item.mask==='1')||!requests.some(item=>item.kind==='agent'&&item.mask==='0')||!requests.some(item=>item.kind==='context'&&item.mask==='0'&&item.call===null)||!requests.some(item=>item.kind==='context'&&item.mask==='1'))throw Error(JSON.stringify(requests));
    if(errors.length)throw Error(JSON.stringify(errors));
    return {settings,context,requests,errors};
  }finally{
    await page.evaluate(value=>{document.getElementById('settings-dialog').close();document.getElementById('detail-dialog').close();setContentMasking(value.mask);locale=value.locale;applyLanguage();saveView();const thread=data.codex.threads.find(item=>item.thread_id===value.threadId);if(thread){if(value.messages==null)delete thread.agent_messages;else thread.agent_messages=value.messages;}},original);
    await page.unroute('**/api/codex/agent-message?**');await page.unroute('**/api/codex/context?**');
  }
}
