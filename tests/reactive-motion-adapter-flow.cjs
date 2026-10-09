async page=>{
  await page.setViewportSize({width:1366,height:900});
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  const result=await page.evaluate(async()=>{
    if(!data.codex.threads.every(thread=>thread.model?.startsWith('demo-model-')))throw Error('Synthetic fixture required');
    const check=(ok,message)=>{if(!ok)throw Error(message);},wait=ms=>new Promise(resolve=>setTimeout(resolve,ms)),savedMotion=appearance.reduceMotion;
    appearance.reduceMotion=false;WorkbenchUI.setMotion(document.documentElement,true);
    const grid=document.createElement('div');grid.id='motion-fixture-cards';grid.className='cards';grid.style.cssText='position:fixed;left:10px;top:60px;width:900px;z-index:1000;display:flex';document.body.append(grid);
    const key='motion-fixture-table',wrap=document.createElement('div');wrap.className='table-wrap';wrap.style.cssText='position:fixed;left:10px;top:300px;width:1000px;z-index:1000';const table=document.createElement('table'),head=table.createTHead().insertRow(),body=table.createTBody();table.style.width='100%';wrap.append(table);document.body.append(wrap);
    const headers=['工具','來源','狀態'];headers.forEach((label,index)=>{const cell=document.createElement('th');cell.textContent=label;cell.dataset.columnIndex=index;head.append(cell);});
    const makeRow=id=>{const row=document.createElement('tr');row.dataset.fixture=id;headers.forEach((label,index)=>{const cell=row.insertCell();cell.textContent=id+' '+label;cell.dataset.columnIndex=index;});return row;};
    const a=makeRow('a'),b=makeRow('b'),c=makeRow('c'),view={table,body,rows:[a,b],headerCells:[...head.children],headers,title:'synthetic'};
    tableViews.set(key,view);tableStates[key]={size:10,page:1,hidden:[],heatmap:false};body.append(a,b);
    try{
      const source=[['A','1','a'],['B','2','b'],['C','3','c']];preferences.summaries??={};preferences.summaries[grid.id]={count:2,order:[0,1]};cards(grid,source);const first=grid.children[0],second=grid.children[1];
      cards(grid,[['A translated','9','changed'],source[1],source[2]]);check(grid.children[0]===first&&grid.children[1]===second,'card value/label update preserves element identity');check(first.querySelector('.value').textContent==='9','card value updated in place');
      preferences.summaries[grid.id]={count:3,order:[0,2,1]};cards(grid,source);const inserted=grid.querySelector('[data-summary-key="2"]');check(inserted?.style.visibility==='hidden','adapter reserves new card space before reveal');await wait(420);check(inserted.style.visibility===''&&grid.children[2]===second,'adapter card reveal and stable key reorder');
      preferences.summaries[grid.id]={count:2,order:[0,1]};cards(grid,source);check(inserted.parentElement===grid,'outgoing card retains space during fade');await wait(620);check(!inserted.parentElement,'outgoing card is removed');
      applyTableColumns(key);tableStates[key].hidden=['來源'];applyTableColumns(key);check(head.children.length===3,'outgoing table column remains during fade');await wait(620);check(head.children.length===2&&a.children.length===2&&b.children.length===2,'header/body column removed together');
      tableStates[key].hidden=[];applyTableColumns(key);check(head.children.length===3&&head.children[1].style.visibility==='hidden','table column reinsertion reserves hidden space');await wait(620);check(a.children.length===3&&head.children[1].style.visibility==='','table column reinsertion completes');
      const headerRects=[...head.children].map(cell=>cell.getBoundingClientRect()),bodyRects=[...a.children].map(cell=>cell.getBoundingClientRect());check(headerRects.every((rect,index)=>Math.abs(rect.left-bodyRects[index].left)<2&&Math.abs(rect.width-bodyRects[index].width)<2),'native table final column alignment');
      updateTableChildren(body,[a,b]);updateTableChildren(body,[a,c,b]);check(c.style.visibility==='hidden'&&targetTableRows(body).length===3,'new row reserves space and target count excludes stale rows');await wait(420);check(targetTableRows(body)[1]===c&&c.parentElement===body,'row insertion completes');
      updateTableChildren(body,[a,b]);check(c.parentElement===body&&targetTableRows(body).length===2,'row deletion count changes before outgoing animation finishes');await wait(620);check(!c.parentElement,'row deletion completes');
      appearance.reduceMotion=true;cards(grid,source);check([...grid.children].every(card=>!card.getAnimations().length),'motion preference disables card animation');
      return {cardIdentity:true,cardInsertRemove:true,columnsInsertRemove:true,columnAlignment:true,rowInsertRemove:true,targetCounts:true,reducedMotion:true};
    }finally{
      for(const root of [grid,body,head,a,b,c])WorkbenchUI.cancelChildMotion(root);grid.remove();wrap.remove();tableViews.delete(key);delete tableStates[key];delete preferences.summaries[grid.id];summaryLibrary.delete(grid.id);appearance.reduceMotion=savedMotion;WorkbenchUI.setMotion(document.documentElement,!savedMotion);
    }
  });return result;
}
