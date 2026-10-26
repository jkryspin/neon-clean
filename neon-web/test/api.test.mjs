import test from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { createApi } from '../server/api.mjs'

// Call the handler without a real server: fake req/res objects.
function setup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'neon-api-'))
  fs.mkdirSync(path.join(root, 'neon-calib', 'maps'), { recursive: true })
  fs.writeFileSync(path.join(root, 'neon-calib', 'map-names.json'), JSON.stringify({ 10001030: { name: 'Forest West of Henesys', street: 'Victoria Road', region: 'VictoriaIsland' } }))
  fs.writeFileSync(path.join(root, 'neon-calib', 'mob-names.json'), JSON.stringify({ 2: 'Snail' }))
  fs.writeFileSync(path.join(root, 'neon-calib', 'maps', '10001030.json'), JSON.stringify({ footholds: [{}], ladders: [{}, {}], portals: [{ type: 2, to_map: 10001020 }, { type: 0, to_map: 9 }], mobs: { 2: 13 } }))
  const api = createApi(root)
  const call = (method, url, body) => new Promise(resolve => {
    const chunks = body === undefined ? [] : [Buffer.from(JSON.stringify(body))]
    const req = { method, url, on(ev, cb) { if (ev === 'data') chunks.forEach(c => cb(c)); if (ev === 'end') setImmediate(cb); return this }, destroy() {} }
    const res = { headers: {}, statusCode: 200, setHeader(k, v) { this.headers[k] = v }, end(b) { resolve({ status: this.statusCode, body: b ? (() => { try { return JSON.parse(b.toString()) } catch { return b.toString() } })() : null }) } }
    api(req, res, () => resolve({ status: 404, body: 'next' }))
  })
  return { root, call }
}

test('control: each POST bumps seq and merges settings instead of replacing them', async () => {
  const { root, call } = setup()
  let r = await call('POST', '/api/control?session=main', { paused: true, settings: { flee_hp_below: 55 } })
  assert.equal(r.body.seq, 1); assert.equal(r.body.paused, true)
  r = await call('POST', '/api/control?session=main', { settings: { mobs: [2, 3] } })
  assert.equal(r.body.seq, 2)
  assert.deepEqual(r.body.settings, { flee_hp_below: 55, mobs: [2, 3] }, 'earlier settings survive')
  assert.equal(r.body.paused, true, 'unrelated fields survive')
  assert.ok(fs.existsSync(path.join(root, 'neon-control.json')), 'main uses the unsuffixed file the loader watches')
  r = await call('POST', '/api/control?session=night', { stop: true })
  assert.ok(fs.existsSync(path.join(root, 'neon-control.night.json')))
  assert.equal((await call('GET', '/api/control?session=night')).body.stop, true)
})

test('control: plans and rules are passed through to the bot (and rules must be a list)', async () => {
  const { call } = setup()
  const rule = { name: 'buff', when: { kind: 'every_ms', ms: 1000 }, then: [{ kind: 'tap', key: 'F5' }] }
  let r = await call('POST', '/api/control', { rules: [rule], plan: { name: 'p', steps: [{ kind: 'rest', minutes: 1 }] } })
  assert.deepEqual(r.body.rules, [rule]); assert.equal(r.body.plan.name, 'p')
  r = await call('POST', '/api/control', { rules: 'not a list' })
  assert.deepEqual(r.body.rules, [rule], 'a non-list is ignored, the previous rules stay')
  r = await call('POST', '/api/control', { rules: [] })
  assert.deepEqual(r.body.rules, [], 'an empty list clears the rules')
})

test('control: values of the wrong type are ignored, not stored', async () => {
  const { call } = setup()
  const r = await call('POST', '/api/control', { paused: 'yes', stop: 1, settings: 'nope' })
  assert.equal(r.body.paused, undefined); assert.equal(r.body.stop, undefined); assert.equal(r.body.settings, undefined)
})

