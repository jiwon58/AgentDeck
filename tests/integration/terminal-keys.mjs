import assert from 'node:assert/strict';
import { harnessKey } from '../../src/terminalKeys.ts';

const key = (value, modifiers = {}) => ({
  type: 'keydown', key: value, altKey: false, ctrlKey: false, shiftKey: false,
  metaKey: false, isComposing: false, getModifierState: () => false, ...modifiers,
});

for (const agent of ['gajae', 'claude', 'hermes', 'generic']) {
  for (const [direction, code] of [['Up', 'A'], ['Down', 'B'], ['Right', 'C'], ['Left', 'D']]) {
    assert.equal(harnessKey(key('Arrow' + direction, { altKey: true }), agent), '\x1b[1;3' + code);
  }
  assert.equal(harnessKey(key('t', { ctrlKey: true }), agent), undefined);
  assert.equal(harnessKey(key('Tab', { shiftKey: true }), agent), undefined);
  assert.equal(harnessKey(key('Enter'), agent), undefined);
}
assert.equal(harnessKey(key('Enter', { shiftKey: true }), 'gajae'), '\x1b[13;2u');
assert.equal(harnessKey(key('Enter', { ctrlKey: true }), 'gajae'), '\x1b[13;5u');
assert.equal(harnessKey(key('Enter', { shiftKey: true }), 'claude'), '\n');
assert.equal(harnessKey(key('Enter', { ctrlKey: true }), 'hermes'), '\n');
assert.equal(harnessKey(key('Enter', { shiftKey: true }), 'generic'), undefined);
assert.equal(harnessKey(key('ArrowUp', { altKey: true, isComposing: true }), 'gajae'), undefined);
assert.equal(harnessKey(key('ArrowUp', { altKey: true, getModifierState: () => true }), 'gajae'), undefined);
console.log('Harness key encoder passed.');
