import assert from 'node:assert/strict';
import fs from 'node:fs';
const source = fs.readFileSync(new URL('../Service.qml', import.meta.url), 'utf8');
const body = source.match(/  function launch\(packPath\) \{([\s\S]*?)\n  \}\n\n  function rendererExited/)[1];
const launch = new Function('root', 'rendererProc', 'rendererOut', 'rendererErr', 'packPath', body);
for (const [video, enabled, expected] of [[true,1,'video-player'],[true,0,'video-player'],[false,1,'prepared-player'],[false,0,'/renderer']]) {
  let hardware;
  const root = {
    renderer:'/renderer', themeDir:'/theme', branding:'/brand', logo:'/logo', badge:'/badge', photos:'/photos', videos:'/videos',
    overlayArgument:'', performanceValues:{enabled,videoHardware:1}, settingsArg:()=>'', videosOf:()=>video?1:-1,
    videoCommand:(args,useHardware)=>{hardware=useHardware;return ['video-player',...args]},
    cacheCommand:()=>['prepared-player'], cacheEnvironment:()=>({prepared:true}), environment:()=>({normal:true})
  };
  const proc = {};
  launch(root,proc,{reset(){}},{reset(){}},'/pack');
  assert.equal(proc.command[0],expected);
  assert.equal(proc.running,true);
  if(video) assert.equal(hardware,true);
  assert.equal(proc.environment.prepared===true,!video&&enabled===1);
}
assert.match(source,/\["catalog", "thumbnail", "check", "render"\].*root\.videoCommand\(args, false\)/);
assert.match(source,/livePreviewProc\.command = root\.videosOf\(path\).*root\.videoCommand/);
console.log('Video/prepared playback routing and preview/thumbnail policies passed.');
