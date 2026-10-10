async page=>{
  const check=(value,message)=>{if(!value)throw Error(message);};
  check(await page.evaluate(()=>data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-'))),'isolated synthetic data required');
  const fixture=await page.evaluate(()=>{switchTab('overview');const source='error-category-chart',copy=[...overviewCopies].find(([,id])=>id===source)?.[0];if(!copy)throw Error('Missing category copy');overviewHidden.delete(copy);applyOverview();renderCharts();return {source,copy,events:data.errors.events.length};});
  await page.locator('#'+fixture.copy).evaluate(node=>node.closest('.panel').scrollIntoView({block:'center'}));
  await page.waitForFunction(id=>!chartJobs.has(id)&&$(id).childElementCount>0,fixture.copy);
  const category=await page.evaluate(({source,copy})=>({sourceHidden:!$(source).getBoundingClientRect().width,recipe:chartViews.get(source).renderData.kind,copyChildren:$(copy).childElementCount,state:panelDataStates.get($(copy).closest('.panel')).view.element.dataset.state}),fixture);
  check(fixture.events>0&&category.sourceHidden&&category.recipe==='category'&&category.copyChildren>0&&category.state==='ready','hidden source retains category recipe for visible overview copy');
  const severity=await page.evaluate(()=>({key:chartViews.get('error-level-chart').renderData.args[1],known:data.errors.events.filter(event=>event.severity).length}));check(severity.key==='severity'&&severity.known===fixture.events,'error level chart uses the severity field actually provided by the API');
  const thread=await page.evaluate(()=>{const source='skill-thread-chart',copy=[...overviewCopies].find(([,id])=>id===source)?.[0];if(!copy)throw Error('Missing thread copy');const panel=$(source).closest('.panel');window.chartCopyPanel={panel,hidden:panel.hidden};panel.hidden=true;chartViews.get(source).renderData=null;activeThreadsTimeline(source,'skill-thread-note',[{thread_id:'synthetic-chart-thread',timestamp:new Date().toISOString()}]);overviewHidden.delete(copy);applyOverview();renderOverviewCopies();return {source,copy};});
  await page.locator('#'+thread.copy).evaluate(node=>node.closest('.panel').scrollIntoView({block:'center'}));await page.waitForFunction(id=>!chartJobs.has(id)&&$(id).childElementCount>0,thread.copy);
  check(await page.evaluate(source=>chartViews.get(source).renderData.kind==='threads',thread.source),'hidden thread source retains its recipe');
  await page.evaluate(({source})=>{categoryTimeline(source,'error-category-note',[],'category',errorCategories);renderOverviewCopies();},fixture);
  await page.locator('#'+fixture.copy).evaluate(node=>node.closest('.panel').scrollIntoView({block:'center'}));await page.waitForFunction(id=>$(id+'-note').classList.contains('chart-empty'),fixture.copy);
  const empty=await page.evaluate(id=>$(id+'-note').textContent,fixture.copy);check(Boolean(empty),'actual empty data has a visible explanation');
  await page.evaluate(()=>{chartCopyPanel.panel.hidden=chartCopyPanel.hidden;delete window.chartCopyPanel;renderCharts();});
  return {category,severity,threadCopy:true,empty,syntheticEvents:fixture.events};
}
