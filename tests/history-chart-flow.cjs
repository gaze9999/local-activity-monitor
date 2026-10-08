async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  if(!await page.evaluate(()=>data.codex.threads.every(t=>t.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  const outcome=await page.evaluate(async()=>{
    const ids=['sqlite-time-chart','sqlite-breakdown-chart','mcp-time-chart','mcp-breakdown-chart'];
    const now=new Date().toISOString(),aggregate={scope:'retained_database',window:'all',total:2400,bucket_seconds:60,classification_series_truncated:false,series:[{time:now,calls:2400,direct:1600,nested:800,operations:{read:2400},servers:{future:{total:2400,direct:1600,nested:800}}}]};
    const oldSql=data.codex.sqlite,oldMcp=data.mcp,mcpSelection=mcpSource,oldSettings=new Map(ids.map(id=>[id,{...chartViews.get(id).settings}]));
    const copies=ids.map(id=>'overview-copy-'+id),hidden=new Set(overviewHidden),copySettings=new Map(copies.map(id=>[id,{...chartViews.get(id).settings}]));
    data.codex.sqlite={...oldSql,aggregate,events:[{timestamp:now,operation:'read'}]};data.mcp={...oldMcp,history_aggregate:aggregate,events:[{timestamp:now,server:'future',nested:false}]};mcpSource='all';
    for(const id of [...ids,...copies]){chartViews.get(id).settings.range='all';overviewHidden.delete(id);}
    const main={};
    for(const id of ids){switchTab(id.startsWith('sqlite')?'sqlite':'mcp');document.getElementById(id).scrollIntoView();renderCharts();for(let n=0;n<30&&document.getElementById(id).dataset.historyScope!=='retained_database';n++)await new Promise(resolve=>setTimeout(resolve,100));main[id]=document.getElementById(id).dataset.historyScope;}
    if(Object.values(main).some(scope=>scope!=='retained_database'))throw Error('Main chart scope: '+JSON.stringify(main));
    switchTab('overview');renderCharts();
    const wait=async scope=>{for(const id of copies){document.getElementById(id).scrollIntoView();renderCharts();for(let n=0;n<30&&document.getElementById(id).dataset.historyScope!==scope;n++)await new Promise(resolve=>setTimeout(resolve,100));}};await wait('retained_database');
    const full=Object.fromEntries(copies.map(id=>[id,document.getElementById(id).dataset.historyScope]));
    if(Object.values(full).some(scope=>scope!=='retained_database'))throw Error('Full copy scope: '+JSON.stringify(full));
    for(const id of copies){const s=chartViews.get(id).settings;s.range='custom';s.start=new Date(Date.now()-3600000).toISOString();s.end=new Date(Date.now()+1000).toISOString();}
    renderCharts();await wait('working_set');
    const custom=Object.fromEntries(copies.map(id=>[id,document.getElementById(id).dataset.historyScope]));if(Object.values(custom).some(scope=>scope!=='working_set'))throw Error('Custom copy scope: '+JSON.stringify(custom));
    if(ids.some(id=>chartViews.get(id).settings.range!=='all'))throw Error('Copy range changed source settings');
    data.codex.sqlite=oldSql;data.mcp=oldMcp;mcpSource=mcpSelection;for(const [id,settings]of [...oldSettings,...copySettings])Object.assign(chartViews.get(id).settings,settings);overviewHidden.clear();for(const id of hidden)overviewHidden.add(id);renderCharts();
    return {main,full,custom,independentSettings:true};
  });
  if(errors.length)throw Error(JSON.stringify(errors));return {...outcome,errors};
}
