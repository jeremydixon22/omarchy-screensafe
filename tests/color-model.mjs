import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const m=vm.createContext({});
vm.runInContext(fs.readFileSync(new URL('../ColorModel.js', import.meta.url),'utf8'),m);
const paletteSource=fs.readFileSync(new URL('../renderer/src/palette.rs',import.meta.url),'utf8');
const swatches=[...paletteSource.matchAll(/\("([a-z_]+)", "(#[0-9a-f]{6})"\)/g)].map((c,i)=>({value:i+1,name:c[1],color:c[2]}));
assert.equal(swatches.length,27);
assert.equal(m.palette(swatches).length,27);
for (const v of [-1,1.5,28,16777215,33554432,NaN,Infinity,'1',true,null]) assert.equal(m.valid(v),false);
for (const v of [0,1,27,16777216,33554431]) assert.equal(m.valid(v),true);
assert.equal(m.palette(swatches.slice(1)).length,0);
assert.equal(m.palette(swatches.map((c,i)=>i===0?{...c,color:'url(https://example.invalid)'}:c)).length,0);
assert.equal(m.hex(0,swatches),'#7aa2f7');
for (const hex of ['#000000','#ffffff','#0000ff','#ffff00','#ff0000','#4678ad','#010101']) {
 const hsv=m.hsv(hex),code=m.fromHsv(hsv.h,hsv.s,hsv.v);
 assert.equal(m.hex(code,swatches),hex);
}
for(let i=0;i<1000;i++) {
 const code=m.fromHsv(i/997,(i%19)/18,(i%23)/22);
 assert.equal(m.valid(code),true);
 const h=m.hsv(m.hex(code,swatches));
 assert.equal(m.fromHsv(h.h,h.s,h.v),code);
 assert.equal(m.valid(m.adjust(code,'h',1,swatches)),true);
}
assert.equal(m.step(0,-1,false,swatches),0);
assert.ok(m.step(27,1,false,swatches)>=m.customBase);
assert.equal(m.step(m.customBase,1,true,swatches),0);
console.log('Color encodings, palette validation, custom HSV and keyboard stepping passed.');
