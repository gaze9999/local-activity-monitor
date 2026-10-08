async page => {
  const check=(value,message)=>{if(!value)throw Error(message);},errors=[],onError=error=>errors.push(error.message),origin=new URL(page.url()).origin;
    const closeModal=async()=>{const modal=page.locator('dialog[open]').last();await modal.locator('button[id$="-close"]').click();await page.waitForFunction(()=>!document.querySelector('dialog[open]'));await page.evaluate(()=>new Promise(resolve=>setTimeout(resolve,0)));};
  page.on('pageerror',onError);
  try {
    await page.emulateMedia({reducedMotion:'no-preference'});
    await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    await page.evaluate(()=>{if(!data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('Isolated fixture required');localStorage.removeItem(preferenceKey);});
    await page.goto(origin);await page.setViewportSize({width:1280,height:900});
    await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic fixture required');
    check(await page.evaluate(()=>[...document.querySelectorAll('[data-child-summary]')].every(root=>root.hidden&&!root.childElementCount)),'child summaries hidden by default');
    await page.getByRole('button',{name:'設定',exact:true}).click();
    const settings=page.locator('#settings-dialog'),reduce=settings.getByRole('switch',{name:'減少動畫效果',exact:true});
    check(!await reduce.isChecked(),'motion enabled by default');
    await page.evaluate(()=>{
      const UI=WorkbenchUI,root=UI.element('section');root.id='motion-fixture';document.body.append(root);
      const state=UI.createState(root);state.setState('loading');const skeleton=UI.createSkeleton(root),toggle=UI.createSwitch(root,{label:'Synthetic switch'}),progress=UI.createProgress(root,{label:'Synthetic progress',value:25}),menu=UI.createMenu(root,{label:'Synthetic menu',items:[{label:'Synthetic action',action(){}}]}),tree=UI.createTree(root,{nodes:[{id:'demo',label:'Synthetic node'}]});
      menu.open();const target=UI.button('Synthetic popup'),popup=UI.bindPopover(target,{label:'Synthetic popup',content:'Synthetic only'});root.append(target);popup.open();
      const tooltip=UI.bindTooltip(target,{text:'Synthetic tooltip',delay:0});target.dispatchEvent(new FocusEvent('focusin'));
      window.motionFixture={root,controllers:[state,skeleton,toggle,progress,menu,tree,popup,tooltip]};
    });
    const motion=()=>page.evaluate(()=>{
      const root=document.querySelector('#motion-fixture'),read=(node,pseudo)=>{const s=getComputedStyle(node,pseudo);return {animation:s.animationName,transition:s.transitionDuration};};
      return {modal:read(document.querySelector('#settings-dialog')),backdrop:read(document.querySelector('#settings-dialog'),'::backdrop'),tab:read(document.querySelector('#tab-tools')),switch:read(document.querySelector('#reduce-motion')),knob:read(root.querySelector('.wb-switch'),'::before'),skeleton:read(root.querySelector('.wb-skeleton-line')),spinner:read(root.querySelector('.wb-state'),'::before'),menu:read(root.querySelector('.wb-menu-items')),popover:read(motionFixture.controllers[6].element),tree:read(root.querySelector('.wb-tree-view'))};
    });
    const enabled=await motion();check(enabled.modal.animation==='wb-appear'&&enabled.skeleton.animation==='wb-pulse'&&enabled.spinner.animation==='wb-spin'&&enabled.tab.transition.includes('0.12s'),'shared component motion enabled');
    await reduce.check();const disabled=await motion();
    check(Object.values(disabled).every(value=>value.animation==='none'&&value.transition==='0s'),'reduced motion disables animations, transitions and pseudo-elements '+JSON.stringify(disabled));
    const persisted=await page.evaluate(()=>{const value=configuration();return {value,valid:readConfiguration(value).preferences.appearance.reduceMotion};});
    check(persisted.valid===true,'reduced motion export/import retains true');
    check(await page.evaluate(()=>{const value=structuredClone(configuration());value.preferences.appearance.reduceMotion='false';try{readConfiguration(value);return false;}catch{return true;}}),'invalid motion import rejected');
    await page.evaluate(()=>{for(const controller of motionFixture.controllers)controller.destroy();motionFixture.root.remove();delete window.motionFixture;});
    await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    check(await page.evaluate(()=>appearance.reduceMotion&&document.documentElement.dataset.wbMotion==='false'),'motion preference survives reload');
    await page.getByRole('button',{name:'設定',exact:true}).click();await reduce.uncheck();
    await page.emulateMedia({reducedMotion:'reduce'});
    check(await settings.evaluate(node=>getComputedStyle(node).animationName==='none'&&getComputedStyle(node.querySelector('input')).transitionDuration==='0s'),'system reduced motion respected');
    await page.emulateMedia({reducedMotion:'no-preference'});await settings.getByRole('tab',{name:'顯示',exact:true}).click();
    const childCount=settings.getByRole('combobox',{name:'子分頁摘要卡預設數量',exact:true});check(await childCount.inputValue()==='0','child summary default is hidden');
    await childCount.selectOption('3');await closeModal();
    await page.getByRole('tab',{name:'工具',exact:true}).click();await page.locator('#tools-tabs').getByRole('tab',{name:'MCP',exact:true}).click();
    check(await page.evaluate(()=>document.querySelector('#mcp-cards').children.length===3&&!document.querySelector('#mcp-cards').hidden),'global child summary opt-in after lazy subpage mount');
    await page.getByRole('button',{name:'設定',exact:true}).click();await settings.getByRole('tab',{name:'顯示',exact:true}).click();await childCount.selectOption('0');await closeModal();
    await page.getByRole('button',{name:'MCP Tab 設定',exact:true}).click();
    const tabSettings=page.locator('#tab-dialog'),count=tabSettings.getByRole('combobox',{name:'摘要卡數量',exact:true});
    await tabSettings.getByRole('tab',{name:'摘要卡',exact:true}).click();
    await count.selectOption('3');check(await page.evaluate(()=>document.querySelector('#mcp-cards').children.length===3),'per-subpage count opt-in');
    await closeModal();await page.locator('#tools-tabs').getByRole('tab',{name:'技能',exact:true}).click();
    check(await page.evaluate(()=>document.querySelector('#skill-cards').hidden),'other subpage stays hidden');
    await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length>0);await page.locator('#tools-tabs').getByRole('tab',{name:'MCP',exact:true}).click();
    check(await page.evaluate(()=>document.querySelector('#mcp-cards').children.length===3&&display.subSummary===0),'independent child override survives reload');
    await page.getByRole('button',{name:'MCP Tab 設定',exact:true}).click();await tabSettings.getByRole('tab',{name:'摘要卡',exact:true}).click();await count.selectOption('0');
    check(await page.evaluate(()=>{const value=configuration();return readConfiguration(value).preferences.summaries['mcp-cards'].count===0;}),'hidden child summary export/import');
    check(await page.evaluate(()=>{const value=structuredClone(configuration());value.preferences.summaries['tool-cards']={count:0};try{readConfiguration(value);return false;}catch{return true;}}),'main summary cannot import zero');
    await count.selectOption('inherit');await closeModal();
    await page.evaluate(()=>{
      const sql="select 0 as count from demo where note = '[已隱藏]';\n"+Array.from({length:900},(_,i)=>'-- Synthetic comment '+i+' '+'.'.repeat(35)).join('\n')+'\nselect 1 as final_value;';
      const io=window.IntersectionObserver;window.IntersectionObserver=undefined;let reads=0;
      const value={content_page:true,format:'text',text:sql.slice(0,32768),next:32768,total:sql.length,load:async()=>{reads++;return {text:sql.slice(32768),next:null};}};
      openDetail({kind:'sqlite',event:{id:'synthetic-sql',timestamp:new Date().toISOString(),statement:'SELECT',operation:'select',engine:'demo',tool:'exec'},sqlLoaded:true,sqlContent:{sql:value}});
      window.IntersectionObserver=io;window.parserFixture={sql,reads:()=>reads};
    });
    const dialog=page.locator('#detail-dialog'),sqlOutput=dialog.locator('.wb-output').first();check(await dialog.locator('.wb-output').count()===2,'SQL and containing tool response use shared parsed output');
    check(await page.evaluate(()=>parserFixture.reads()===0),'SQL paging stays lazy');
    await page.evaluate(async()=>{const view=[...payloadViews.values()][0];for(let i=0;i<40&&!view.element.querySelector('.wb-output-content>.wb-button').hidden;i++)await view.loadMore();});
    check((await sqlOutput.locator('code').allTextContents()).join('')===await page.evaluate(()=>WorkbenchUI.formatSql(parserFixture.sql)),'complete SQL formatting beyond first page');
    check(await sqlOutput.locator('.wb-code-keyword').count()>0,'SQL colors');
    await sqlOutput.getByRole('button',{name:'原文',exact:true}).click();await page.evaluate(async()=>{const view=[...payloadViews.values()][0];for(let i=0;i<40&&!view.element.querySelector('.wb-output-content>.wb-button').hidden;i++)await view.loadMore();});
    check((await sqlOutput.locator('code').allTextContents()).join('')===await page.evaluate(()=>parserFixture.sql),'SQL raw exact and complete');await closeModal();
    const yaml='enabled: false\ncount: 0\nmissing: null\nname: "<img src=x onerror=alert(1)>"\n';
    await page.evaluate(yaml=>openDetail({kind:'mcp-file',server:'demo_docs',file:{name:'demo.yaml'},documentContent:{id:'synthetic-yaml',path:'demo.yaml',format:'yaml',text:yaml,editable:false}}),yaml);
    const yamlCount=await dialog.locator('.wb-output').count(),unsafeCount=await dialog.locator('img,script').count();check(yamlCount===1&&!unsafeCount,'YAML file uses safe shared parser '+JSON.stringify({yamlCount,unsafeCount,text:await dialog.innerText()}));
    check((await dialog.locator('code').allTextContents()).join('').includes('false')&&(await dialog.locator('code').allTextContents()).join('').includes('null'),'YAML preserves false zero null');
    await dialog.getByRole('button',{name:'原文',exact:true}).click();check(await dialog.locator('code').textContent()===yaml,'YAML raw exact');await closeModal();
    await page.evaluate(()=>openDetail({kind:'mcp-file',server:'demo_docs',file:{name:'edit.yaml'},documentContent:{id:'synthetic-edit',path:'edit.yaml',format:'yaml',text:'count: 0\n',editable:true}}));
    const fold=dialog.locator('details').filter({has:page.getByText('內容預覽',{exact:true})}),editor=dialog.getByRole('textbox',{name:'檔案內容',exact:true});
    await fold.evaluate(node=>node.open=false);await fold.locator('summary').click();await dialog.locator('.wb-output').waitFor();await fold.locator('summary').click();await editor.fill('count: 1\n');await fold.locator('summary').click();
    await page.waitForFunction(()=>document.querySelector('#detail-dialog .wb-output code')?.textContent.includes('1'));
    check((await dialog.locator('.wb-output code').textContent()).includes('1')&&await page.evaluate(()=>payloadViews.size===1),'editable YAML preview updates without leaked controllers');
    const layouts=[];
    for(const mode of ['dark','light'])for(const width of [1280,820,390,320]){await page.setViewportSize({width,height:900});await page.evaluate(mode=>{appearance.mode=mode;applyAppearance();},mode);layouts.push(await dialog.evaluate(node=>({width:innerWidth,mode:document.documentElement.dataset.mode,overflow:node.getBoundingClientRect().right>innerWidth+1||node.querySelector('.wb-output').getBoundingClientRect().right>node.getBoundingClientRect().right+1})));}
    check(layouts.every(item=>!item.overflow),'parser responsive layout');await closeModal();
    await page.getByRole('button',{name:'設定',exact:true}).click();
    for(const locale of ['en','ja','zh-TW']){await page.locator('#language-select').selectOption(locale);check(await page.locator('#reduce-motion').evaluate(node=>node.labels[0].textContent.trim()===({en:'Reduce motion',ja:'アニメーションを減らす','zh-TW':'減少動畫效果'})[document.documentElement.lang]),'localized motion label');}
    await closeModal();
    check(await page.evaluate(()=>payloadViews.size===0&&payloadObservers.size===0),'all output controllers disposed');check(!errors.length,'browser errors '+errors.join(','));
    return {passed:true,sqlChars:await page.evaluate(()=>parserFixture.sql.length),enabled,disabled,layouts,errors};
  } finally {
    await page.emulateMedia({reducedMotion:'no-preference'});
    await page.evaluate(()=>{for(const dialog of document.querySelectorAll('dialog[open]'))dialog.close();if(window.motionFixture){for(const controller of motionFixture.controllers)controller.destroy();motionFixture.root.remove();delete window.motionFixture;}delete window.parserFixture;});
    page.off('pageerror',onError);
  }
}
