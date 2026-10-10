async page=>{
  const context=await page.context().browser().newContext(),probe=await context.newPage(),check=(value,message)=>{if(!value)throw Error(message);};
  try{
    await probe.clock.install();await probe.goto(page.url());await probe.waitForFunction(()=>data?.updated_at&&eventStreamReady&&!busy);await probe.evaluate(()=>document.fonts.ready);
    await probe.evaluate(()=>{window.fixtureUptime=renderUptime;window.fixtureTicks=0;window.fixtureBaseline=0;renderUptime=()=>{fixtureTicks++;fixtureUptime();};window.fixtureBaselineTimer=setInterval(()=>fixtureBaseline++,1000);lastInteraction=performance.now()-61000;connectionDeadline=null;scheduleChrome();});
    await probe.clock.runFor(60000);
    const counts=await probe.evaluate(()=>({baseline:fixtureBaseline,idle:fixtureTicks}));
    check(counts.baseline===60&&counts.idle===4,'60-second idle callback count '+JSON.stringify(counts));
    await probe.locator('#snapshot-pause').click();await probe.evaluate(()=>{clearInterval(fixtureBaselineTimer);connectionDeadline=null;scheduleChrome();fixtureTicks=0;});
    await probe.clock.runFor(60000);check(await probe.evaluate(()=>fixtureTicks===0),'manual pause stops nonessential timer');
    await probe.locator('#snapshot-pause').click();
    await probe.evaluate(()=>{locale='zh-TW';applyLanguage();switchTab('monitor');Object.assign(data.monitor,{processor:'Demo CPU',architecture:'AMD64',logical_cpus:12,physical_memory_bytes:32*1024**3,available_memory_bytes:16*1024**3,cpu_usage_percent:0,device_checked_at:new Date().toISOString()});renderMonitor();arrangeContentPanels();});
    check(await probe.locator('#monitor-device-updated').isVisible(),'independent device sample timestamp visible');
    const layouts=[];
    for(const width of [1436,1366,900,390])for(const language of ['zh-TW','en','ja']){
      await probe.setViewportSize({width,height:1000});await probe.evaluate(language=>{locale=language;applyLanguage();},language);await probe.clock.runFor(500);
      layouts.push(await probe.evaluate(()=>{const root=document.getElementById('monitor-hardware').closest('.panel'),heading=root.querySelector('h3'),first=root.querySelector('dt');return {width:innerWidth,locale,overflow:document.documentElement.scrollWidth>innerWidth,gap:first.getBoundingClientRect().top-heading.getBoundingClientRect().bottom};}));
    }
    check(layouts.every(item=>!item.overflow&&item.gap>=0&&item.gap<35),'hardware / header layout '+JSON.stringify(layouts));
    await probe.setViewportSize({width:1366,height:900});await probe.evaluate(()=>{locale='zh-TW';applyLanguage();});await probe.clock.runFor(500);await probe.screenshot({path:'.local/idle-hardware-layout.png',fullPage:false});
    return {virtualSeconds:60,callbacks:counts,pausedCallbacks:0,layouts};
  }finally{await context.close();}
}
