async page => {
  const origin=new URL(page.url()).origin,errors=[],onError=error=>errors.push(error.message);page.on('pageerror',onError);
  let phase='failed',requests=0;
  await page.route(origin+'/api/snapshot?*',async route=>{
    requests++;const response=await route.fetch(),snapshot=await response.json();
    if(!snapshot.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('This flow requires an isolated synthetic fixture');
    if(phase==='failed'){await new Promise(resolve=>setTimeout(resolve,1000));await route.fulfill({status:503,contentType:'application/json',body:'{}'});return;}
    if(phase==='missing')snapshot.codex.usage={limits:[],credits:{},plan_type:null};
    else snapshot.codex.usage.credits={balance:0,has_credits:false,unlimited:false};
    if(phase==='delayed')await new Promise(resolve=>setTimeout(resolve,1000));
    await route.fulfill({response,json:snapshot});
  });
  const check=(value,message)=>{if(!value)throw Error(message);};
  try{
    await page.evaluate(()=>localStorage.setItem('local-activity-monitor.preferences.v1',JSON.stringify({themeVersion:1,appearance:{mode:'dark',theme:'steam',accent:'blue',font:14}})));
    await page.setViewportSize({width:1600,height:1000});await page.goto(origin);
    await page.locator('section.panel').filter({has:page.locator('#overview-account-card')}).getByText('載入中',{exact:true}).waitFor();await page.locator('section.panel').filter({has:page.locator('#overview-account-card')}).getByText('連線中斷',{exact:true}).waitFor();
    phase='ready';await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await page.locator('#overview-account-card').getByText('pro',{exact:true}).waitFor();
    const account=await page.locator('#overview-account-card').innerText();check(account.includes('0')&&account.includes('否'),'zero/false account values');check(await page.getByRole('tab',{name:'用量與額度',exact:true}).getAttribute('aria-selected')==='false','cold overview must not require usage page');
    const height=await page.locator('#overview-account-card').evaluate(node=>node.closest('.panel').getBoundingClientRect().height);phase='delayed';await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await page.waitForFunction(()=>document.querySelector('#overview-account-card')?.closest('.wb-data-host')?.dataset.wbRefreshing==='true');
    const updating=await page.locator('#overview-account-card').evaluate(node=>{const panel=node.closest('.panel'),host=node.closest('.wb-data-host'),state=host.querySelector('.wb-state'),rootStyle=getComputedStyle(host,'::after');return {height:panel.getBoundingClientRect().height,text:!!host.querySelector('.wb-refresh-text'),emptyBackdrop:rootStyle.content==='none',spinner:getComputedStyle(state,'::before').display,heading:panel.querySelector('h3').textContent};});check(Math.abs(height-updating.height)<1&&updating.text&&updating.emptyBackdrop&&updating.spinner==='none','retained state / size '+JSON.stringify({height,updating}));
    await page.waitForFunction(()=>document.querySelector('#overview-account-card')?.closest('.wb-data-host')?.dataset.wbRefreshing==='false');
    check(await page.evaluate(()=>{const range=document.querySelector('#source-window-global'),theme=document.querySelector('#dark-toggle'),drag=document.querySelector('.layout-edit-control');return drag.nextElementSibling===range&&range.nextElementSibling===theme&&theme.querySelector('svg')&&document.querySelector('#observation-settings svg');}),'header order and SVG actions');
    await page.locator('#overview-token-stack').scrollIntoViewIfNeeded();await page.waitForTimeout(150);check(await page.locator('#overview-token-stack svg').count()>0,'cold model tokens chart');
    phase='missing';await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await page.locator('#overview-account-card').getByText('尚未取得帳戶用量資料',{exact:true}).waitFor();
    phase='ready';await page.evaluate(()=>document.dispatchEvent(new Event('visibilitychange')));await page.locator('#overview-account-card').getByText('pro',{exact:true}).waitFor();
    await page.getByRole('tab',{name:'監測程式',exact:true}).click();await page.waitForTimeout(150);const layouts=[];
    for(const width of [1600,820,390]){
      await page.setViewportSize({width,height:1000});await page.waitForTimeout(150);
      layouts.push(await page.evaluate(()=>{
        const view=document.querySelector('#view-monitor'),grids=[...view.querySelectorAll('.wb-column-layout')].filter(grid=>grid.getBoundingClientRect().width),cards=grids.flatMap(grid=>[...grid.querySelectorAll(':scope>.panel')].filter(card=>card.getBoundingClientRect().height&&card.getBoundingClientRect().width));let overlaps=0;
        for(let i=0;i<cards.length;i++)for(let j=i+1;j<cards.length;j++){const a=cards[i].getBoundingClientRect(),b=cards[j].getBoundingClientRect();if(a.left<b.right-.5&&b.left<a.right-.5&&a.top<b.bottom-.5&&b.top<a.bottom-.5)overlaps++;}
        const footer=view.querySelector(':scope>.source-reference'),box=footer.getBoundingClientRect();return {width:innerWidth,grids:grids.length,cards:cards.length,overlaps,overflow:document.documentElement.scrollWidth>innerWidth,footerFull:Math.abs(box.width-view.clientWidth)<3,footerBottom:cards.every(card=>card.getBoundingClientRect().bottom<=box.top+1),borders:cards.every(card=>parseFloat(getComputedStyle(card).borderTopWidth)>0)};
      }));
    }
    check(layouts.every(value=>value.grids>0&&value.cards>0&&!value.overlaps&&!value.overflow&&value.footerFull&&value.footerBottom&&value.borders),'monitor layout / footer '+JSON.stringify(layouts));check(await page.evaluate(()=>document.documentElement.dataset.theme)==='workbench','default theme name');check(!errors.length,'browser errors: '+errors.join(','));
    return {passed:true,account,requests,updating,layouts,errors};
  }finally{await page.unrouteAll({behavior:'wait'});page.off('pageerror',onError);}
}
