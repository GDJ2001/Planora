// Optional browser QA. npm install --no-save playwright, then npx playwright install chromium.
const {chromium}=require('playwright');
const fs=require('node:fs');
const path=require('node:path');
const {pathToFileURL}=require('node:url');
const assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..');
(async()=>{
 const browser=await chromium.launch({headless:true,...(process.env.PLANORA_BROWSER?{executablePath:process.env.PLANORA_BROWSER}:{})});
 try{
   const page=await browser.newPage({viewport:{width:1440,height:1100}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
   await page.goto(pathToFileURL(path.join(root,'assets/brand/showcase.html')).href);await page.waitForLoadState('load');
   assert.equal(await page.title(),'Planora — Brand foundation');
   assert(await page.locator('img').evaluateAll(images=>images.every(image=>image.complete&&image.naturalWidth>0)));
   fs.mkdirSync(path.join(root,'state'),{recursive:true});
   await page.screenshot({path:path.join(root,'state/showcase-dark.png'),fullPage:true});
   await page.locator('#theme').selectOption('light');assert.equal(await page.locator('html').getAttribute('data-theme'),'light');
   await page.screenshot({path:path.join(root,'state/showcase-light.png'),fullPage:true});
   await page.locator('#filter').fill('browser');assert.equal(await page.locator('[data-process]:visible').count(),1);
   await page.locator('#filter').fill('does-not-exist');assert(await page.locator('#no-results').isVisible());
   await page.locator('#filter').fill('');assert.equal(await page.locator('[data-process]:visible').count(),5);
   await page.locator('#theme').selectOption('system');await page.emulateMedia({colorScheme:'dark'});await page.waitForFunction(()=>document.documentElement.dataset.theme==='dark');
   await page.emulateMedia({colorScheme:'light',reducedMotion:'reduce'});await page.waitForFunction(()=>document.documentElement.dataset.theme==='light');
   assert.equal(await page.locator('#copy').evaluate(el=>getComputedStyle(el).transitionDuration),'0s');
   await page.locator('#copy').click();assert.match(await page.locator('#copy-status').innerText(),/#2563EB/i);
   await page.locator('#theme').focus();assert.equal(await page.locator('#theme').evaluate(el=>getComputedStyle(el).outlineStyle),'solid');
   await page.setViewportSize({width:390,height:844});await page.evaluate(()=>window.scrollTo(0,0));
   assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'Page overflows narrow viewport');
   await page.screenshot({path:path.join(root,'state/showcase-mobile.png'),fullPage:true});
   await page.setViewportSize({width:1440,height:1100});await page.locator('#theme').selectOption('dark');
   await page.locator('#preview').screenshot({path:path.join(root,'state/preview-dark.png')});
   await page.locator('.sizes').screenshot({path:path.join(root,'state/icon-sizes.png')});
   await page.locator('#theme').selectOption('light');await page.locator('#preview').screenshot({path:path.join(root,'state/preview-light.png')});
   assert.deepEqual(errors,[]);console.log('PASS: offline rendering, dark/light/system themes, process filter, empty state, copy feedback, keyboard focus, reduced motion, and 390px layout.');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
