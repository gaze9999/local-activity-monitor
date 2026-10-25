async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'Synthetic fixture required');
  const origin=new URL(page.url()).origin,failed=[];
  let release,started;const gate=new Promise(resolve=>release=resolve),ready=new Promise(resolve=>started=resolve);
  const onFailed=request=>{if(new URL(request.url()).pathname==='/api/codex/file')failed.push(request.failure()?.errorText);};
  const handler=async route=>{started();await gate;try{await route.fulfill({json:{health:'ok',bytes:0}});}catch{}};
  page.on('requestfailed',onFailed);await page.route(origin+'/api/codex/file?*',handler);
  try{
    await page.evaluate(()=>openDetail({kind:'file',event:{thread_id:data.codex.threads[0].thread_id,call_id:'synthetic-owned-http',path:'synthetic.txt',operation:'read',tool:'exec_command',timestamp:data.updated_at}}));
    await ready;await page.locator('#detail-close').click();await page.waitForFunction(()=>detail===null&&!document.getElementById('detail-dialog').open);
    check(failed.includes('net::ERR_ABORTED'),'Closing detail aborts the actual HTTP request');
  }finally{release();await page.unroute(origin+'/api/codex/file?*',handler);page.off('requestfailed',onFailed);}
  const result=await page.evaluate(async()=>{
    stopEventStream();const original=request,pending=[],results=[];
    const tick=()=>new Promise(resolve=>setTimeout(resolve,0));
    const close=()=>new Promise((resolve,reject)=>{const dialog=$('detail-dialog'),timer=setTimeout(()=>reject(Error('Detail close event missing')),3000);dialog.addEventListener('close',()=>{clearTimeout(timer);resolve();},{once:true});dialog.close();});
    request=(url,options)=>url.startsWith('/api/codex/')||url.startsWith('/api/mcp/files')||url.startsWith('/api/history?')?new Promise(resolve=>pending.push({url,signal:options?.signal,resolve})):original(url,options);
    const owner=()=>({kind:'instructions',scope:'global',instructionsLoaded:true,instructionsContent:{documents:[]},thread:data.codex.threads[0],event:{id:'synthetic',content_id:'synthetic',thread_id:data.codex.threads[0].thread_id,call_id:'synthetic-owner',operation:'read'},project:{id:'demo-project'},worktree:{id:'synthetic'},server:'demo_docs',file:{id:'synthetic'},skill:'demo-skill',namespace:'codex',section:'sql',historyCursors:[null],historyIndex:0,historyLimit:10});
    const reply=marker=>({ok:true,status:200,data:{health:'ok',marker,request:marker,response:marker,bytes:0,files:[],root:'synthetic',documents:[{relative_path:'README.md',name:'README.md',file:'README.md',text:marker}],text:[],items:[],total:0,next_cursor:null}});
    const cases=[['file',loadFileMetadata,'fileMetadata'],['project',loadProjectContent,'projectContent'],['instructions',loadInstructions,'instructionsContent'],['worktree',loadWorktreeContent,'worktreeContent'],['error',loadErrorContent,'errorContent'],['mcp',loadMcpContent,'mcpContent'],['document',loadMcpDocument,'documentContent'],['skill',loadSkillDocuments,'documents'],['sql',loadSqlContent,'sqlContent'],['git',loadGitContent,'gitContent'],['check',loadCheckContent,'checkContent'],['context',loadCommunicationContent,'contextContent'],['tool',loadToolContent,'toolContent'],['history',loadHistory,'historyPage']];
    try{
      for(const [name,load,key]of cases){
        const selected=owner();openDetail(selected,false);const first=load(selected);await tick();const old=pending.at(-1);
        openDetail(owner(),false);await tick();if(!old.signal?.aborted)throw Error(name+' request remains active after owner replacement');
        openDetail(selected,false);const second=load(selected);await tick();const current=pending.at(-1);
        if(current===old||current.signal?.aborted)throw Error(name+' cannot restart');
        old.resolve(reply('STALE-'+name));await first;await tick();
        if(String(JSON.stringify(selected[key])).includes('STALE-'))throw Error(name+' late result changed reopened owner');
        current.resolve(reply('CURRENT-'+name));await second;await tick();
        if(!String(JSON.stringify(selected[key])).includes('CURRENT-'))throw Error(name+' current result missing');
        const closing=load(selected);await tick();const closed=pending.at(-1);await close();await tick();
        if(!closed.signal?.aborted)throw Error(name+' request remains active after close');
        closed.resolve(reply('CLOSED-'+name));await closing;await tick();
        if(detail!==null||$('detail-content').children.length||detailHistory.length||skillTree)throw Error(name+' retained closed detail state');
        if([...payloadViews.keys(),...payloadObservers.keys()].some(node=>!node.isConnected||$('detail-content').contains(node)))throw Error(name+' retained payload controller');
        results.push({name,replacementAborted:true,reopenFresh:true,lateIgnored:true,closeAborted:true,controllersReleased:true});
      }
      const call={thread_id:data.codex.threads[0].thread_id,call_id:'synthetic-jev',index:0,operation:'select',timestamp:data.updated_at,completed_at:data.updated_at,duration_ms:0};
      const first=openJev(call,data.codex.threads[0],false);await tick();const old=pending.at(-1);openDetail(owner(),false);
      if(!old.signal?.aborted)throw Error('Jev request remains active after replacement');
      old.resolve(reply('STALE-JEV'));await first;await tick();
      if($('detail-content').textContent.includes('STALE-JEV'))throw Error('Jev late result changed current detail');
      const current=openJev(call,data.codex.threads[0],false);await tick();pending.at(-1).resolve(reply('CURRENT-JEV'));await current;await tick();
      if(!$('detail-content').textContent.includes('CURRENT-JEV'))throw Error('Jev current result missing');await close();await tick();
      const closing=openJev(call,data.codex.threads[0],false);await tick();const closed=pending.at(-1);await close();await tick();
      if(!closed.signal?.aborted)throw Error('Jev request remains active after close');closed.resolve(reply('CLOSED-JEV'));await closing;
      results.push({name:'jev',replacementAborted:true,reopenFresh:true,lateIgnored:true,closeAborted:true,controllersReleased:true});
      const selected=owner();openDetail(selected,false);const work=loadFileMetadata(selected);await tick();const job=pending.at(-1);dispatchEvent(new Event('pagehide'));await tick();
      if(!job.signal?.aborted||eventStream!==null||snapshotFrame!==0||cardLayouts.size)throw Error('pagehide retained owned work');
      job.resolve(reply('AFTER-PAGEHIDE'));await work;await tick();
      return {owners:results,pagehide:true};
    }finally{
      request=original;for(const job of pending)job.resolve(reply('CLEANUP'));$('detail-dialog').close();
    }
  });
  check(result.owners.length===15&&result.pagehide,'Detail lifecycle coverage');return {...result,realHttpAbort:true};
}
