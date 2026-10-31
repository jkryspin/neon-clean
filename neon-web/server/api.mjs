// Read-only bridge from the web UI to neon-ce's loopback API.
//
// neon-ce owns the game data; this module only reads it. Control files
// (`neon-control*.json`) are still written so a running bot picks up pause/stop.

import fs from 'node:fs'
import path from 'node:path'
import { spawn } from 'node:child_process'

const SESSION_RE = /^[a-zA-Z0-9_-]{1,32}$/
const PROFILE_RE = /^[a-zA-Z0-9 _.-]{1,48}$/
const MAX_BODY = 256 * 1024
const CE = process.env.NEON_CE_URL || 'http://127.0.0.1:8790'

export function createApi(root) {
  const data = fs.existsSync(path.join(root, 'data')) ? path.join(root, 'data') : path.join(root, 'neon-calib')
  const profilesDir = path.join(root, 'profiles')
  const historyDir = path.join(root, 'history')
  const runDir = path.join(root, 'run')
  const cePids = new Map()

  const json = (res, code, body) => {
    res.statusCode = code
    res.setHeader('Content-Type', 'application/json')
    res.setHeader('Cache-Control', 'no-store')
    res.end(JSON.stringify(body))
  }
  const readJson = (file, fallback = null) => {
    try { return JSON.parse(fs.readFileSync(file, 'utf8')) } catch { return fallback }
  }
  const atomicWrite = (file, text) => {
    const tmp = `${file}.${process.pid}.tmp`
    fs.writeFileSync(tmp, text)
    fs.renameSync(tmp, file)
  }
  const sessionOf = url => {
    const s = url.searchParams.get('session') || 'main'
    return SESSION_RE.test(s) ? s : null
  }
  const suffix = s => (s === 'main' ? '' : `.${s}`)
  const controlPath = s => path.join(root, `neon-control${suffix(s)}.json`)
  const statusPath = s => path.join(root, `neon-status${suffix(s)}.json`)

  async function ceJson(p) {
    const res = await fetch(`${CE}${p}`, { cache: 'no-store' })
    if (!res.ok) throw new Error(`ce ${res.status}`)
    return res.json()
  }

  let metaCache = null
  function meta() {
    if (metaCache) return metaCache
    const mapNames = readJson(path.join(data, 'map-names.json'), {})
    const mobNames = readJson(path.join(data, 'mob-names.json'), {})
    const maps = []
    let files = []
    try { files = fs.readdirSync(path.join(data, 'maps')) } catch { /* no maps */ }
    for (const f of files) {
      if (!f.endsWith('.json')) continue
      const id = f.slice(0, -5)
      const g = readJson(path.join(data, 'maps', f), {})
      const n = mapNames[id] || {}
      maps.push({
        id: Number(id), name: (n.name || '').trim(), street: n.street || '', region: n.region || '',
        mobs: Object.fromEntries(Object.entries(g.mobs || {})),
        portals: (g.portals || []).filter(p => p.type === 2 && p.to_map).map(p => p.to_map),
        platforms: (g.footholds || []).length, ropes: (g.ladders || []).length,
      })
    }
    maps.sort((a, b) => a.id - b.id)
    const exp = readJson(path.join(data, 'exp-table.json'), {}).exp_to_next_level || {}
    metaCache = { maps, mobNames, expToNext: exp }
    return metaCache
  }

  function questList(level, onlyMoney) {
    const quests = readJson(path.join(data, 'quests.json'), {})
    const npcNames = readJson(path.join(data, 'npc-names.json'), {})
    const mapNames = readJson(path.join(data, 'map-names.json'), {})
    const where = {}
    let files = []
    try { files = fs.readdirSync(path.join(data, 'maps')) } catch { /* no maps */ }
    for (const f of files) {
      const g = readJson(path.join(data, 'maps', f), {})
      for (const n of g.npcs || []) if (!where[n.id]) where[n.id] = Number(f.replace('.json', ''))
    }
    const rows = []
    for (const [id, q] of Object.entries(quests)) {
      if (q.blocked || !q.name || !q.start.npc) continue
      const lvmin = q.start.lvmin || 0
      const money = (q.start.money || 0) + (q.end.money || 0)
      if (onlyMoney && money <= 0) continue
      if (level && lvmin > level) continue
      const needs = [...(q.end.mobs || []).map(m => ({ kind: 'mob', id: m.id, count: m.count })), ...(q.end.items || []).map(i => ({ kind: 'item', id: i.id, count: i.count }))]
      const map = where[q.start.npc]
      rows.push({
        id: Number(id), name: q.name, lvmin, endLv: q.end.lvmin || 0, money, exp: q.end.exp || 0,
        startNpc: npcNames[q.start.npc] || String(q.start.npc), endNpc: npcNames[q.end.npc] || String(q.end.npc),
        map: map || 0, mapName: map ? (mapNames[map] || {}).name || '' : '',
        needs, prereq: (q.start.quests || []).filter(p => p.state === 2).map(p => p.id),
        easy: needs.length === 0, reachable: !!map && Number(id) < 500000,
      })
    }
    rows.sort((a, b) => Number(b.easy) - Number(a.easy) || b.money - a.money || a.lvmin - b.lvmin)
    return rows.slice(0, 200)
  }

  function bestMaps(level, fromMap) {
    const { maps } = meta()
    const info = readJson(path.join(data, 'mob-info.json'), { mobs: {} }).mobs
    const hops = new Map([[fromMap, 0]])
    const byId = new Map(maps.map(m => [m.id, m]))
    for (const q = [fromMap]; q.length;) {
      const cur = q.shift()
      for (const t of byId.get(cur)?.portals || []) if (!hops.has(t) && byId.has(t)) { hops.set(t, hops.get(cur) + 1); q.push(t) }
    }
    const rows = []
    for (const m of maps) {
      let safe = 0, fair = 0, danger = 0, value = 0, penalty = 0
      const safeMobs = [], dangerMobs = []
      for (const [id, n] of Object.entries(m.mobs)) {
        const mob = info[id]
        if (!mob || !mob.exp) continue
        const eff = mob.exp / mob.hp
        if (mob.level < level) { safe += n; value += n * eff; safeMobs.push(mob.name) }
        else if (mob.level <= level + 1) { fair += n; value += n * eff * 0.5 }
        else { danger += n; penalty += n * eff * 1.5; dangerMobs.push(mob.name) }
      }
      if (safe < 5) continue
      const total = safe + fair + danger
      rows.push({ id: m.id, name: m.name, safe, fair, danger, dangerPct: Math.round((100 * danger) / total), safeMobs: [...new Set(safeMobs)], dangerMobs: [...new Set(dangerMobs)], score: Math.round((value - penalty) * 100) / 100, hops: hops.get(m.id) ?? null })
    }
    return rows.filter(r => r.hops !== null).sort((a, b) => (b.score - b.hops * 0.5) - (a.score - a.hops * 0.5)).slice(0, 10)
  }

  function routeInfo(level, from, to) {
    const { maps } = meta()
    const info = readJson(path.join(data, 'mob-info.json'), { mobs: {} }).mobs
    const byId = new Map(maps.map(m => [m.id, m]))
    const prev = new Map([[from, null]])
    for (const q = [from]; q.length;) {
      const cur = q.shift()
      if (cur === to) break
      for (const t of byId.get(cur)?.portals || []) if (!prev.has(t) && byId.has(t)) { prev.set(t, cur); q.push(t) }
    }
    if (!prev.has(to)) return { reachable: false, steps: [] }
    const ids = []
    for (let m = to; m !== null; m = prev.get(m)) ids.unshift(m)
    const steps = ids.map(id => {
      const m = byId.get(id)
      const danger = Object.entries(m.mobs).filter(([k]) => info[k] && info[k].level > level + 1)
      return { id, name: m.name, dangerous: danger.reduce((a, [, n]) => a + n, 0), dangerMobs: danger.map(([k, n]) => `${info[k].name} ×${n}`) }
    })
    const through = steps.slice(1, -1).filter(s => s.dangerous > 0)
    return { reachable: true, steps, risky: through.length > 0, riskyMaps: through.map(s => s.name) }
  }

  async function live(session) {
    const status = readJson(statusPath(session))
    let ce = null
    try { ce = await ceJson('/api/live') } catch { /* CE not running */ }
    if (status && Date.now() - (status.updated || 0) < 4000) {
      return { ...status, alive: true, using: 'bot' }
    }
    if (!ce) {
      if (status) return { ...status, alive: false, using: 'bot' }
      const err = new Error('no status file yet')
      err.status = 404
      throw err
    }
    return {
      updated: ce.ts_ms || Date.now(), running: !ce.error, alive: !ce.error, mode: 'running', dry_run: false,
      state: {
        player: { x: ce.x ?? 0, y: ce.y ?? 0, map_id: ce.map_id ?? null, level: ce.level ?? null, hp: ce.hp ?? null, max_hp: ce.max_hp ?? null, mp: ce.mp ?? null },
        mobs: (ce.contacts || []).map(c => ({ x: c.x, y: c.y, id: null })), others: [], drops: [], footholds: [],
      },
      stats: { mobs_killed: 0, kills_by_mob: {} }, progress: { exp_gained: 0, exp_per_hour: 0, session_minutes: 0 },
      events: [], series: [], rules: [], using: 'ce', error: ce.error || null,
    }
  }

  function mergeControl(session, patch) {
    const file = controlPath(session)
    const cur = readJson(file, { seq: 0 })
    const next = { ...cur, seq: (cur.seq || 0) + 1 }
    if (typeof patch.paused === 'boolean') next.paused = patch.paused
    if (typeof patch.stop === 'boolean') next.stop = patch.stop
    if (patch.settings && typeof patch.settings === 'object') next.settings = { ...(cur.settings || {}), ...patch.settings }
    if (patch.plan !== undefined) next.plan = patch.plan
    if (Array.isArray(patch.rules)) next.rules = patch.rules
    if (patch.clearStop) next.stop = false
    atomicWrite(file, JSON.stringify(next))
    return next
  }

  function tailLines(file, max) {
    let text = ''
    try { text = fs.readFileSync(file, 'utf8') } catch { return [] }
    const rows = text.split('\n').filter(Boolean).map(l => { try { return JSON.parse(l) } catch { return null } }).filter(Boolean)
    if (rows.length <= max) return rows
    const step = Math.ceil(rows.length / max)
    return rows.filter((_, i) => i % step === 0 || i === rows.length - 1)
  }

  function ceExe() {
    return process.env.NEON_CE_EXE || path.join(root, 'target', 'debug', 'neon-ce.exe')
  }

  async function startSession(session) {
    try {
      const s = await ceJson('/api/health')
      if (s.ok) return { ok: true, note: 'neon-ce already running' }
    } catch { /* not running */ }
    let pid = null
    try { pid = Number(fs.readFileSync(path.join(runDir, `${session}.pid`), 'utf8')) } catch { /* none */ }
    if (pid) {
      try { process.kill(pid, 0); return { ok: false, error: 'already running' } } catch { /* stale */ }
    }
    mergeControl(session, { clearStop: true, paused: false })
    fs.mkdirSync(runDir, { recursive: true })
    const child = spawn(ceExe(), [], { cwd: root, detached: true, stdio: 'ignore' })
    child.unref()
    cePids.set(session, child.pid)
    fs.writeFileSync(path.join(runDir, `${session}.pid`), String(child.pid))
    return { ok: true, pid: child.pid }
  }

  function stopSession(session) {
    mergeControl(session, { stop: true })
    const pid = cePids.get(session)
    if (pid) { try { process.kill(pid) } catch { /* gone */ } }
    return { ok: true }
  }

  const body = req => new Promise((resolve, reject) => {
    let size = 0
    const chunks = []
    req.on('data', c => {
      size += c.length
      if (size > MAX_BODY) { reject(new Error('body too large')); req.destroy(); return }
      chunks.push(c)
    })
    req.on('end', () => {
      try { resolve(chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {}) } catch { reject(new Error('invalid JSON')) }
    })
    req.on('error', reject)
  })

  const handler = async function handler(req, res, next) {
    const url = new URL(req.url, 'http://localhost')
    const p = url.pathname
    if (!p.startsWith('/api/')) return next()
    try {
      if (req.method === 'GET' && p === '/api/live') {
        const s = sessionOf(url)
        if (!s) return json(res, 400, { error: 'bad session' })
        return json(res, 200, await live(s))
      }
      if (req.method === 'GET' && p === '/api/meta') return json(res, 200, meta())
      if (req.method === 'GET' && p === '/api/sessions') {
        return json(res, 200, [{ name: 'main', updated: Date.now(), running: true, mode: 'running', using: 'ce' }])
      }
      if (req.method === 'GET' && p === '/api/route') {
        const level = Number(url.searchParams.get('level')), from = Number(url.searchParams.get('from')), to = Number(url.searchParams.get('to'))
        if (![level, from, to].every(Number.isInteger) || level < 1) return json(res, 400, { error: 'level, from and to must be integers' })
        return json(res, 200, routeInfo(level, from, to))
      }
      if (req.method === 'GET' && p === '/api/quests') {
        const level = Number(url.searchParams.get('level') || 0)
        if (!Number.isInteger(level) || level < 0 || level > 300) return json(res, 400, { error: 'bad level' })
        return json(res, 200, questList(level, url.searchParams.get('money') !== '0'))
      }
      if (req.method === 'GET' && p === '/api/best-maps') {
        const level = Number(url.searchParams.get('level')), from = Number(url.searchParams.get('from') || 10001000)
        if (!Number.isInteger(level) || level < 1) return json(res, 400, { error: 'bad level' })
        return json(res, 200, bestMaps(level, from))
      }
      if (p === '/api/control') {
        const s = sessionOf(url)
        if (!s) return json(res, 400, { error: 'bad session' })
        if (req.method === 'GET') return json(res, 200, readJson(controlPath(s), { seq: 0 }))
        if (req.method === 'POST') return json(res, 200, mergeControl(s, await body(req)))
      }
      if (req.method === 'GET' && p === '/api/history') {
        const s = sessionOf(url)
        if (!s) return json(res, 400, { error: 'bad session' })
        return json(res, 200, tailLines(path.join(historyDir, `${s}.jsonl`), Math.min(Number(url.searchParams.get('limit')) || 400, 2000)))
      }
      if (req.method === 'POST' && p === '/api/session/start') {
        const b = await body(req)
        const s = SESSION_RE.test(b.session || 'main') ? (b.session || 'main') : null
        return s ? json(res, 200, await startSession(s)) : json(res, 400, { error: 'bad session' })
      }
      if (req.method === 'POST' && p === '/api/session/stop') {
        const b = await body(req)
        const s = SESSION_RE.test(b.session || 'main') ? (b.session || 'main') : null
        return s ? json(res, 200, stopSession(s)) : json(res, 400, { error: 'bad session' })
      }
      if (p === '/api/profiles') {
        fs.mkdirSync(profilesDir, { recursive: true })
        if (req.method === 'GET') return json(res, 200, fs.readdirSync(profilesDir).filter(f => f.endsWith('.json')).map(f => ({ name: f.slice(0, -5), ...readJson(path.join(profilesDir, f), {}) })))
        if (req.method === 'POST') {
          const b = await body(req)
          if (!PROFILE_RE.test(b.name || '')) return json(res, 400, { error: 'bad profile name' })
          atomicWrite(path.join(profilesDir, `${b.name}.json`), JSON.stringify({ settings: b.settings || {}, plan: b.plan || null }))
          return json(res, 200, { ok: true })
        }
        if (req.method === 'DELETE') {
          const name = url.searchParams.get('name') || ''
          if (!PROFILE_RE.test(name)) return json(res, 400, { error: 'bad profile name' })
          fs.rmSync(path.join(profilesDir, `${name}.json`), { force: true })
          return json(res, 200, { ok: true })
        }
      }
      if (req.method === 'GET' && p === '/api/stream/status') {
        return json(res, 200, { viewers: 0, capturing: false })
      }
      if (req.method === 'GET' && (p === '/api/stream' || p === '/api/frame.jpg')) {
        return json(res, 200, { ok: false, error: 'live frame capture is not wired on Windows yet' })
      }
      return json(res, 404, { error: 'unknown route' })
    } catch (e) {
      return json(res, e.status || 400, { error: String(e.message || e) })
    }
  }
  handler.meta = meta
  return handler
}
