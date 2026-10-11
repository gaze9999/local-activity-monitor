async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);};
  await page.evaluate(()=>{stopEventStream();switchTab('overview');overviewHidden.delete('environment-chart');applyOverview();chartDrawing=true;try{bars('environment-chart',{Local:682,Cloud:88,Unknown:46,Remote:16});}finally{chartDrawing=false;}setSnapshotState('ready');});
  const layouts=[];
  for(const locale of ['zh-TW','en','ja'])for(const mode of ['dark','light'])for(const width of [1366,390,320]){
    await page.setViewportSize({width,height:900});
    await page.evaluate(({language,mode})=>{locale=language;applyLanguage();Object.assign(appearance,{theme:'sand',mode,font:24});applyAppearance();},{language:locale,mode});
    const result=await page.evaluate(()=>{
      const read=element=>{const {x,y,width,height}=element.getBoundingClientRect();return {x,y,width,height};};
      return ['activity-chart','environment-chart'].map(id=>{
        const graph=$(id),panel=graph.closest('.panel');setPanelDataState(panel,'ready');
        const before=read(panel),heading=read(panel.querySelector('h3')),graphBefore=read(graph),child=graph.firstElementChild,button=panel.querySelector('button');button?.focus({preventScroll:true});
        setPanelDataState(panel,'error');
        const entry=panelDataState(panel),style=getComputedStyle(panel),status=entry.failureStatus;
        const result={id,before,after:read(panel),heading,headingAfter:read(panel.querySelector('h3')),graphBefore,graphAfter:read(graph),sameChild:child===graph.firstElementChild,focused:!button||document.activeElement===button,stale:panel.classList.contains('panel-update-failed'),borderStyle:style.borderTopStyle,status:status.textContent,role:status.getAttribute('role'),clipped:getComputedStyle(status).clip,feedbackVisible:[...panel.querySelectorAll('.wb-data-feedback')].some(node=>node.checkVisibility()&&node.getBoundingClientRect().width>1)};
        setPanelDataState(panel,'refreshing');result.retryRetainsEffect=panel.classList.contains('panel-update-failed');setPanelDataState(panel,'ready');result.recovered=!panel.classList.contains('panel-update-failed')&&!status.textContent;return result;
      });
    });
    for(const item of result){
      const equal=(first,second)=>Object.keys(first).every(key=>Math.abs(first[key]-second[key])<.5);
      check(item.before.width>0&&item.before.height>0&&equal(item.before,item.after)&&equal(item.heading,item.headingAfter)&&equal(item.graphBefore,item.graphAfter),'retained error does not change geometry '+JSON.stringify({locale,mode,width,item}));
      check(item.stale&&item.borderStyle==='dashed'&&!item.feedbackVisible&&item.status&&item.role==='status'&&item.clipped!=='auto','effect and accessible status '+item.id);
      check(item.sameChild&&item.focused&&item.retryRetainsEffect&&item.recovered,'content, focus, retry and recovery '+JSON.stringify({locale,mode,width,item}));
    }
    layouts.push({locale,mode,width,panels:result.length});
  }
  await page.evaluate(()=>{switchTab('codex');const panel=$('codex-rows').closest('.panel');setPanelDataState(panel,'ready');window.errorRows=[...$('codex-rows').children];setPanelDataState(panel,'error');});
  check(await page.evaluate(()=>$('codex-rows').closest('.panel').classList.contains('panel-update-failed')&&errorRows.every((row,index)=>row===$('codex-rows').children[index])),'table retains rows with effect');
  await page.emulateMedia({reducedMotion:'reduce'});
  check(await page.evaluate(()=>$('codex-rows').closest('.panel').getAnimations().length===0),'failure effect works without animation');
  await page.evaluate(()=>{const panel=$('codex-rows').closest('.panel');setPanelDataState(panel,'ready');panelDataState(panel).loaded=false;setPanelDataState(panel,'error');});
  check(await page.locator('#codex-rows').evaluate(rows=>!rows.closest('.panel').classList.contains('panel-update-failed')),'initial failure does not claim retained data');
  check(await page.locator('#view-codex .wb-data-feedback[data-state=error]').isVisible(),'initial failure retains visible explanation');
  await page.setViewportSize({width:1366,height:900});await page.emulateMedia({reducedMotion:'no-preference'});
  await page.evaluate(()=>{locale='zh-TW';applyLanguage();Object.assign(appearance,{theme:'sand',mode:'dark',font:15});applyAppearance();switchTab('overview');});
  for(const id of ['activity-chart','environment-chart']){await page.locator('#'+id).scrollIntoViewIfNeeded();await page.waitForFunction(id=>!chartJobs.has(id),id);}
  await page.evaluate(()=>{chartDrawing=true;try{bars('environment-chart',{Local:682,Cloud:88,Unknown:46,Remote:16});}finally{chartDrawing=false;}setSnapshotState('ready');setSnapshotState('error');});
  check(await page.evaluate(()=>['activity-chart','environment-chart'].every(id=>getComputedStyle($(id).closest('.panel')).borderTopStyle==='dashed')),'snapshot error keeps both warning borders');
  await page.locator('#view-overview .panel:has(#environment-chart)').screenshot({path:'.local/retained-error-final.png',animations:'disabled'});await page.locator('#view-overview .panel:has(#activity-chart)').screenshot({path:'.local/retained-error-chart-final.png',animations:'disabled'});
  return {layouts,tableRetained:true,initialFailure:true,reducedMotion:true};
}
