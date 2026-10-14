async page=>{
 const check=(value,message)=>{if(!value)throw Error(message);};
 await page.evaluate(()=>{stopEventStream();switchTab('monitor');const root=node('section',null,'panel'),trigger=button('Tooltip semantics'),next=button('Next control'),description=node('span','Existing description');description.id='tooltip-original';trigger.setAttribute('aria-describedby',description.id);root.append(description,trigger,next);$('view-monitor').prepend(root);bindHelp(trigger,'Pure text <button> with no interactive controls');window.tooltipFixture={root,trigger,next,tip:helpBindings.get(trigger).controller.element};});
 const trigger=page.getByRole('button',{name:'Tooltip semantics',exact:true});
 try{
  await trigger.focus();await page.waitForFunction(()=>!tooltipFixture.tip.hidden);
  check(await page.evaluate(()=>document.activeElement===tooltipFixture.trigger&&tooltipFixture.tip.matches(':popover-open')&&!tooltipFixture.tip.children.length&&tooltipFixture.trigger.getAttribute('aria-describedby').includes('tooltip-original')),'Tooltip text, original description and focus');
  await page.keyboard.press('Escape');check(await trigger.getAttribute('aria-describedby')==='tooltip-original'&&await page.evaluate(()=>tooltipFixture.tip.hidden),'Escape restores original description');
  await page.getByRole('button',{name:'Next control',exact:true}).focus();await trigger.hover();await page.waitForFunction(()=>!tooltipFixture.tip.hidden);
  await page.locator('[role=tooltip]:visible').hover();await page.waitForTimeout(120);check(await page.evaluate(()=>!tooltipFixture.tip.hidden),'Hover transfers to tooltip');
  await trigger.click();const rect=await trigger.boundingBox();await page.mouse.move(rect.x+5,rect.y+5);await page.waitForTimeout(150);check(await page.evaluate(()=>tooltipFixture.tip.hidden),'Click with stationary pointer stays dismissed');
  await page.evaluate(()=>{openSettings();const target=button('Modal hint trigger');$('settings-dialog').append(target);bindHelp(target,'Modal tooltip explanation');tooltipFixture.modalTarget=target;tooltipFixture.modalTip=helpBindings.get(target).controller.element;});
  await page.getByRole('button',{name:'Modal hint trigger',exact:true}).focus();await page.waitForFunction(()=>!tooltipFixture.modalTip.hidden);
  await page.keyboard.press('Escape');check(await page.locator('#settings-dialog').isVisible()&&await page.evaluate(()=>tooltipFixture.modalTip.hidden),'Tooltip Escape preserves dialog');
  await page.keyboard.press('Escape');check(!await page.locator('#settings-dialog').isVisible(),'Next Escape closes dialog');
  await page.evaluate(()=>openSettings());check(await page.locator('[role=tooltip]:visible').count()===0,'Reopening dialog has no old tooltip');await page.evaluate(()=>$('settings-dialog').close());
  await page.evaluate(()=>switchTab('sqlite'));check(await page.locator('[role=tooltip]:visible').count()===0,'Changing page has no old tooltip');
  await page.evaluate(()=>{openDetail({kind:'thread',thread:data.codex.threads[0]});});
  const tabs=page.locator('#detail-dialog>.modal-tabs button:not(:disabled)'),count=await tabs.count();
  for(let index=0;index<Math.min(count,6);index++){await tabs.nth(index).hover();await page.waitForTimeout(30);check(await page.locator('[role=tooltip]:visible').count()<=1,'Fast hover has one tooltip');await tabs.nth(index).click();check(await page.locator('[role=tooltip]:visible').count()===0,'Tab click clears old and pending tooltip');}
  return {sharedTooltip:true,textOnly:true,focusRetained:true,descriptionRestored:true,hoverTransfer:true,clickSuppression:true,dialogLifecycle:true,pageLifecycle:true,singleTooltip:true,rapidTabs:true};
 }finally{await page.evaluate(()=>{hideHelp();tooltipFixture.modalTarget?.remove();tooltipFixture.root.remove();syncHelp();delete window.tooltipFixture;for(const dialog of document.querySelectorAll('dialog[open]'))dialog.close();});}
}
