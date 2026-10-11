async page=>{
  const check=(ok,message)=>{if(!ok)throw Error(message);},requests=[];
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'synthetic fixture required');
  await page.evaluate(async()=>{stopEventStream();await changeSettings({observations:{public_openai:false,public_codex:false}});switchTab('monitor');});
  const palettes=await page.evaluate(()=>{
    const host=node('section',null,'panel'),chart=node('div');chart.id='synthetic-palette';host.append(chart);$('view-monitor').append(host);
    const results=[],luminance=color=>{const channels=color.match(/\d+/g).slice(0,3).map(Number).map(v=>{v/=255;return v<=.04045?v/12.92:((v+.055)/1.055)**2.4;});return channels.reduce((sum,v,i)=>sum+v*[.2126,.7152,.0722][i],0);};
    for(const theme of ['sand','workbench','neutral'])for(const mode of ['light','dark']){
      Object.assign(appearance,{theme,mode,accent:'theme'});applyAppearance();chartDrawing=true;try{bars(chart.id,{Local:666,Cloud:85,Unknown:46,Remote:16});}finally{chartDrawing=false;}
      const colors=[...chart.querySelectorAll('.bar-fill')].map(el=>getComputedStyle(el).fill),background=getComputedStyle(host).backgroundColor,light=luminance(background),contrast=colors.map(color=>{const value=luminance(color);return (Math.max(value,light)+.05)/(Math.min(value,light)+.05);});
      results.push({theme,mode,colors,contrast,accent:$('accent-color').value,hex:$('accent-hex').value,labels:[...chart.querySelectorAll('.bar-label')].map(el=>el.textContent)});
    }
    host.remove();return results;
  });
  for(const item of palettes)check(new Set(item.colors).size===4&&item.contrast.every(value=>value>=3)&&item.accent===item.hex,'theme palette '+JSON.stringify(item));
  check(new Set(palettes.map(item=>item.accent)).size===6,'theme-specific accents');
  await page.evaluate(()=>{openSettings();});await page.locator('#settings-dialog').getByRole('tab',{name:'API 查詢',exact:true}).click();
  const layouts=[];
  check(await page.locator('#observation-api input[name=codex_account]').count()===1,'account API setting exists');
  check(await page.locator('#observation-api .api-setting-row').count()===7,'seven source settings');
  for(const lang of ['zh-TW','en','ja'])for(const width of [1366,390,320]){
    await page.setViewportSize({width,height:900});await page.evaluate(lang=>{locale=lang;applyLanguage();appearance.font=24;applyAppearance();},lang);
    const overflow=await page.locator('#observation-api').evaluate(el=>el.scrollWidth>el.clientWidth+1);check(!overflow,'API layout '+lang+' '+width);layouts.push({lang,width});
  }
  await page.locator('#settings-close').click();
  await page.evaluate(()=>{stopEventStream();locale='zh-TW';applyLanguage();appearance.font=15;appearance.theme='sand';appearance.mode='dark';applyAppearance();switchTab('monitor');});
  const modalTables=await page.evaluate(()=>{
    const rows=Array.from({length:31},(_,index)=>{const row=node('tr');cell(row,index);return row;});
    openDetail({kind:'recorded-values',heading:'Codex 版本',values:rows.map((_,index)=>'0.160.'+index)});
    const single=!$('detail-content').querySelector('.table-disclosure');
    const content=$('detail-content');clearPayloadObservers(content);content.replaceChildren();
    const one=node('section'),two=node('section');one.setAttribute('role','tabpanel');two.setAttribute('role','tabpanel');two.hidden=true;
    for(const section of [one,two])section.append(table(['Version'],rows.map(row=>row.cloneNode(true)),'First','detail:fold:'+String(section===one)+':first'),table(['Version'],rows.map(row=>row.cloneNode(true)),'Last','detail:fold:'+String(section===one)+':last'));
    content.append(one,two);attachTables(false,content);const state=[one,two].map(section=>[...section.querySelectorAll('table')].map(el=>!!el.closest('.table-disclosure')));$('detail-close').click();return {single,state};
  });check(modalTables.single&&modalTables.state.every(items=>JSON.stringify(items)==='[true,false]'),'modal last table remains open '+JSON.stringify(modalTables));
  await page.locator('#monitor-codex').screenshot({path:'.local/codex-card.png'});
  return {palettes,layouts,requests,modalTables};
}