test('session names cannot escape the directory', async () => {
  const { call } = setup()
  for (const s of ['../etc', 'a/b', 'a b', '', 'x'.repeat(40)]) {
    const r = await call('GET', `/api/live?session=${encodeURIComponent(s)}`)
    assert.ok(r.status === 400 || (s === '' && r.status === 404), `${JSON.stringify(s)} -> ${r.status}`)
  }
  assert.equal((await call('POST', '/api/session/start', { session: '../x' })).status, 400)
  assert.equal((await call('POST', '/api/control?session=..%2F..%2Fx', {})).status, 400)
})

test('live: 404 until the bot writes a status file, then the file is served as is', async () => {
  const { root, call } = setup()
  assert.equal((await call('GET', '/api/live')).status, 404)
  fs.writeFileSync(path.join(root, 'neon-status.json'), JSON.stringify({ updated: Date.now(), running: true, state: { player: { level: 7 } } }))
  const r = await call('GET', '/api/live')
  assert.equal(r.status, 200); assert.equal(r.body.state.player.level, 7)
})

test('meta: map names, spawn lists and portal targets come from the calibration files', async () => {
  const { call } = setup()
  const { body } = await call('GET', '/api/meta')
  const m = body.maps.find(x => x.id === 10001030)
  assert.equal(m.name, 'Forest West of Henesys')
  assert.deepEqual(m.mobs, { 2: 13 })
  assert.deepEqual(m.portals, [10001020], 'only regular portals')
  assert.equal(m.ropes, 2)
  assert.equal(body.mobNames[2], 'Snail')
})

test('profiles: save, list, delete, and reject bad names', async () => {
  const { call } = setup()
  assert.equal((await call('POST', '/api/profiles', { name: '../evil', settings: {} })).status, 400)
  assert.equal((await call('POST', '/api/profiles', { name: 'night shift', settings: { loot: false } })).status, 200)
  const list = (await call('GET', '/api/profiles')).body
  assert.equal(list.length, 1); assert.equal(list[0].name, 'night shift'); assert.equal(list[0].settings.loot, false)
  assert.equal((await call('DELETE', '/api/profiles?name=night%20shift')).status, 200)
  assert.equal((await call('GET', '/api/profiles')).body.length, 0)
})

test('history: returns parsed lines, thinned to the requested limit, last point kept', async () => {
  const { root, call } = setup()
  fs.mkdirSync(path.join(root, 'history'))
  const lines = Array.from({ length: 1000 }, (_, i) => JSON.stringify({ t: i, kills: i }))
  fs.writeFileSync(path.join(root, 'history', 'main.jsonl'), lines.join('\n') + '\nnot json\n')
  const { body } = await call('GET', '/api/history?session=main&limit=100')
  assert.ok(body.length <= 120 && body.length >= 90, `${body.length} points`)
  assert.equal(body[body.length - 1].t, 999)
})

test('best maps: safe, reachable, low-danger maps outrank dangerous ones', async () => {
  const { root, call } = setup()
  fs.writeFileSync(path.join(root, 'neon-calib', 'mob-info.json'), JSON.stringify({ mobs: {
    2: { name: 'Snail', level: 1, hp: 45, exp: 2 }, 8: { name: 'Pig', level: 7, hp: 128, exp: 13 }, 9: { name: 'Orange Mushroom', level: 8, hp: 142, exp: 15 } } }))
  const mk = (id, mobs, portals) => fs.writeFileSync(path.join(root, 'neon-calib', 'maps', `${id}.json`), JSON.stringify({ footholds: [], ladders: [], portals: portals.map(t => ({ type: 2, to_map: t })), mobs }))
  mk(10001000, {}, [10001030, 10001020, 10009999])
  mk(10001030, { 2: 20 }, [10001000])           // 20 snails: safe
  mk(10001020, { 2: 6, 8: 16, 9: 10 }, [10001000]) // mostly pigs and mushrooms for a level-3 character
  mk(10009999, { 2: 30 }, [])                    // safe but nothing leads back: still reachable from town
  mk(10008888, { 2: 40 }, [])                    // unreachable
  const r = await call('GET', '/api/best-maps?level=3&from=10001000')
  assert.equal(r.status, 200)
  const ids = r.body.map(x => x.id)
  assert.ok(!ids.includes(10008888), 'unreachable maps are never recommended')
  assert.ok(ids.indexOf(10001030) < ids.indexOf(10001020), 'the snail map beats the pig map at level 3')
  const pig = r.body.find(x => x.id === 10001020)
  assert.ok(pig.dangerPct > 50 && pig.dangerMobs.includes('Pig'))
  assert.equal((await call('GET', '/api/best-maps?level=abc')).status, 400)
})

