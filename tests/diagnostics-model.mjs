// Execute the real service paths that retain a failure across fallback/reset.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const source = fs.readFileSync(new URL('../Service.qml', import.meta.url), 'utf8');
function fn(name) {
  const start = source.indexOf(`  function ${name}(`);
  const end = source.indexOf('\n  }', start)+4;
  return source.slice(start, end);
}
const logs = [], launches = [];
const context = {Date, JSON, diagnosticProc:{running:true}, rendererErr:{over:false}, root:{
  diagnosticQueue:[], diagnosticSerial:0, token:'s-test', diagnosticsState:'idle',
  recordDiagnostic:(...args)=>logs.push(args), failureReason:null,
  launching:'/videos', resolvedPack:()=>'/videos', defaultPack:'/omarchy',
  videosOf:()=>1, nameOf:()=> 'Videos', launch:path=>launches.push(path),
  fromIdle:true, rendererSession:'s-1', rendererStartedAt:1,
}};
vm.createContext(context);
vm.runInContext(fn('failureReason'), context);
context.root.failureReason = context.failureReason;
const cause = 'screensafe: video frame 17 failed: SSVv2 pixel stream ended early';
const errors = `${cause}\n{"ssv2":{"failures":0}}\nscreensafe-video: decoder channel disconnected before renderer exit\n`;
assert.equal(context.failureReason(errors), cause.replace('screensafe: ',''));
vm.runInContext(fn('rendererExited'), context);
context.rendererExited(3, errors);
assert.deepEqual(launches, ['/omarchy']);
assert.equal(logs[0][6], errors, 'capture stderr before fallback resets the process buffer');
assert.match(context.root.problem, /pixel stream ended early/);
for(const code of [0,15,130,143]) {
  launches.length=0; context.rendererExited(code,errors);
  assert.equal(launches.length,0,'cancellation must never trigger fallback');
}
launches.length=0;context.rendererExited(2, errors);
assert.equal(launches[0], '/omarchy','hardware/broker startup errors also fall back');
context.root.recordDiagnostic=null;
vm.runInContext(fn('recordDiagnostic'), context);
context.root.nextDiagnostic=()=>{};
for(let i=0;i<40;i++) context.recordDiagnostic('exit','fullscreen','/videos',`s-${i}`,Date.now(),3,errors,false,'video-hardware','idle');
assert.equal(context.root.diagnosticQueue.length,8);
assert.equal(JSON.parse(context.root.diagnosticQueue.at(-1)).session,'s-39');
const reader = source.slice(source.indexOf('  component BoundedOutput:'), source.indexOf('    id: diagnosticProc'));
const onRead = reader.match(/onRead: data => \{([\s\S]*?)\n    \}/)[1];
for(const retain of [false,true]) {
  const state={text:'original cause\n',over:false,limit:32,retainOnOverflow:retain,process:retain?null:{running:true}};
  vm.createContext(state);
  vm.runInContext(`(function(data){${onRead}})('x'.repeat(10000))`,state);
  assert.equal(state.over,true);
  if(retain) {assert.match(state.text,/^original cause/);assert.equal(state.text.length,32)}
  else {assert.equal(state.text,'');assert.equal(state.process.running,false)}
  const previous=state.text;
  vm.runInContext(`(function(data){${onRead}})('more')`,state);
  assert.equal(state.text,previous,'overflow never keeps accumulating output');
}
console.log('First failure survives fallback; cancellation stays quiet; diagnostic queue stays bounded.');
