async page=>{
  const check=(ok,message)=>{if(!ok)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic fixture required');
  const result=await page.evaluate(async()=>{
    stopEventStream();const marker='PRIVATE_SENTINEL_DO_NOT_SHARE',key=marker+'-numeric';
    appearance.fontFamily=marker;preferences.copy={private:marker};preferences.charts={[marker]:{range:'custom',shape:'line',length:30,[key]:77,start:marker,end:marker}};
    tableStates[marker]={size:20,page:1,filters:{name:marker},columns:[marker],[key]:88};
    preferences.summaries={private:{count:4,custom:[{id:marker,title:marker,source:'monitor.cpu'}],sources:{[marker]:'monitor.cpu'}}};
    preferences.cardLayouts={private:{groups:[{id:marker,title:marker}],custom:[{id:marker,title:marker,source:'monitor.cpu'}],assignments:{[marker]:marker}}};
    data.settings[key]=99;saveView();const before=JSON.stringify(preferences),stored=localStorage.getItem(preferenceKey),plain=JSON.stringify(diagnosticConfiguration());
    if(plain.includes(marker))throw Error('Private sentinel in diagnostic output');
    const file=await diagnosticSettingsFile(),decoded=await new Response(file.blob.stream().pipeThrough(new DecompressionStream('gzip'))).text();
    if(decoded!==plain||!file.name.endsWith('.json.gz'))throw Error('Compressed settings round trip');
    const compression=window.CompressionStream;let fallback;try{window.CompressionStream=undefined;fallback=await diagnosticSettingsFile();}finally{window.CompressionStream=compression;}
    if(await fallback.blob.text()!==plain||!fallback.name.endsWith('.json'))throw Error('JSON fallback');
    if(JSON.stringify(preferences)!==before||localStorage.getItem(preferenceKey)!==stored||!stored.includes(marker))throw Error('Local settings changed by diagnostic export');
    return {privateNamesRemoved:true,privateNumericKeysRemoved:true,compressedRoundTrip:true,jsonFallback:true,localSettingsPreserved:true,bytes:file.blob.size,plainBytes:new TextEncoder().encode(plain).length};
  });return result;
}
