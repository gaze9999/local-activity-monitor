async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);};
  await page.evaluate(()=>{locale='zh-TW';applyLanguage();switchTab('codex');});
  check(await page.evaluate(()=>typeof WorkbenchUI.bindUpdateGuard==='function'),'current shared source required');
  const push=async total=>{
    await page.evaluate(total=>{const snapshot=structuredClone(data);snapshot.codex.threads[0].tokens.total_tokens=total;eventStream.dispatchEvent(new MessageEvent('snapshot',{data:JSON.stringify({version:(eventVersion??0)+1,window:activeSourceWindow(),snapshot,logs:logData})}));},total);
    await page.waitForFunction(()=>!busy&&!queuedSnapshot&&!snapshotFrame);
  };
  await page.mouse.move(1,1);
  await page.evaluate(()=>document.activeElement?.blur());
  const original=await page.evaluate(()=>data.codex.threads[0].tokens.total_tokens);
  await page.locator('#snapshot-pause').click();
  await push(original+1);await push(original+2);
  check(await page.evaluate(original=>displayPaused&&pausedSnapshot?.value.snapshot.codex.threads[0].tokens.total_tokens===original+2&&data.codex.threads[0].tokens.total_tokens===original,original),'pause retains display and coalesces latest snapshot');
  await page.locator('#snapshot-pause').click();
  await page.waitForFunction(original=>!displayPaused&&data.codex.threads[0].tokens.total_tokens===original+2,original);
  check(await page.locator('#snapshot-pause svg').count()===1,'pause control uses SVG');
  await page.evaluate(()=>{const view=tableViews.get('codex-rows');view.interactionRoot.dispatchEvent(new PointerEvent('pointerenter',{pointerType:'mouse'}));document.getElementById('codex-rows').querySelector('tr').focus();});
  await push(original+3);
  check(await page.evaluate(()=>tableViews.get('codex-rows').pendingUpdate),'hover or keyboard focus retains table');
  await page.evaluate(()=>window.dispatchEvent(new Event('blur')));
  await page.waitForFunction(()=>!tableViews.get('codex-rows').pendingUpdate);
  check(await page.evaluate(()=>!tableViews.get('codex-rows').updateGuard.held),'visible window blur releases transient interaction');
  await page.evaluate(()=>{document.activeElement?.blur();window.dispatchEvent(new Event('focus'));const root=tableViews.get('codex-rows').interactionRoot;root.dispatchEvent(new PointerEvent('pointerleave'));root.dispatchEvent(new PointerEvent('pointerenter',{pointerType:'touch'}));});
  check(await page.evaluate(()=>!tableViews.get('codex-rows').updateGuard.held),'touch does not create permanent hover');
  await page.evaluate(()=>{const node=document.getElementById('codex-rows').querySelector('td'),range=document.createRange();range.selectNodeContents(node);document.getSelection().removeAllRanges();document.getSelection().addRange(range);});
  await page.waitForFunction(()=>tableViews.get('codex-rows').updateGuard.held);
  await push(original+4);
  check(await page.evaluate(()=>tableViews.get('codex-rows').pendingUpdate),'text selection retains rows');
  await page.evaluate(()=>document.getSelection().removeAllRanges());
  await page.waitForFunction(()=>!tableViews.get('codex-rows').pendingUpdate);
  const response=page.waitForResponse(response=>new URL(response.url()).pathname==='/api/diagnostics');
  await page.evaluate(()=>reportFrontendError({name:'TypeError',stack:'TypeError\n    at '+location.origin+'/:999:10'},'render'));
  check((await response).ok(),'anonymous diagnostic frame accepted');
  await page.evaluate(()=>{const snapshot=structuredClone(data);snapshot.mcp.events.push({server:'web',tool:'web.run',metadata:{references:['https://example.org:bad/page','https://example.org/good']},result:{references:[]}});renderSnapshot(snapshot);});
  check(await page.evaluate(()=>references(data.mcp.events).every(ref=>new URL(ref.url))),'invalid cached URL does not interrupt rendering');
  await page.evaluate(()=>{window.fixtureRenderSnapshot=renderSnapshot;renderSnapshot=()=>{renderSnapshot=fixtureRenderSnapshot;throw new TypeError('Synthetic render failure');};});
  const failed=page.waitForResponse(response=>new URL(response.url()).pathname==='/api/diagnostics');
  await push(original+5);check((await failed).ok(),'render failure diagnostic accepted');
  check(await page.evaluate(()=>snapshotDisplayState==='error'&&![...document.querySelectorAll('.wb-data-state button,.wb-data-status button')].some(button=>button.checkVisibility()&&button.textContent.includes('重試'))),'source update error has no manual retry');
  await push(original+6);check(await page.evaluate(()=>snapshotDisplayState==='ready'),'next source update recovers rendering');
  const cadence=await page.evaluate(()=>{
    const nativeTimeout=window.setTimeout,delays=[];window.setTimeout=(callback,delay,...args)=>{delays.push(delay);return nativeTimeout(callback,delay,...args);};
    try{lastInteraction=performance.now();connectionDeadline=null;scheduleChrome();const active=delays.at(-1);lastInteraction-=61000;scheduleChrome();const idle=delays.at(-1);return {active,idle};}finally{window.setTimeout=nativeTimeout;lastInteraction=performance.now();scheduleChrome();}
  });
  check(cadence.active===1000&&cadence.idle===15000,'idle display cadence');
  await page.evaluate(()=>{data.codex.connection={state:'response',observed_at:new Date().toISOString(),recent_seconds:300};renderConnectionStatus();window.fixtureDateNow=Date.now;Date.now=()=>fixtureDateNow()+301000;scheduleChrome();});
  await page.waitForFunction(()=>document.getElementById('codex-connection').dataset.state==='unknown');
  await page.evaluate(()=>{Date.now=fixtureDateNow;renderConnectionStatus();});
  await page.locator('#snapshot-pause').click();
  await page.evaluate(()=>{Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));});
  check(await page.evaluate(()=>chromeTimer===0&&eventStream===null),'hidden page stops display timer and stream');
  await page.evaluate(()=>{delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));});
  await page.waitForFunction(()=>eventStreamReady&&!busy&&pausedSnapshot);
  check(await page.evaluate(()=>displayPaused&&pausedSnapshot&&document.getElementById('snapshot-pause').getAttribute('aria-pressed')==='true'),'foreground preserves manual pause');
  const sourceWindow=await page.locator('#source-window-global').inputValue();
  await page.locator('#source-window-global').selectOption(sourceWindow==='1h'?'24h':'1h');
  await page.waitForFunction(()=>!displayPaused&&!busy&&data.codex.activity_scope.window===activeSourceWindow());
  check(await page.evaluate(()=>chromeTimer!==0),'source range change resumes requested data');
  await page.locator('#source-window-global').selectOption(sourceWindow);
  await page.waitForFunction(()=>!busy&&data.codex.activity_scope.window===activeSourceWindow());
  const fileCadence=await page.evaluate(()=>{
    const files=document.getElementById('view-files'),parent=document.getElementById('view-data'),selected=activeSourceWindow(),previous={filesHidden:files.hidden,parentHidden:parent.hidden,attempt:fileSnapshotAttempts.get(selected),interaction:lastInteraction},nativeLoad=loadFileSizes,calls=[];
    loadFileSizes=()=>{calls.push(Date.now());fileSnapshotAttempts.set(selected,Date.now());};
    try{
      parent.hidden=files.hidden=false;lastInteraction=performance.now()-61000;
      fileSnapshotAttempts.set(selected,Date.now()-120000);renderFiles();const idleEarly=calls.length;
      fileSnapshotAttempts.set(selected,Date.now()-301000);renderFiles();const idleEligible=calls.length;
      lastInteraction=performance.now();fileSnapshotAttempts.set(selected,Date.now()-61000);renderFiles();const activeEligible=calls.length;
      parent.hidden=true;fileSnapshotAttempts.set(selected,0);renderFiles();const inactive=calls.length;
      return {idleEarly,idleEligible,activeEligible,inactive};
    }finally{loadFileSizes=nativeLoad;files.hidden=previous.filesHidden;parent.hidden=previous.parentHidden;lastInteraction=previous.interaction;if(previous.attempt===undefined)fileSnapshotAttempts.delete(selected);else fileSnapshotAttempts.set(selected,previous.attempt);scheduleChrome();}
  });
  check(fileCadence.idleEarly===0&&fileCadence.idleEligible===1&&fileCadence.activeEligible===2&&fileCadence.inactive===2,'file metadata checks follow visibility and idle cadence');
  return {pause:'latest snapshot, retained on foreground',interaction:'hover, keyboard, selection, touch, blur',recovery:'next source event',diagnostics:'accepted',cadence,fileCadence,visibility:'restored',sourceWindow:'explicit change resumes',statusExpiry:'automatic'};
}
