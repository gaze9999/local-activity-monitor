async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&eventStreamReady&&!busy);
  const result=await page.evaluate(async()=>{
    const nativeSource=window.EventSource,base=structuredClone(data),logs=structuredClone(logData);
    class Source{
      constructor(url){this.url=url;this.listeners={};}
      addEventListener(name,callback){this.listeners[name]=callback;}
      close(){this.closed=true;}
      snapshot(version,updated){this.listeners.snapshot({data:JSON.stringify({version,window:activeSourceWindow(),snapshot:{...base,updated_at:updated},logs})});}
    }
    stopEventStream();window.EventSource=Source;
    try{
      startEventStream();const stream=eventStream;startEventStream();const single=stream===eventStream;
      stream.onopen();stream.snapshot(0,'2026-10-10T00:00:01Z');await new Promise(resolve=>requestAnimationFrame(resolve));
      const initial=data.updated_at==='2026-10-10T00:00:01Z';
      stream.snapshot(0,'2026-10-10T00:00:02Z');const duplicate=data.updated_at==='2026-10-10T00:00:01Z';
      busy=true;stream.snapshot(2,'2026-10-10T00:00:03Z');stream.snapshot(3,'2026-10-10T00:00:04Z');busy=false;
      drainSnapshots();await new Promise(resolve=>requestAnimationFrame(resolve));const latest=data.updated_at==='2026-10-10T00:00:04Z';
      stream.snapshot(1,'2026-10-10T00:00:02Z');const ordered=data.updated_at==='2026-10-10T00:00:04Z'&&!connectionLost;
      stream.onerror();const disconnected=connectionLost&&!eventStreamReady;
      stream.onopen();stream.snapshot(0,'2026-10-10T00:00:05Z');await new Promise(resolve=>requestAnimationFrame(resolve));
      const recovered=!connectionLost&&data.updated_at==='2026-10-10T00:00:05Z';
      stopEventStream();const closed=stream.closed&&eventStream===null;
      return {single,initial,duplicate,latest,ordered,disconnected,recovered,closed,noFrequencyControl:!document.getElementById('refresh-interval')};
    }finally{stopEventStream();window.EventSource=nativeSource;busy=false;await refresh();}
  });
  if(!Object.values(result).every(Boolean))throw Error(JSON.stringify(result));
  await page.waitForFunction(()=>eventStreamReady);
  return result;
}
