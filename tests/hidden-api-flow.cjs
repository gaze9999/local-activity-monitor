async page=>{
  const origin=new URL(page.url()).origin,requests=[],onRequest=request=>{const path=new URL(request.url()).pathname;if(/^\/api\/(?:codex\/(?:account|cloud)|public-query|openai-platform)/.test(path))requests.push(path);};
  page.on('request',onRequest);
  try{
    await page.evaluate(()=>{stopEventStream();localStorage.setItem(preferenceKey,JSON.stringify({tab:'codex',conversationSource:'cloud'}));});
    await page.goto(origin);await page.waitForFunction(()=>data?.updated_at&&!busy);
    const restored=await page.evaluate(()=>({source:conversationSource,cloudTab:!!$('codex-cloud'),observations:JSON.stringify(data.settings.observations),intervals:JSON.stringify(data.settings.observation_intervals)}));
    if(restored.source!=='conversations'||restored.cloudTab)throw Error('Hidden cloud preference must restore to conversations');
    const cases=[];
    for(const language of ['zh-TW','en','ja']){
      await page.evaluate(language=>{locale=language;applyLanguage();stopEventStream();},language);
      await page.locator('#observation-settings').click();
      const result=await page.evaluate(()=>({language:locale,apiSettings:!!document.querySelector('[data-modal-key="API 查詢"]'),content:!!$('observation-api')?.children.length}));
      if(result.apiSettings||result.content)throw Error('API settings must be absent');cases.push(result);
      await page.locator('#settings-close').click();
      const visible=await page.evaluate(()=>Object.keys(apiCatalog).some(key=>document.querySelector('main .api-query-actions[data-api-key="'+key+'"]')));
      if(visible)throw Error('Hidden API source is still rendered');
    }
    const preserved=await page.evaluate(before=>JSON.stringify(data.settings.observations)===before.observations&&JSON.stringify(data.settings.observation_intervals)===before.intervals,restored);
    if(!preserved||requests.length)throw Error('Settings changed or API queried');return {restoredSource:restored.source,cases,preserved,requests};
  }finally{page.off('request',onRequest);await page.evaluate(()=>stopEventStream());}
}
