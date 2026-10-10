async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  if(!await page.evaluate(()=>data.codex.threads?.length>0&&data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))))throw Error('Isolated synthetic fixture required');

  const errors=[],onError=e=>errors.push(e.message);page.on('pageerror',onError);
  await page.evaluate(()=>document.getElementById('thread-search').value='preserved filter');
  let failed,recovered;
  try{
    await page.context().setOffline(true);
    await page.evaluate(()=>refresh());
    await page.waitForFunction(()=>connectionLost&&!eventStreamReady);
    failed=await page.evaluate(()=>({lost:connectionLost,retrying:eventStream?.readyState===EventSource.CONNECTING,label:document.getElementById('live').textContent}));
    if(!failed.lost||!failed.retrying)throw Error(JSON.stringify(failed));
    await page.context().setOffline(false);
    await page.waitForFunction(()=>!connectionLost&&eventStreamReady&&!busy,null,{timeout:20000});
    recovered=await page.evaluate(()=>({lost:connectionLost,ready:eventStreamReady,filter:document.getElementById('thread-search').value}));
  }finally{await page.context().setOffline(false);page.off('pageerror',onError);}
  const states=await page.evaluate(()=>{
    const old=data;const out={};
    for(const [health,files]of [['ok',1],['ok',0],['missing',0],['unavailable',1]]){data={...old,settings:{...old.settings,observations:{...old.settings.observations,codex:true}},activity:{paused:false},codex:{...old.codex,connection:{state:'unknown'},health,files,threads:[]}};idlePaused=false;renderConnectionStatus();out[health+'-'+files]=document.getElementById('codex-live').textContent;}
    data=old;renderConnectionStatus();return out;
  });
  if(recovered.lost||!recovered.ready||recovered.filter!=='preserved filter'||states['ok-1']!=='來源可讀取'||states['ok-0']!=='等待活動'||states['missing-0']!=='未找到來源'||states['unavailable-1']!=='來源讀取異常'||errors.length)throw Error(JSON.stringify({recovered,states,errors}));
  return {failed,recovered,states,errors};
}
