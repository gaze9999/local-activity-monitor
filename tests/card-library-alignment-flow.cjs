async page=>{
 const result=[];
 await page.evaluate(()=>{stopEventStream();switchTab('errors');openTabSettings('errors');});
 for(const width of [1366,900,390,320]){
  await page.setViewportSize({width,height:900});const layout=await page.evaluate(()=>{const rows=[...$('tab-dialog').querySelectorAll('.card-library-row')].filter(row=>row.getClientRects().length&&!row.querySelector('input[type=text]'));return {count:rows.length,slots:rows.every(row=>row.cells.length===4),sources:rows.map(row=>row.querySelector('select')?.getBoundingClientRect().x).filter(value=>value!=null),handles:rows.map(row=>row.querySelector('.settings-drag-handle')?.getBoundingClientRect().x).filter(value=>value!=null),overflow:rows.some(row=>row.getBoundingClientRect().right>$('tab-dialog').getBoundingClientRect().right+1)};});
  if(!layout.count||!layout.slots||layout.overflow||width>=900&&(Math.max(...layout.sources)-Math.min(...layout.sources)>1||Math.max(...layout.handles)-Math.min(...layout.handles)>1))throw Error(JSON.stringify({width,...layout}));result.push({width,count:layout.count,aligned:true});
 }
 await page.setViewportSize({width:1366,height:900});await page.screenshot({path:'.local/card-library-alignment.png'});await page.locator('#tab-settings-close').click();return {layouts:result};
}
