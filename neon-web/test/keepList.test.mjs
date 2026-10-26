import test from 'node:test'
import assert from 'node:assert/strict'
import { parseIds, classify, summarize } from '../src/keepList.js'

test('parseIds accepts commas and spaces, drops junk and duplicates', () => {
  assert.deepEqual(parseIds('2000000, 2000001 x 2000000;-5 1.5'), [2000000, 2000001])
  assert.deepEqual(parseIds(''), [])
  assert.deepEqual(parseIds(null), [])
})

test('classify keeps top rows and listed ids wherever they sit', () => {
  const rows = classify([111, 222, 333, null], 1, [333])
  assert.deepEqual(rows.map(r => r.action), ['keep', 'sell', 'keep', 'sell'])
  assert.deepEqual(summarize(rows), { kept: 2, sold: 2 })
})
