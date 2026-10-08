async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  if(!await page.evaluate(()=>data.codex.threads?.length>0&&data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))))throw Error('Isolated synthetic fixture required');

  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  let failures=0;
  await page.route('**/api/snapshot?**',async route=>{if(failures++===0)await route.abort('failed');else await route.continue();});
  const before=await page.evaluate(()=>{idlePaused=true;schedule(3600);return data.updated_at;});
  await page.evaluate(()=>refresh());
  const failed=await page.evaluate(()=>({lost:connectionLost,retrying:reconnectTimer!==null,label:document.getElementById('live').textContent,period:refreshSeconds}));
  if(!failed.lost||!failed.retrying||failed.period!==3600)throw Error(JSON.stringify(failed));
  await page.waitForFunction(()=>!connectionLost&&!busy);
  const recovered=await page.evaluate(()=>({lost:connectionLost,retrying:reconnectTimer!==null,label:document.getElementById('live').textContent}));
  const states=await page.evaluate(()=>{
    const old=data;const out={};
    for(const [health,files]of [['ok',1],['ok',0],['missing',0],['unavailable',1]]){data={...old,settings:{...old.settings,observations:{...old.settings.observations,codex:true}},activity:{paused:false},codex:{...old.codex,connection:{state:'unknown'},health,files,threads:[]}};idlePaused=false;renderConnectionStatus();out[health+'-'+files]=document.getElementById('codex-live').textContent;}
    data=old;renderConnectionStatus();return out;
  });
  await page.unroute('**/api/snapshot?**');
  if(recovered.lost||recovered.retrying||states['ok-1']!=='來源可讀取'||states['ok-0']!=='等待活動'||states['missing-0']!=='未找到來源'||states['unavailable-1']!=='來源讀取異常'||errors.length)throw Error(JSON.stringify({recovered,states,errors}));
  return {failed,recovered,states,requests:failures,errors};
}
