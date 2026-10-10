async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic source required');
  await page.evaluate(()=>{locale='zh-TW';applyLanguage();switchTab('mcp');selectMcpSource('all');const snapshot=structuredClone(data);snapshot.mcp.telemetry.fixture_telemetry={health:'ok',enabled_at:snapshot.updated_at,updated_at:snapshot.updated_at,summary:{calls:2,tokens:0}};renderSnapshot(snapshot);arrangeContentPanels(true);});
  await page.mouse.move(1,1);
  const initial=await page.evaluate(()=>{
    const card=document.querySelector('#mcp-observations .source-observation'),list=card.querySelector('.metadata-grid');window.fixtureMcpCard=card;window.fixtureMcpList=list;window.fixtureMcpTitle=card.querySelector('h3 button');
    const source=data.mcp.servers.find(source=>'mcp:'+source.server===card.dataset.cardKey);return {key:card.dataset.cardKey,server:source.server,recognized:source.recognized,height:card.getBoundingClientRect().height,rows:[...list.querySelectorAll(':scope>dt,:scope>dd')].map(row=>row.textContent)};
  });
  await page.evaluate(()=>setSnapshotState('refreshing'));
  const refreshing=await page.evaluate(()=>({retained:fixtureMcpList.dataset.wbRefreshing,pending:fixtureMcpList.classList.contains('wb-data-pending'),visible:getComputedStyle(fixtureMcpList).visibility,light:!!fixtureMcpList.querySelector('.wb-refresh-text'),height:fixtureMcpCard.getBoundingClientRect().height,rows:[...fixtureMcpList.querySelectorAll(':scope>dt,:scope>dd')].map(row=>row.textContent)}));
  check(refreshing.retained==='true'&&!refreshing.pending&&refreshing.visible==='visible'&&refreshing.light&&Math.abs(initial.height-refreshing.height)<1,'retained MCP refresh effect: '+JSON.stringify(refreshing));
  check(JSON.stringify(refreshing.rows)===JSON.stringify(initial.rows),'refresh does not erase displayed values');
  const update=async increment=>{
    await page.evaluate(({server,increment})=>{const snapshot=structuredClone(data);snapshot.mcp.servers.find(source=>source.server===server).recognized+=increment;eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:(eventVersion??0)+1,window:activeSourceWindow(),snapshot,logs:logData})}));},{server:initial.server,increment});
    await page.waitForFunction(()=>!busy&&!queuedSnapshot&&!snapshotFrame);
  };
  await update(1);
  const retained=await page.evaluate(key=>{const card=[...document.querySelector('#mcp-observations').children].find(card=>card.dataset.cardKey===key);return {card:card===fixtureMcpCard,list:card.querySelector('.metadata-grid')===fixtureMcpList,title:card.querySelector('h3 button')===fixtureMcpTitle,recognized:card.querySelectorAll('.metadata-grid dd')[1].textContent,actions:[...card.querySelectorAll('button')].filter(button=>button.checkVisibility()).length};},initial.key);
  check(retained.card&&retained.list&&retained.title&&retained.recognized===String(initial.recognized+1)&&retained.actions===1,'MCP cards retain nodes and one detail entry: '+JSON.stringify(retained));
  const frames=await page.evaluate(async server=>{
    const samples=[];for(let index=0;index<12;index++){const snapshot=structuredClone(data);snapshot.mcp.servers.find(source=>source.server===server).recognized++;eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:(eventVersion??0)+1,window:activeSourceWindow(),snapshot,logs:logData})}));await new Promise(requestAnimationFrame);samples.push({same:fixtureMcpCard.isConnected,visible:getComputedStyle(fixtureMcpList).visibility,pending:fixtureMcpList.classList.contains('wb-data-pending'),height:fixtureMcpCard.getBoundingClientRect().height});}return samples;
  },initial.server);
  check(frames.every(frame=>frame.same&&frame.visible==='visible'&&!frame.pending&&Math.abs(frame.height-initial.height)<1),'MCP frame stability: '+JSON.stringify(frames));
  await page.waitForFunction(()=>!busy&&!queuedSnapshot&&!snapshotFrame);
  const layouts=[];
  for(const width of [1436,900,390])for(const language of ['zh-TW','en','ja']){
    await page.setViewportSize({width,height:900});await page.evaluate(language=>{locale=language;applyLanguage();arrangeContentPanels(true);},language);
    layouts.push(await page.evaluate(()=>{
      const cards=[...document.querySelector('#mcp-observations').children],rects=cards.map(card=>card.getBoundingClientRect());let overlap=false;
      for(let i=0;i<rects.length;i++)for(let j=i+1;j<rects.length;j++)if(rects[i].left<rects[j].right-.5&&rects[j].left<rects[i].right-.5&&rects[i].top<rects[j].bottom-.5&&rects[j].top<rects[i].bottom-.5)overlap=true;
      return {width:innerWidth,language:locale,cards:cards.length,overlap,overflow:document.documentElement.scrollWidth>innerWidth,aligned:cards.every(card=>[...card.querySelectorAll('.metadata-grid dd')].every(entry=>getComputedStyle(entry).textAlign==='right'))};
    }));
  }
  check(layouts.every(layout=>layout.cards>=3&&!layout.overlap&&!layout.overflow&&layout.aligned),'MCP layouts: '+JSON.stringify(layouts));
  await page.setViewportSize({width:1366,height:900});await page.evaluate(()=>{locale='zh-TW';applyLanguage();});
  await page.locator('#mcp-observations .source-observation h3 button').first().click();
  check(await page.evaluate(()=>document.getElementById('detail-dialog').open&&detail.kind==='mcp-source'&&detail.server===data.mcp.servers.find(source=>'mcp:'+source.server===fixtureMcpCard.dataset.cardKey).server),'source detail opens from retained card');
  await page.evaluate(()=>document.getElementById('detail-dialog').close());
  await page.screenshot({path:'.local/mcp-refresh-cards.png'});
  return {retained,refreshing,frames:frames.length,layouts,detail:'source matched'};
}
