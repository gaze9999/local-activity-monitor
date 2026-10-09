async page=>{
  const errors=[],onError=error=>errors.push(error.message);page.on('pageerror',onError);
  const check=(value,label)=>{if(!value)throw Error(label);};
  await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length&&typeof WorkbenchUI.bindDisclosure==='function');
  await page.evaluate(()=>{locale='zh-TW';applyLanguage();});
  const result=await page.evaluate(async()=>{
    const check=(value,label)=>{if(!value)throw Error(label);};
    switchTab('codex');selectConversationSource('context');renderVisibleContent();attachTables();
    const parent=data.codex.threads.find(t=>t.thread_id.endsWith('000000000001')),child=data.codex.threads.find(t=>t.thread_id.endsWith('000000000002'));
    check(child.execution.parent_thread_id===parent.thread_id,'child parent identity');
    const view=tableViews.get('context-rows'),saved={...tableStates['context-rows']};
    tableStates['context-rows']={...saved,columns:['操作','父對話',...view.headers.filter(header=>!['操作','父對話'].includes(header))],hidden:['代理訊息']};
    applyTableColumns('context-rows');renderContexts();
    tableStates['context-rows']={...tableStates['context-rows'],hidden:[]};applyTableColumns('context-rows');renderContexts();
    const rows=view.rows,childRow=rows.find(row=>row.textContent.includes(title(child))&&row.textContent.includes('子代理'));
    check(childRow&&childRow._rowAction,'row opens context');childRow._rowAction();
    return {parentId:parent.thread_id,childId:child.thread_id,saved};
  });
  await page.waitForFunction(()=>detail?.contextLoaded);
  const child=await page.evaluate(()=>({id:detail.thread.thread_id,text:JSON.stringify(detail.contextContent),title:document.getElementById('detail-title').textContent,folds:[...document.querySelectorAll('#detail-content .structured-payload')].map(el=>el.open)}));
  check(child.id===result.childId&&child.text.includes('CHILD_CONTEXT_ONLY')&&!child.text.includes('PARENT_CONTEXT_ONLY'),'child context identity');check(child.title.includes('子代理')&&child.folds.every(Boolean),'child role and initial disclosure');
  await page.evaluate(id=>{document.getElementById('detail-dialog').close();const row=tableViews.get('context-rows').rows.find(row=>row.textContent.includes(title(threadIndex.get(id))));const buttons=[...row.querySelectorAll('button')];buttons.find(button=>button.textContent==='代理訊息').click();},result.childId);
  check(await page.evaluate(()=>detail.section==='agent-messages'&&document.querySelector('#detail-dialog [role="tab"][aria-selected="true"]').textContent==='代理訊息'),'message button targets correct section');
  await page.evaluate(id=>{document.getElementById('detail-dialog').close();const row=tableViews.get('context-rows').rows.find(row=>row.textContent.includes(title(threadIndex.get(id))));rowCells(row)[2].querySelector('button').click();},result.childId);
  check(await page.evaluate(id=>detail.kind==='thread'&&detail.id===id,result.parentId),'parent button remains parent after column changes');
  await page.evaluate(id=>{document.getElementById('detail-dialog').close();openDetail({kind:'context',thread:threadIndex.get(id)});},result.parentId);
  await page.waitForFunction(()=>detail?.contextLoaded);
  const parent=await page.evaluate(()=>({id:detail.thread.thread_id,text:JSON.stringify(detail.contextContent),title:document.getElementById('detail-title').textContent}));
  check(parent.id===result.parentId&&parent.text.includes('PARENT_CONTEXT_ONLY')&&!parent.text.includes('CHILD_CONTEXT_ONLY')&&parent.title.includes('父代理'),'parent context identity');
  const motion=await page.evaluate(async()=>{
    document.getElementById('detail-dialog').close();
    const root=document.createElement('details'),summary=document.createElement('summary'),body=document.createElement('div');root.append(summary,body);summary.textContent='Motion fixture';body.style.height='120px';body.textContent='Motion content';document.body.append(root);const view=WorkbenchUI.bindDisclosure(root,{content:body,duration:120});
    const wait=()=>new Promise(resolve=>requestAnimationFrame(resolve));
    summary.click();await wait();summary.click();await wait();summary.click();await Promise.all(body.getAnimations().map(a=>a.finished.catch(()=>{})));await wait();
    const rapid=root.open&&summary.getAttribute('aria-expanded')==='true'&&body.getAnimations().length===0;
    summary.click();await Promise.all(body.getAnimations().map(a=>a.finished.catch(()=>{})));await wait();const fullyClosed=!root.open;
    summary.click();await Promise.all(body.getAnimations().map(a=>a.finished.catch(()=>{})));await wait();const fullyOpen=root.open&&body.style.height==='120px';
    view.destroy();root.remove();return {rapid,fullyClosed,fullyOpen};
  });check(Object.values(motion).every(Boolean),'disclosure transitions '+JSON.stringify(motion));
  const parser=await page.evaluate(async()=>{
    const pre=document.createElement('pre'),code=document.createElement('code');pre.append(code);pre.style.cssText='position:fixed;top:100px;left:100px;width:600px;height:240px;overflow:auto;line-height:20px;white-space:pre';document.body.append(pre);
    const text=Array.from({length:1500},(_,i)=>`L${i+1}: pub async fn stream<'a>() { let value = ${i}; }`).join('\n'),renderer=WorkbenchUI.createCodeRenderer(code,{format:'rust',scrollRoot:pre});renderer.append(text,true);
    const sample=()=>[...code.querySelectorAll('.wb-code-chunk')].filter(el=>{const r=el.getBoundingClientRect(),p=pre.getBoundingClientRect();return r.bottom>p.top&&r.top<p.bottom;}).every(el=>!!el.querySelector('.wb-code-keyword'));
    const start=sample();pre.scrollTop=pre.scrollHeight-260;pre.dispatchEvent(new Event('scroll'));const end=sample();pre.scrollTop=8000;pre.dispatchEvent(new Event('scroll'));const middle=sample();const nodes=code.querySelectorAll('*').length,complete=code.textContent===text;renderer.destroy();pre.remove();return {start,end,middle,nodes,complete};
  });check(parser.start&&parser.end&&parser.middle&&parser.complete,'viewport parser '+JSON.stringify(parser));
  const layouts=[];
  await page.evaluate(saved=>{tableStates['context-rows']=saved;applyTableColumns('context-rows');appearance.font=18;applyAppearance();},result.saved);
  for(const width of [1366,820,390,320])for(const language of ['zh-TW','en','ja']){
    await page.setViewportSize({width,height:900});
    await page.evaluate(value=>{locale=value;applyLanguage();},language);
    const layout=await page.evaluate(()=>{
      const faults=[];for(const button of document.querySelectorAll('.tabs>button[data-tab][role="tab"]')){button.click();renderVisibleContent();attachTables();}
      switchTab('codex');selectConversationSource('context');renderVisibleContent();attachTables();
      const row=tableViews.get('context-rows')?.rows.find(row=>row.querySelector('.wb-action-group')),controls=[...row?.querySelectorAll('.wb-action-group>button')||[]];if(controls.length>1){const a=controls[0].getBoundingClientRect(),b=controls[1].getBoundingClientRect();if(a.width&&b.width&&b.left-a.right<4)faults.push('action gap');}
      const header=document.querySelector('.header-status'),primary=document.querySelector('.header-info-primary'),secondary=document.querySelector('.header-info-secondary');
      const line=parseFloat(getComputedStyle(secondary).lineHeight);
      return {width:innerWidth,locale,font:appearance.font,overflow:document.documentElement.scrollWidth-innerWidth,headerHeight:header.getBoundingClientRect().height,primary:primary.getBoundingClientRect().height,primaryLine:parseFloat(getComputedStyle(primary).lineHeight),secondary:secondary.getBoundingClientRect().height,line,faults,fonts:document.getElementById('font-family').options.length};
    });layouts.push(layout);check(layout.font===18&&layout.overflow<=1&&!layout.faults.length&&layout.secondary<=layout.line+1&&layout.primary<=layout.primaryLine*1.3,'layout '+JSON.stringify(layout));
  }
  await page.evaluate(()=>{locale='zh-TW';appearance.font=14;applyLanguage();});
  check(!errors.length,'page errors '+JSON.stringify(errors));page.off('pageerror',onError);
  return {identities:result,child,parent,motion,parser,layouts,errors};
}
