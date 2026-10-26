import test from 'node:test'
import assert from 'node:assert/strict'
import { encodeCode, decodeCode, addEntry, removeEntry, patchFor } from '../src/library.js'

const plan = { name: 'p', steps: [{ kind: 'rest', minutes: 5 }] }

test('codes round-trip and are told apart by prefix', () => {
  const c = encodeCode('plan', plan)
  assert.ok(c.startsWith('NEON1:'))
  assert.deepEqual(decodeCode(c), { kind: 'plan', value: plan })
  assert.equal(decodeCode(encodeCode('profile', { settings: { kami: true } })).kind, 'profile')
})

test('bad codes are rejected with a reason', () => {
  assert.throws(() => decodeCode('hello'), /not a NEON1/)
  assert.throws(() => decodeCode('NEON1:@@@'), /damaged/)
  assert.throws(() => decodeCode(encodeCode('plan', { steps: 3 })), /no steps/)
  assert.throws(() => decodeCode(encodeCode('profile', {})), /no settings/)
})

test('adding replaces by name, newest first; removing works', () => {
  const c = encodeCode('plan', plan)
  let list = addEntry([], 'a', c, 1)
  list = addEntry(list, 'b', c, 2)
  list = addEntry(list, 'a', c, 3)
  assert.deepEqual(list.map(e => e.name), ['a', 'b'])
  assert.equal(list[0].added, 3)
  assert.deepEqual(removeEntry(list, 'a').map(e => e.name), ['b'])
  assert.throws(() => addEntry([], ' ', c), /name/)
})

test('patchFor builds the control patch', () => {
  const [e] = addEntry([], 'x', encodeCode('plan', plan))
  assert.deepEqual(patchFor(e), { plan })
  const [f] = addEntry([], 'y', encodeCode('profile', { settings: { kami: true }, plan }))
  assert.deepEqual(patchFor(f), { settings: { kami: true }, plan })
})
