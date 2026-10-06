import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import vm from "node:vm";

const app=readFileSync(new URL("../src/local_activity_monitor/web/app.js",import.meta.url),"utf8"),start=app.indexOf("function browserEnvironment()"),end=app.indexOf("function cliVersionList",start),source=app.slice(start,end);
function environment(navigator){return vm.runInNewContext(source+"browserEnvironment()",{navigator,innerWidth:1320,innerHeight:900,devicePixelRatio:1.25,Intl});}
assert.equal(environment({userAgentData:{brands:[{brand:"Not_A Brand",version:"99"},{brand:"Chromium",version:"154"},{brand:"Microsoft Edge",version:"154"}],platform:"Windows"},language:"zh-TW"}).browser,"Microsoft Edge 154");
assert.equal(environment({userAgent:"Chrome/154.0.0.0 Safari/537.36 Edg/154.0.4258.62"}).browser,"Microsoft Edge 154.0.4258.62");
assert.equal(environment({userAgent:"Mozilla/5.0 Firefox/149.0"}).browser,"Firefox 149.0");
assert.equal(environment({userAgent:"AppleWebKit/605.1.15 Version/26.1 Safari/605.1.15",platform:"MacIntel"}).browser,"Safari 26.1");
const missing=environment({});
assert.equal(missing.browser,null);
assert.equal(missing.language,null);
assert.equal(Object.hasOwn(missing,"platform"),false);
assert.equal(Object.hasOwn(missing,"viewport"),false);
assert.equal(Object.hasOwn(missing,"pixelRatio"),false);
assert.ok(missing.timeZone);
console.log("Browser client hints, UA fallback and omitted device fields passed");
