async page => {
  const origin=new URL(page.url()).origin,check=(value,message)=>{if(!value)throw Error(message);},errors=[],onError=error=>errors.push(error.message);
  page.on('pageerror',onError);
  try {
    await page.goto(origin);await page.waitForFunction(()=>data?.codex?.threads?.length>0);await page.evaluate(()=>{if(!data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');localStorage.removeItem(preferenceKey);});await page.reload();await page.setViewportSize({width:1600,height:1000});await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'synthetic fixture required');
    await page.evaluate(()=>{clearInterval(refreshTimer);appearance.reduceMotion=false;appearance.mode='dark';applyAppearance();});
    await page.getByRole('button',{name:'設定',exact:true}).click();const settings=page.locator('#settings-dialog');await settings.getByRole('tab',{name:'外觀',exact:true}).click();
    check(await page.locator('#theme-select option').count()===2,'obsolete slate theme removed');
    await page.locator('#accent-select').selectOption('custom');await page.locator('#accent-hex').fill('#a482e6');await page.locator('#accent-hex').press('Tab');
    await page.locator('#font-family').selectOption('Microsoft JhengHei');
    check(await page.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--accent')==='#a482e6'&&getComputedStyle(document.documentElement).fontFamily.includes('Microsoft JhengHei')),'system font appearance applied');
    check(await page.evaluate(()=>{const a=readConfiguration(configuration()).preferences.appearance;return a.accent==='custom'&&a.accentColor==='#a482e6'&&a.fontFamily==='Microsoft JhengHei';}),'system font export/import');
    for(const patch of [{accentColor:'red'},{accentColor:''},{fontFamily:'url(https://example.invalid)'},{fontFamily:'Arial; color:red'}])check(await page.evaluate(patch=>{const value=structuredClone(configuration());Object.assign(value.preferences.appearance,patch);try{readConfiguration(value);return false;}catch{return true;}},patch),'unsafe appearance rejected');
    await page.evaluate(()=>{appearance.fontFamily='Arial, sans-serif';applyAppearance();});check(await page.locator('#font-family option[value="Arial, sans-serif"]').count()===1,'saved custom font remains selectable');
    await page.locator('#font-family').selectOption('Segoe UI');
    const menus=await page.evaluate(()=>{
      const saved=appearance.mode,results=[];
      for(const mode of ['dark','light']){
        appearance.mode=mode;applyAppearance();const select=document.getElementById('font-family'),selected=getComputedStyle(select.selectedOptions[0]),other=getComputedStyle(select.options[0]);
        results.push({mode,supported:CSS.supports('appearance','base-select'),appearance:getComputedStyle(select).appearance,selected:selected.backgroundColor,other:other.backgroundColor});
      }
      appearance.mode=saved;applyAppearance();return results;
    });
    check(menus.every(menu=>!menu.supported||menu.appearance==='base-select'&&menu.selected!==menu.other),'open select theme '+JSON.stringify(menus));
    for(const locale of ['en','ja','zh-TW']){await page.locator('#language-select').selectOption(locale);check(await page.locator('#theme-select option[value=workbench]').textContent()===({en:'Workbench',ja:'ワークベンチ','zh-TW':'工作台'})[locale],'theme translation');}
    await settings.getByRole('tab',{name:'更新與追蹤',exact:true}).click();check(await page.locator('#refresh-interval').evaluate(input=>getComputedStyle(input).appearance==='textfield'&&[...document.styleSheets].some(sheet=>[...sheet.cssRules].some(rule=>rule.selectorText?.includes('inner-spin-button')&&rule.style.appearance==='none'))),'number spinner removed');
    await page.locator('#refresh-interval').fill('10');await page.locator('#refresh-interval').press('ArrowUp');check(await page.locator('#refresh-interval').inputValue()==='11','native number keyboard retained');
    await page.locator('#settings-close').click();await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    check(await page.evaluate(()=>appearance.accentColor==='#a482e6'&&appearance.fontFamily==='Segoe UI'),'system font persists');
    await page.evaluate(()=>{clearInterval(refreshTimer);appearance.accent='blue';appearance.fontFamily='';appearance.theme='slate';applyAppearance();saveView();});
    check(await page.evaluate(()=>appearance.theme==='workbench'&&!document.documentElement.style.getPropertyValue('--accent')&&!document.documentElement.style.getPropertyValue('--wb-accent')&&!document.documentElement.style.getPropertyValue('--wb-font-family')),'old theme migrates and overrides reset');
    await page.getByRole('tab',{name:'專案',exact:true}).click();await page.locator('#activity-tabs').getByRole('tab',{name:'驗證',exact:true}).click();
    const layouts=[];
    for(const width of [1600,820,390,320]){
      await page.setViewportSize({width,height:1000});
      await page.evaluate(async()=>{
        clearInterval(refreshTimer);resetChartQueue();const view=chartViews.get('check-breakdown-chart');view.statisticsRoot.replaceChildren();view.statisticsPanel.classList.add('empty-statistics');arrangeContentPanels();await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
        const target=chartTarget('check-breakdown-chart');chartJobs.set('check-breakdown-chart',()=>{view.statisticsPanel.classList.remove('empty-statistics','layout-hidden');view.statisticsPanel.hidden=false;view.statisticsRoot.replaceChildren(...Array.from({length:3},(_,index)=>{const group=node('div',null,'trend-statistic-group');group.append(node('b','Synthetic operation '+index),statisticLabels({size:25,total:33,mean:1.32,p99:12.28},ui('次'),'Synthetic'));return group;}));});chartReady.add('check-breakdown-chart');drawNextChart();
      });
      await page.waitForTimeout(100);
      layouts.push(await page.evaluate(()=>{const view=document.querySelector('#view-checks'),cards=[...view.querySelectorAll('.panel')].filter(card=>card.getBoundingClientRect().height),table=document.querySelector('#check-rows').closest('.panel').getBoundingClientRect();let overlaps=0;for(let i=0;i<cards.length;i++)for(let j=i+1;j<cards.length;j++){const a=cards[i].getBoundingClientRect(),b=cards[j].getBoundingClientRect();if(a.left<b.right-.5&&b.left<a.right-.5&&a.top<b.bottom-.5&&b.top<a.bottom-.5)overlaps++;}return {width:innerWidth,overlaps,statisticsBottom:chartViews.get('check-breakdown-chart').statisticsPanel.getBoundingClientRect().bottom,tableTop:table.top,overflow:document.documentElement.scrollWidth>innerWidth};}));
    }
    check(layouts.every(item=>!item.overlaps&&!item.overflow&&item.statisticsBottom<item.tableTop),'deferred statistics reserve space above table '+JSON.stringify(layouts));
    await page.setViewportSize({width:1600,height:1000});
    const flicker=await page.evaluate(async()=>{resetChartQueue();const id='check-breakdown-chart',view=chartViews.get(id);view.settings.shape='column';view.settings.statistics=1;view.settings.range='all';let flashes=0;const listen=event=>{if(event.target.matches('svg,table')||event.target.closest('#'+id))flashes++;};document.addEventListener('animationstart',listen);const states=[];try{for(let round=0;round<3;round++){chartDrawing=true;try{columnChart(id,[['Synthetic',round+1]],null,null,ui('次'),view.settings);}finally{chartDrawing=false;}const svg=document.querySelector('#'+id+' svg');states.push({animation:getComputedStyle(svg).animationName,opacity:getComputedStyle(svg).opacity});await new Promise(r=>requestAnimationFrame(r));}return {flashes,states};}finally{document.removeEventListener('animationstart',listen);}});
    check(!flicker.flashes&&flicker.states.every(item=>item.animation==='none'&&item.opacity==='1'),'chart updates do not fade '+JSON.stringify(flicker));
    check(!errors.length,'browser errors '+errors.join(','));return {passed:true,layouts,flicker,errors};
  } finally {page.off('pageerror',onError);await page.goto(origin);}
}
