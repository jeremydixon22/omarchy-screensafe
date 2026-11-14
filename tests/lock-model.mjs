// Exercise the real service lock decisions and its process-exit callback.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';

const source = fs.readFileSync(new URL('../Service.qml', import.meta.url), 'utf8');
const decision = source.slice(source.indexOf('  function lockResult('), source.indexOf('  function observePreparationOutput('));
const process = source.slice(source.indexOf('id: lockProc'), source.indexOf('id: photosFolderProc'));
const callback = process.match(/onExited:\s*(function\([^)]*\)\s*\{[^\n]+\})/)[1];

function result(text, code, purpose, over = false, fromIdle = false) {
  const launches = [];
  const context = {
    root: {stoppedForLock: false, fromIdle, previewPath: '/preview-pack', resolvedPack: () => '/idle-pack'},
    rendererProc: {running: true},
    lockOut: {text, over},
    lockProc: {purpose},
    lastLine: value => String(value || '').trim().split('\n').at(-1),
    launch: path => launches.push(path),
  };
  vm.createContext(context);
  vm.runInContext(decision, context, {timeout: 100});
  context.root.lockResult = context.lockResult;
  vm.runInContext(`(${callback})(${code})`, context, {timeout: 100});
  return {running: context.rendererProc.running, stopped: context.root.stoppedForLock, launches};
}

for (const [text, code, over] of [
  ['true', 0, false], ['', 0, false], ['unknown', 0, false],
  ['false', 1, false], ['false', 143, false], ['false', 0, true],
]) {
  const watched = result(text, code, 'watch', over);
  assert.equal(watched.running, false, `continued with ${JSON.stringify({text, code, over})}`);
  assert.equal(watched.stopped, true, 'unsafe lock state must suppress Preview reopening');
  assert.deepEqual(watched.launches, []);
  assert.deepEqual(result(text, code, 'start', over).launches, [], 'unsafe lock state must refuse startup');
}
const safe = result('false\n', 0, 'watch');
assert.equal(safe.running, true);
assert.equal(safe.stopped, false);
assert.deepEqual(result('false', 0, 'start').launches, ['/preview-pack']);
assert.deepEqual(result('false', 0, 'start', false, true).launches, ['/idle-pack']);
console.log('Lock checks fail closed on errors, unknown states and overflow; explicit unlock permits playback.');
