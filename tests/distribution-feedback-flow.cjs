async page => {
  const origin=new URL(page.url()).origin,errors=[],onError=error=>errors.push(error.message);page.on('pageerror',onError);
  const check=(value,message)=>{if(!value)throw Error(message);};
  try{
    await page.goto(origin);await page.waitForFunction(()=>data?.codex?.threads?.length>0);
    check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic fixture required');
    const before=await page.evaluate(()=>{
      clearInterval(refreshTimer);resetChartQueue();switchTab('overview');const UI=WorkbenchUI,root=node('section',null,'panel');root.id='distribution-fixture';root.style.width='480px';root.style.maxWidth='100%';
      const clickable=node('div',null,'bar-chart'),plain=node('div',null,'bar-chart');clickable.id='fixture-clickable';plain.id='fixture-plain';root.append(clickable,plain);document.querySelector('#view-overview').prepend(root);
      chartDrawing=true;try{bars(clickable.id,{Synthetic:5},null,()=>navigateToTab('tools'));bars(plain.id,{Synthetic:5});}finally{chartDrawing=false;}
      const svg=svgNode('svg',{viewBox:'0 0 400 160',class:'chart'}),text=svgNode('text',{x:25,y:40,'text-anchor':'start'});text.textContent='Synthetic label';svg.append(text,svgNode('polyline',{points:'25,120 160,90 350,55',fill:'none',stroke:'var(--accent)','stroke-width':2}));const graph=node('div'),metric=node('div');graph.append(svg);metric.append(node('b','Synthetic statistic'));root.append(graph,metric);
      const graphState=UI.createDataState(graph,{kind:'chart'}),metricState=UI.createDataState(metric,{kind:'metric'});graphState.setState('ready','',true);metricState.setState('ready','',true);const box=root.getBoundingClientRect(),font=getComputedStyle(text).fontSize;graphState.setState('refreshing','',true);metricState.setState('refreshing','',true);
      window.distributionFixture={root,graphState,metricState,text,svg};const labelStyle=target=>{const style=getComputedStyle(target);return {font:style.fontSize,align:style.textAlign,padding:style.padding,height:target.getBoundingClientRect().height};},light=svg.querySelector('.wb-refresh-highlight'),mirror=svg.querySelector('mask text');
      return {clickable:labelStyle(clickable.querySelector('.bar-label')),plain:labelStyle(plain.querySelector('.bar-label')),sizeStable:Math.abs(root.getBoundingClientRect().height-box.height)<1,fontStable:getComputedStyle(mirror).fontSize===font,lightFill:getComputedStyle(light).fill,phase:Math.abs(parseFloat(light.style.animationDelay)-parseFloat(metric.querySelector('.wb-refresh-text').style.animationDelay))<20,background:getComputedStyle(graph,'::after').content,textCount:svg.querySelectorAll(':scope>text').length,maskIds:svg.querySelector('mask').querySelectorAll('[id]').length};
    });
    check(before.clickable.font===before.plain.font&&before.clickable.align===before.plain.align&&before.clickable.padding===before.plain.padding&&Math.abs(before.clickable.height-before.plain.height)<1,'clickable / read-only distribution labels '+JSON.stringify(before));
    check(before.sizeStable&&before.fontStable&&before.lightFill.startsWith('url(')&&before.phase&&before.background==='none'&&before.textCount===1&&!before.maskIds,'shimmer geometry / paint / phase '+JSON.stringify(before));
    await page.locator('#distribution-fixture').screenshot({path:'.local/distribution-feedback.png'});
    await page.locator('#fixture-clickable .bar-label').hover();check(await page.locator('#chart-tooltip').isVisible(),'hover chart tooltip');await page.locator('#fixture-clickable .bar-label').click();check(await page.locator('#view-tools').isVisible()&&!await page.locator('#chart-tooltip').isVisible(),'navigation immediately dismisses tooltip');
    const restored=await page.evaluate(()=>{const f=distributionFixture;f.graphState.setState('ready','',true);f.metricState.setState('ready','',true);return !f.root.querySelector('.wb-refresh-highlight,.wb-refresh-text,mask')&&f.text.parentElement===f.svg;});check(restored,'refresh removes owned effects and preserves original graph nodes');
    check(!errors.length,'browser errors '+errors.join(','));return {passed:true,before,restored,errors};
  }finally{await page.evaluate(()=>{window.distributionFixture?.graphState.destroy();window.distributionFixture?.metricState.destroy();window.distributionFixture?.root.remove();delete window.distributionFixture;});page.off('pageerror',onError);}
}
