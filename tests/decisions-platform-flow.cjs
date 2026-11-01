async page=>{
const check=(value,message)=>{if(!value)throw Error(message);};
await page.waitForFunction(()=>typeof data!=='undefined'&&data?.updated_at);
await page.evaluate(()=>switchTab('usage'));
check(await page.locator('#platform-decisions .api-query-actions button').isDisabled(),'no key disables query');
check(await page.locator('#platform-mode').innerText().then(t=>t.includes('無 key')),'no key mode');
let health='ok', calls=0;
await page.route('**/api/openai-platform?*',async route=>{calls++;const source=new URL(route.request().url()).searchParams.get('source');await route.fulfill({status:200,json:{source,health,checked_at:new Date().toISOString(),has_more:false,items:health==='ok'?[source==='costs'?{start_time:'2026-11-01T00:00:00Z',amounts:{usd:'0.3'}}:{start_time:'2026-11-01T00:00:00Z',input_tokens:352,input_cached_tokens:null,output_tokens:0,num_model_requests:2}]:[]}});});
await page.evaluate(()=>{data.platform_credentials={mode:'with_key',available:{decisions:true,costs:true}};renderUsage();});
check(!await page.locator('#platform-decisions .api-query-actions button').isDisabled(),'with key enables query');
for(const source of ['decisions','costs'])await page.locator('#platform-'+source+' .api-query-actions button').click();
await page.waitForFunction(()=>data.api_queries.platform_costs?.health==='ok');
check(await page.locator('#platform-decisions tbody').getByText('0',{exact:true}).count()===1,'zero preserved');
check(await page.locator('#platform-decisions tbody').getByText('--',{exact:true}).count()===1,'unknown preserved');
check((await page.locator('#platform-costs').innerText()).includes('0.3 USD'),'decimal cost currency');
health='unauthorized';
await page.locator('#platform-decisions .api-query-actions button').click();
await page.waitForFunction(()=>data.api_queries.platform_decisions?.health==='unauthorized');
check((await page.locator('#platform-decisions').innerText()).includes('金鑰權限不足'),'auth failure shown');
health='rate_limited';
await page.locator('#platform-decisions .api-query-actions button').click();
await page.waitForFunction(()=>data.api_queries.platform_decisions?.health==='rate_limited');
check((await page.locator('#platform-decisions').innerText()).includes('已達查詢速率限制'),'rate limit shown');
health='ok';
await page.locator('#platform-decisions .api-query-actions button').click();
await page.waitForFunction(()=>data.api_queries.platform_decisions?.health==='ok');
await page.unroute('**/api/openai-platform?*');
return {calls,noKey:true,withKey:true,zeroAndUnknown:true,costCurrency:true,authFailure:true,rateLimit:true,recovery:true};
}