async page=>{
  const errors=[],onError=error=>errors.push(error.message);page.on('pageerror',onError);
  const check=(value,label)=>{if(!value)throw Error(label);};
  await page.reload();await page.waitForFunction(()=>data?.codex?.threads?.length);
  const samples=[];
  for(const [width,height] of [[1366,900],[820,900],[390,900],[320,900],[768,1366],[900,1600],[1080,1920]])for(const font of [14,18]){
    await page.setViewportSize({width,height});
    await page.evaluate(value=>{appearance.font=value;applyAppearance();},font);
    const result=await page.evaluate(async()=>{
      if(!data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');
      locale='zh-TW';applyLanguage();overviewHidden.clear();applyOverview();render(data);const samples=[],wait=()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(()=>setTimeout(resolve,0))));
      for(const main of document.querySelectorAll('.tabs>button[data-tab][role=tab]')){
        main.click();await wait();const view=document.querySelector(main.getAttribute('aria-controls')?'#'+main.getAttribute('aria-controls'):'main');
        const tabs=[...view.querySelectorAll('button[role=tab]')].filter(tab=>tab.getBoundingClientRect().width);
        for(const sub of [null,...tabs]){
          sub?.click();renderVisibleContent();attachTables();await wait();
          if(readConfiguration(configuration()).preferences.tableSchema!==10)throw Error('Configuration import for '+main.textContent+' / '+sub?.textContent);
          const tables=[...view.querySelectorAll('table')].filter(table=>table.getBoundingClientRect().width&&table.getBoundingClientRect().height),faults=[];
          for(const table of tables){const wrap=table.closest('.table-wrap');if(!wrap)faults.push('missing table wrapper');const body=table.tBodies[0],model=tableViews.get(body?.id);if(model&&model.rows.some(row=>rowCells(row).length!==model.headers.length))faults.push('column count: '+body.id);}
          const panels=[...view.querySelectorAll('.panel')].filter(panel=>panel.getBoundingClientRect().width&&panel.getBoundingClientRect().height&&!panel.parentElement.closest('.panel'));
          for(let i=0;i<panels.length;i++)for(let j=i+1;j<panels.length;j++){const a=panels[i].getBoundingClientRect(),b=panels[j].getBoundingClientRect();if(a.left<b.right-.5&&b.left<a.right-.5&&a.top<b.bottom-.5&&b.top<a.bottom-.5)faults.push('overlapping cards: '+panels[i].querySelector('h3')?.textContent+' / '+panels[j].querySelector('h3')?.textContent+' '+JSON.stringify({a:{top:a.top,bottom:a.bottom,left:a.left,right:a.right},b:{top:b.top,bottom:b.bottom,left:b.left,right:b.right},style:{height:getComputedStyle(panels[i]).height,transform:getComputedStyle(panels[i]).transform,row:panels[i].style.gridRow,root:panels[i].parentElement.style.gridTemplateRows,animations:panels[i].getAnimations().length}}));}
          samples.push({main:main.textContent,sub:sub?.textContent||'',width:innerWidth,tables:tables.length,panels:panels.length,overflow:document.documentElement.scrollWidth-innerWidth,faults});
        }
      }
      const config=readConfiguration(configuration());return {samples,schema:config.preferences.tableSchema};
    });samples.push(...result.samples);check(result.schema===10,'current configuration import');
  }
  check(samples.length>=32,'all tabs exercised');const faults=samples.filter(sample=>sample.overflow>1||sample.faults.length);check(!faults.length,'tab layout '+JSON.stringify(faults));check(!errors.length,'browser errors '+JSON.stringify(errors));
  page.off('pageerror',onError);return {checks:samples.length,tabs:[...new Set(samples.map(sample=>sample.main+' / '+sample.sub))],errors,faults};
}