test('route: reports maps on the way that have mobs out-levelling the character', async () => {
  const { root, call } = setup()
  fs.writeFileSync(path.join(root, 'neon-calib', 'mob-info.json'), JSON.stringify({ mobs: { 2: { name: 'Snail', level: 1, hp: 45, exp: 2 }, 8: { name: 'Pig', level: 7, hp: 128, exp: 13 } } }))
  const mk = (id, mobs, portals) => fs.writeFileSync(path.join(root, 'neon-calib', 'maps', `${id}.json`), JSON.stringify({ footholds: [], ladders: [], portals: portals.map(t => ({ type: 2, to_map: t })), mobs }))
  mk(1, {}, [2]); mk(2, { 8: 16 }, [1, 3]); mk(3, { 2: 10 }, [2])
  let r = (await call('GET', '/api/route?level=3&from=1&to=3')).body
  assert.deepEqual(r.steps.map(s => s.id), [1, 2, 3]); assert.equal(r.risky, true); assert.deepEqual(r.riskyMaps.length, 1)
  assert.equal(r.steps[1].dangerMobs[0], 'Pig ×16')
  r = (await call('GET', '/api/route?level=9&from=1&to=3')).body
  assert.equal(r.risky, false, 'a level-9 character out-levels pigs')
  assert.equal((await call('GET', '/api/route?level=3&from=1&to=99')).body.reachable, false)
  assert.equal((await call('GET', '/api/route?level=x&from=1&to=3')).status, 400)
})

test('an unknown route is a 404 JSON error and non-API paths fall through', async () => {
  const { call } = setup()
  assert.equal((await call('GET', '/api/nope')).status, 404)
  assert.equal((await call('GET', '/index.html')).body, 'next')
})

test('quests: lists meso quests the character can do, talk-only ones first', async () => {
  const { root, call } = setup()
  const ph = (npc, extra) => ({ npc, lvmin: 8, money: null, exp: null, mobs: [], items: [], quests: [], rewards: [], ...extra })
  fs.writeFileSync(path.join(root, 'neon-calib', 'quests.json'), JSON.stringify({
    10001: { name: "Phil's Call", blocked: false, start: ph(101), end: ph(101, { money: 245, exp: 188 }) },
    10000: { name: "Olaf's Training", blocked: false, start: ph(107), end: ph(107, { money: 277, mobs: [{ id: 6, count: 8 }] }) },
    10100: { name: 'Too high', blocked: false, start: ph(101, { lvmin: 30 }), end: ph(101, { money: 9999 }) },
    10200: { name: 'No money', blocked: false, start: ph(101), end: ph(101, { exp: 5 }) },
  }))
  fs.writeFileSync(path.join(root, 'neon-calib', 'npc-names.json'), JSON.stringify({ 101: 'Phil', 107: 'Olaf' }))
  fs.writeFileSync(path.join(root, 'neon-calib', 'maps', '10000000.json'), JSON.stringify({ footholds: [], ladders: [], portals: [], mobs: {}, npcs: [{ id: 101, x: 804, y: 323 }] }))
  const r = await call('GET', '/api/quests?level=8')
  assert.equal(r.status, 200)
  assert.deepEqual(r.body.map(q => q.id), [10001, 10000], 'talk-only first, high-level and money-less quests left out')
  assert.equal(r.body[0].money, 245)
  assert.equal(r.body[0].startNpc, 'Phil')
  assert.equal(r.body[0].map, 10000000)
  assert.equal(r.body[1].easy, false)
  assert.equal((await call('GET', '/api/quests?level=x')).status, 400)
})

test('live view: status is idle until someone watches', async () => {
  const { call } = setup()
  const r = await call('GET', '/api/stream/status')
  assert.equal(r.status, 200)
  assert.equal(r.body.viewers, 0)
  assert.equal(r.body.capturing, false)
})
