import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import vm from "node:vm";

const app=readFileSync(new URL("../frontend/app.js",import.meta.url),"utf8"),start=app.indexOf("function projectUsage("),end=app.indexOf("function renderBilling",start);
const project=vm.runInNewContext(app.slice(start,end)+";projectUsage"),plain=value=>JSON.parse(JSON.stringify(value));
const first=project([{model:"model-a",tokens:{input_tokens:100,cached_input_tokens:25,output_tokens:40,total_tokens:140}},{model:"model-b",tokens:{total_tokens:5}}]);
assert.deepEqual(plain(first.counts),{"model-a":140,"model-b":5});
assert.deepEqual(plain(first.segments),{"model-a":[["一般輸入",75],["快取輸入",25],["輸出",40]],"model-b":[["未分類",5]]});
const missing=project([{model:"unknown"},{model:"zero",tokens:{input_tokens:0,cached_input_tokens:0,output_tokens:0,total_tokens:0}}]);
assert.deepEqual(plain(missing.counts),{zero:0});
assert.equal(missing.groups.get("unknown").known,0);
assert.equal(missing.groups.get("zero").known,1);
assert.deepEqual(plain(project([]).counts),{});
console.log("Usage projections are ready before page rendering; null and zero remain distinct");
