async page=>{
  const check=(ok,message)=>{if(!ok)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'synthetic fixture required');
  await page.evaluate(()=>{stopEventStream();openSettings();});
  const dialog=page.locator('#settings-dialog');await dialog.getByRole('tab',{name:'外觀',exact:true}).click();
  check(await page.locator('#theme-select option[value=neutral]').textContent()==='簡約素色','four-character theme');
  const accents=[];
  for(const preset of ['green','blue','orange']){
    await page.locator('#accent-select').selectOption(preset);
    const result=await page.evaluate(()=>({preset:appearance.accent,picker:$('accent-color').value,hex:$('accent-hex').value,color:getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()}));
    check(result.preset===preset&&result.hex.toLowerCase()===result.color.toLowerCase()&&result.picker===result.hex.toLowerCase(),'preset color controls synchronized');accents.push(result);
  }
  check(new Set(accents.map(item=>item.color)).size===3,'distinct preset colors');
  await page.locator('#accent-hex').fill('#a482e6');await page.locator('#accent-hex').press('Tab');
  check(await page.locator('#accent-select').inputValue()==='custom','editing hex selects custom');
  await page.locator('#accent-select').selectOption('green');await page.locator('#font-size').selectOption('20');await page.locator('#code-font-size').selectOption('18');
  await page.evaluate(()=>{appearance.fontFamily='Segoe UI';appearance.codeFontFamily='Consolas';applyAppearance();saveView();});
  check(await page.locator('#accent-select').inputValue()==='green','font controls retain accent selection');
  const fonts=await page.evaluate(()=>{const raw=codePayload('const example = 1;','javascript'),preview=document.createElement('p');preview.textContent='Interface example';$('settings-dialog').append(raw,preview);const result={code:parseFloat(getComputedStyle(raw).fontSize),ui:parseFloat(getComputedStyle(preview).fontSize),codeFamily:getComputedStyle(raw).fontFamily,uiFamily:getComputedStyle(preview).fontFamily};clearPayloadObservers(raw);raw.remove();preview.remove();return result;});
  check(fonts.code===18&&fonts.ui===20&&fonts.codeFamily.includes('Consolas')&&fonts.uiFamily.includes('Segoe UI'),'independent interface and code fonts');
  const settings=await page.evaluate(()=>readConfiguration(configuration()).preferences.appearance);check(settings.font===20&&settings.codeFont===18&&settings.codeFontFamily==='Consolas','font settings export');
  for(const patch of [{codeFont:11},{codeFont:25},{codeFont:18.5},{codeFontFamily:'url(https://invalid.example)'},{codeFontFamily:'Arial; color:red'}])check(await page.evaluate(patch=>{const value=structuredClone(configuration());Object.assign(value.preferences.appearance,patch);try{readConfiguration(value);return false;}catch{return true;}},patch),'invalid code settings rejected');
  await page.locator('#font-size').selectOption('12');await page.locator('#code-font-size').selectOption('12');
  check(await page.evaluate(()=>{const configurationValue=readConfiguration(configuration());return appearance.font===12&&appearance.codeFont===12&&getComputedStyle(document.documentElement).fontSize==='12px'&&configurationValue.preferences.appearance.codeFont===12;}),'12 px selections accepted');
  await page.locator('#font-size').selectOption('20');await page.locator('#code-font-size').selectOption('18');
  const layouts=[];
  for(const lang of ['zh-TW','en','ja'])for(const width of [1366,900,390,320]){
    await page.setViewportSize({width,height:900});await page.evaluate(lang=>{locale=lang;applyLanguage();appearance.font=24;applyAppearance();},lang);
    const result=await page.evaluate(()=>{const body=$('settings-dialog'),controls=[...body.querySelectorAll('#accent-custom-row input,#accent-select,#font-family,#code-font-family,#font-size,#code-font-size')].map(el=>el.getBoundingClientRect()),bad=controls.some(a=>a.left<body.getBoundingClientRect().left||a.right>body.getBoundingClientRect().right);return {overflow:body.scrollWidth>body.clientWidth+1||document.documentElement.scrollWidth>innerWidth+1,bad};});
    check(!result.overflow&&!result.bad,'appearance layout '+JSON.stringify({lang,width,result}));layouts.push({lang,width});
  }
  await dialog.screenshot({path:'.local/appearance-controls.png',animations:'disabled'});await page.locator('#settings-close').click();
  await page.evaluate(()=>{appearance.font=15;applyAppearance();saveView();});await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  check(await page.evaluate(()=>appearance.font===15&&appearance.codeFont===18&&appearance.codeFontFamily==='Consolas'),'font settings persist after reload');
  const minimum=[];
  for(const tab of ['overview','codex','projects','tools','mcp','files','errors','monitor']){
    await page.evaluate(tab=>switchTab(tab),tab);
    const result=await page.evaluate(()=>{const bad=[];for(const el of document.querySelectorAll('main *'))if(el.getClientRects().length&&[...el.childNodes].some(node=>node.nodeType===3&&node.textContent.trim())&&parseFloat(getComputedStyle(el).fontSize)<(el.closest('.card-updated,.card-library-meta,.custom-metric-card>p,.cards .card-note,.cards .card>.detail,.statistic-label,.trend-legend,.panel>p[id$="-note"]')?11.99:14.99))bad.push({tag:el.tagName,cls:el.className?.baseVal??el.className,text:el.textContent.slice(0,40),size:getComputedStyle(el).fontSize});return bad.slice(0,12);});
    check(!result.length,'minimum text '+tab+' '+JSON.stringify(result));minimum.push(tab);
  }
  check(await page.evaluate(()=>$('updated').parentElement.nextElementSibling===$('program-uptime')),'update time immediately before uptime');
  return {accents,fonts,layouts,minimum};
}
