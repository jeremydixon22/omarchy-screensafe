// Evaluate the actual QML timer binding: every listed short process must keep
// its deadline active by itself, including the previously omitted cache status.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
const source = fs.readFileSync(new URL('../Service.qml', import.meta.url), 'utf8');
const names = source.match(/readonly property var shortProcs:\s*\[([^\]]+)\]/)[1].split(',').map(s => s.trim());
const timer = source.slice(source.indexOf('id: deadline'));
const binding = timer.slice(timer.indexOf('running:')+8, timer.indexOf('onTriggered:')).trim();
function running(selected) {
  const processes = Object.fromEntries(names.map(name => [name, {running: name === selected}]));
  return vm.runInNewContext(binding, {...processes, root: {shortProcs: Object.values(processes)}}, {timeout: 100});
}
assert.ok(names.includes('cacheStatusProc'));
assert.equal(running(null), false);
for (const name of names) assert.equal(running(name), true, `Missing deadline for ${name}`);
console.log('Every short process independently activates the real service deadline binding.');
