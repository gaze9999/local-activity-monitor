async page=>{
  await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length&&eventStreamReady);
  const result=await page.evaluate(()=>{
    const nativeSource=window.EventSource,nativeRefresh=refresh,savedBusy=busy,savedQueued=refreshQueued;
    let calls=0;
    class Source{
      constructor(){this.listeners={};}
      addEventListener(name,callback){this.listeners[name]=callback;}
      close(){this.closed=true;}
      snapshot(version){this.listeners.snapshot({data:JSON.stringify({version})});}
    }
    stopEventStream();window.EventSource=Source;refresh=()=>{calls++;};busy=false;refreshQueued=false;
    try{
      startEventStream();const stream=eventStream;startEventStream();const single=stream===eventStream;
      stream.onopen();stream.snapshot(0);stream.snapshot(0);const initial=calls===1;
      stream.onerror();const fallback=!eventStreamReady;
      stream.onopen();stream.snapshot(0);const restart=calls===2;
      stream.snapshot(7);stream.snapshot(7);const duplicate=calls===3;
      busy=true;stream.snapshot(8);const queued=refreshQueued&&calls===3;
      stopEventStream();const closed=stream.closed&&eventStream===null&&!eventStreamReady;
      return {single,initial,fallback,restart,duplicate,queued,closed};
    }finally{stopEventStream();window.EventSource=nativeSource;refresh=nativeRefresh;busy=savedBusy;refreshQueued=savedQueued;startEventStream();}
  });
  if(!Object.values(result).every(Boolean))throw Error(JSON.stringify(result));
  await page.waitForFunction(()=>eventStreamReady);
  return {checks:result,liveLocalStream:await page.evaluate(()=>eventStream.url===location.origin+'/api/events')};
}
