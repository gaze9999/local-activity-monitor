async page=>{
  const origin=new URL(page.url()).origin,errors=[],onError=error=>errors.push(error.message);page.on('pageerror',onError);let requests=0,phase="preload";
  const routeHandler=async route=>{const response=await route.fetch(),value=await response.json();if(!value.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');requests++;if(phase==="preload"){value.updated_at=null;value.initial_sections=['catalog','runtime'];value.codex.threads=value.codex.threads.map(thread=>({...thread,metadata_only:true,tokens:{},tool_calls:null,task_duration_ms:null,tools:{},tool_events:[]}));value.codex.tools={};value.codex.observed_tool_calls=null;value.codex.activity_series=[];value.codex.tool_series=[];}else await new Promise(resolve=>setTimeout(resolve,1500));await route.fulfill({response,json:value});};
  await page.route(origin+'/api/snapshot?*',routeHandler);
  try{
    await page.evaluate(()=>{const value=JSON.parse(localStorage.getItem(preferenceKey)||'{}');value.tab='codex';value.conversationSource='conversations';localStorage.setItem(preferenceKey,JSON.stringify(value));});await page.goto(origin);
    await page.waitForFunction(()=>data?.initial_sections);
    const initial=await page.evaluate(()=>{const body=document.getElementById('codex-rows'),panel=body.closest('.panel'),host=panelDataStates.get(panel);return {rows:body.children.length,visible:body.getBoundingClientRect().height>0&&getComputedStyle(host.boundary).visibility!=='hidden',fullUpdate:data.updated_at,live:document.getElementById('live').textContent,unknownRate:[...body.querySelectorAll('td[data-column="平均 Token / 秒"]')].every(cell=>cell.textContent==='--'),state:host.view.element.dataset.state};});
    if(!initial.rows||!initial.visible||initial.fullUpdate||initial.state!=='ready')throw Error('Preloaded catalog must be readable before session refresh '+JSON.stringify(initial));
    phase='ready';await page.evaluate(()=>refresh());await page.waitForFunction(()=>data?.updated_at&&!data.initial_sections);if(errors.length)throw Error('Startup faults '+JSON.stringify(errors));return {passed:true,initial,requests,fullUpdateAfterPreload:true,errors};
  }finally{await page.unroute(origin+'/api/snapshot?*',routeHandler);page.off('pageerror',onError);}
}
