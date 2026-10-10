async (page,cdp)=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&eventStreamReady&&!busy&&!queuedSnapshot);
  if(!await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  const samples=[];
  await page.evaluate(()=>{switchTab('data');display.subSummary=4;render(data);window.fixtureRenderSnapshot=renderSnapshot;window.fixtureRenderCount=0;renderSnapshot=next=>{fixtureRenderCount++;return fixtureRenderSnapshot(next);};});
  try{
    for(let batch=0;batch<6;batch++){
      const sample=await page.evaluate(async batch=>{
        const snapshot=structuredClone(data),initial=fixtureRenderCount,start=performance.now(),first=(eventVersion??0)+1;
        snapshot.codex.file_activity={events:snapshot.codex.file_activity?.events||[],total:2973,operations:{read:2973}};
        const size=new TextEncoder().encode(JSON.stringify({version:first,window:activeSourceWindow(),snapshot,logs:logData})).byteLength;
        for(let index=0;index<200;index++){
          snapshot.codex.file_activity.total=2973+batch*200+index;
          eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:first+index,window:activeSourceWindow(),snapshot,logs:logData})}));
        }
        await new Promise(resolve=>requestAnimationFrame(resolve));await new Promise(resolve=>requestAnimationFrame(resolve));
        const final=2973+batch*200+199,amount=document.querySelector('#data-cards [data-summary-key="1"] .value');
        if(data.codex.file_activity.total!==final||Number(amount.textContent.replaceAll(',',''))!==final)throw Error('Latest total must reach data and summary');
        if(fixtureRenderCount-initial!==1)throw Error('Burst must render once per frame');
        if(queuedSnapshot||busy)throw Error('Burst must drain its latest frame');
        return {frames:200,renders:fixtureRenderCount-initial,payloadBytes:size,elapsedMs:Math.round(performance.now()-start),elements:document.querySelectorAll('*').length};
      },batch);
      await cdp.send('HeapProfiler.collectGarbage');
      const heap=await cdp.send('Runtime.getHeapUsage');sample.retainedHeapBytes=heap.usedSize;samples.push(sample);
    }
    const stable=samples.slice(2),growth=stable.at(-1).retainedHeapBytes-stable[0].retainedHeapBytes;
    if(growth>Math.max(1_000_000,stable[0].retainedHeapBytes*.1))throw Error('Retained heap keeps growing '+JSON.stringify(samples));
    if(Math.max(...stable.map(sample=>sample.elements))-Math.min(...stable.map(sample=>sample.elements))>10)throw Error('DOM nodes keep growing '+JSON.stringify(samples));
    await page.mouse.move(0,0);await page.locator('[data-tab="monitor"]').focus();
    const debugBurst=await page.evaluate(async()=>{
      switchTab('monitor');
      const original=renderMonitorDebug,initial=fixtureRenderCount,latest=structuredClone(data.monitor.debug.records[0]||{kind:'collection'});
      const diagnostic={...data.monitor.debug,enabled:true,records:Array.from({length:150},(_,index)=>({...latest,kind:'collection',cpu_ms:index,process_cpu_ms:index}))};
      let renders=0;renderMonitorDebug=()=>{renders++;return original();};
      try{
        for(let index=0;index<200;index++){diagnostic.records[0].process_cpu_ms=index;eventStream.dispatchEvent(new MessageEvent('debug',{data:JSON.stringify(diagnostic)}));}
        await new Promise(resolve=>requestAnimationFrame(resolve));await new Promise(resolve=>requestAnimationFrame(resolve));
        if(renders!==1||fixtureRenderCount!==initial||data.monitor.debug.records[0].process_cpu_ms!==199||queuedDebug)throw Error('Debug-only burst must apply latest once '+renders);
        const debugOnly=renders,first=(eventVersion??0)+1,snapshot=structuredClone(data);
        for(let index=0;index<200;index++){
          eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:first+index,window:activeSourceWindow(),snapshot,logs:logData})}));
          diagnostic.records[0].process_cpu_ms=200+index;eventStream.dispatchEvent(new MessageEvent('debug',{data:JSON.stringify(diagnostic)}));
        }
        await new Promise(resolve=>requestAnimationFrame(resolve));await new Promise(resolve=>requestAnimationFrame(resolve));
        if(renders-debugOnly!==1||fixtureRenderCount-initial!==1||data.monitor.debug.records[0].process_cpu_ms!==399||queuedDebug||queuedSnapshot)throw Error('Snapshot and Debug burst must share one render');
        return {records:150,debugEvents:400,snapshotEvents:200,debugOnlyRenders:debugOnly,combinedRenders:renders-debugOnly};
      }finally{renderMonitorDebug=original;}
    });
    return {frames:1200,renders:6,samples,retainedGrowthBytes:growth,debugBurst};
  }finally{await page.evaluate(()=>{renderSnapshot=fixtureRenderSnapshot;delete window.fixtureRenderSnapshot;delete window.fixtureRenderCount;});}
}
