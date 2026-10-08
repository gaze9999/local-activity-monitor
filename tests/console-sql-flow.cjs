async page=>{
  await page.reload();await page.waitForFunction(()=>data?.updated_at&&!busy);
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  if(!await page.evaluate(()=>data.codex.threads.every(t=>t.model?.startsWith('demo-model-'))))throw Error('Synthetic fixture required');
  await page.route('**/api/codex/sql?**',route=>route.fulfill({contentType:'application/json',body:JSON.stringify({sql:'SELECT 12; SELECT 34',response:{stdout:'\u001b[31mSQL_RESULT\u001b[0m\n'+'line\n'.repeat(150),stderr:'',exit_code:0},response_scope:'containing_tool_call',truncated:false,content_status:'available'})}));
  await page.evaluate(()=>openDetail({kind:'sqlite',event:{id:'f'.repeat(64),statement:'SELECT',operation:'read',recognition:'nested_code',tool:'exec',timestamp:data.updated_at}}));
  await page.waitForFunction(()=>detail.sqlLoaded&&document.querySelector('.wb-console-color-31')?.textContent==='SQL_RESULT');
  const parsed=await page.evaluate(()=>{const output=[...document.querySelectorAll('#detail-content .wb-output')].at(-1),body=output.querySelector('.wb-output-body'),copy=output.querySelector('.wb-code-toolbar');body.scrollTop=200;const a=body.getBoundingClientRect(),b=copy.getBoundingClientRect();return {scope:document.getElementById('detail-dialog').textContent.includes('此回覆來自整次工具呼叫'),noOverlap:b.bottom<=a.top,scrollbar:getComputedStyle(body).scrollbarColor,code:output.textContent.includes('exit_code'),hasColor:!!output.querySelector('.wb-console-color-31')};});
  if(!parsed.scope||!parsed.noOverlap||!parsed.code||!parsed.hasColor||parsed.scrollbar==="auto")throw Error(JSON.stringify(parsed));
  await page.locator('#detail-content .wb-output').last().locator('.wb-output-modes>button').nth(1).click();
  const raw=await page.locator('#detail-content .wb-output').last().locator('.wb-output-content').textContent();if(!raw.includes('\\u001b')||!raw.includes('SQL_RESULT'))throw Error('Original content lost');
  await page.locator('#detail-dialog').evaluate(el=>el.close());await page.unroute('**/api/codex/sql?**');
  if(errors.length)throw Error(JSON.stringify(errors));return {parsed,rawPreserved:true,errors};
}
