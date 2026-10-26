// Local API for the Neon web UI (used by the Vite dev/preview server, no extra dependencies).
//
// Everything is file based: the bot (neon-loader) writes neon-status[.<session>].json and watches
// neon-control[.<session>].json; this module reads the first and writes the second. Mutating routes
// validate their input and are meant for localhost only (the dev server binds 127.0.0.1 unless told otherwise).

import fs from 'node:fs'
import path from 'node:path'
import { spawn } from 'node:child_process'
import { createStream } from './stream.mjs'

const SESSION_RE = /^[a-zA-Z0-9_-]{1,32}$/
const PROFILE_RE = /^[a-zA-Z0-9 _.-]{1,48}$/
const MAX_BODY = 256 * 1024

export function createApi(root) {
  const calib = path.join(root, 'neon-calib')
  const profilesDir = path.join(root, 'profiles')
  const historyDir = path.join(root, 'history')
  const runDir = path.join(root, 'run')
  // Live views, one per display. A session that is an instance (neon-instances.json) is drawn on its own Xvfb display.
  const games = new Map()
  const displayOf = session => {
    try {
      const reg = JSON.parse(fs.readFileSync(path.join(root, 'neon-instances.json'), 'utf8'))
      const d = reg[session]?.display
      if (Number.isInteger(d)) return `:${d}`
    } catch { /* no registry */ }
    return ':0'
  }
  const gameFor = url => {
    const d = displayOf(url.searchParams.get('session') || '')
    if (!games.has(d)) games.set(d, createStream(d))
    return games.get(d)
  }
  const suffix = s => (s === 'main' ? '' : `.${s}`)
  const statusPath = s => path.join(root, `neon-status${suffix(s)}.json`)
  const controlPath = s => path.join(root, `neon-control${suffix(s)}.json`)

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
  const body = req => new Promise((resolve, reject) => {
    let size = 0
    const chunks = []
    req.on('data', c => {
      size += c.length
      if (size > MAX_BODY) { reject(new Error('body too large')); req.destroy(); return }
      chunks.push(c)
    })
    req.on('end', () => {
      try { resolve(chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {}) } catch (e) { reject(new Error('invalid JSON')) }
    })
    req.on('error', reject)
  })

  // ---- static reference data (map and mob names, spawn lists) ---------------------------------
  let metaCache = null
  function meta() {
    if (metaCache) return metaCache
    const mapNames = readJson(path.join(calib, 'map-names.json'), {})
    const mobNames = readJson(path.join(calib, 'mob-names.json'), {})
    const maps = []
    let files = []
    try { files = fs.readdirSync(path.join(calib, 'maps')) } catch { /* no map data yet */ }
    for (const f of files) {
      if (!f.endsWith('.json')) continue
      const id = f.slice(0, -5)
      const g = readJson(path.join(calib, 'maps', f), {})
      const n = mapNames[id] || {}
      maps.push({
        id: Number(id),
        name: (n.name || '').trim(),
        street: n.street || '',
        region: n.region || '',
        mobs: Object.fromEntries(Object.entries(g.mobs || {}).map(([k, v]) => [k, v])),
        portals: (g.portals || []).filter(p => p.type === 2 && p.to_map).map(p => p.to_map),
        platforms: (g.footholds || []).length,
        ropes: (g.ladders || []).length,
      })
    }
    maps.sort((a, b) => a.id - b.id)
    const exp = readJson(path.join(calib, 'exp-table.json'), {}).exp_to_next_level || {}
    metaCache = { maps, mobNames, expToNext: exp }
    return metaCache
  }

  // ---- quests ---------------------------------------------------------------------------------
  // neon-calib/quests.json comes from the client's QuestData (neon-calib/tools/wzquests.py). "easy" = nothing to
  // kill or collect, so the bot can do it by talking alone; prerequisites that are other quests are listed.
  function questList(level, onlyMoney) {
    const quests = readJson(path.join(calib, 'quests.json'), {})
    const npcNames = readJson(path.join(calib, 'npc-names.json'), {})
    const mapNames = readJson(path.join(calib, 'map-names.json'), {})
    const where = {}
    let files = []
    try { files = fs.readdirSync(path.join(calib, 'maps')) } catch { /* no map data */ }
    for (const f of files) {
      const g = readJson(path.join(calib, 'maps', f), {})
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

  // ---- best maps for a level ----------------------------------------------------------------
  // A mob is "safe" when it is weaker than you (level < yours), "fair" when it is about your level and
  // "dangerous" when it clearly out-levels you. Value is EXP per HP: how much EXP a unit of fighting buys.
  function bestMaps(level, fromMap) {
    const { maps } = meta()
    const info = readJson(path.join(calib, 'mob-info.json'), { mobs: {} }).mobs
    // Portal hops from where you stand (breadth first), so far-away maps rank lower.
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
      rows.push({
        id: m.id, name: m.name, safe, fair, danger, dangerPct: Math.round((100 * danger) / total),
        safeMobs: [...new Set(safeMobs)], dangerMobs: [...new Set(dangerMobs)],
        score: Math.round((value - penalty) * 100) / 100,
        hops: hops.has(m.id) ? hops.get(m.id) : null,
      })
    }
    // Reachable first (a map you cannot walk to is not a recommendation), then by score with a small distance cost.
    return rows
      .filter(r => r.hops !== null)
      .sort((a, b) => (b.score - b.hops * 0.5) - (a.score - a.hops * 0.5))
      .slice(0, 10)
  }

  // ---- route with danger report ---------------------------------------------------------------
  // The walk between maps crosses every map on the way. Report which of them have mobs that out-level you.
  function routeInfo(level, from, to) {
    const { maps } = meta()
    const info = readJson(path.join(calib, 'mob-info.json'), { mobs: {} }).mobs
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
      return {
        id, name: m.name, dangerous: danger.reduce((a, [, n]) => a + n, 0),
        dangerMobs: danger.map(([k, n]) => `${info[k].name} ×${n}`),
      }
    })
    // The start and end maps are not "walked through" in the risky sense; the ones between them are.
    const through = steps.slice(1, -1).filter(s => s.dangerous > 0)
    return { reachable: true, steps, risky: through.length > 0, riskyMaps: through.map(s => s.name) }
  }

  // ---- sessions -----------------------------------------------------------------------------
  // A bot is up while its run-bot.sh process lives (run/NAME.pid), even when it is not writing its status: idle
  // breaks, NPC conversations, stat-window clicks and shop trips block the writer for seconds to minutes.
  function botAlive(name) {
    try {
      const pid = Number(fs.readFileSync(path.join(runDir, `${name}.pid`), 'utf8').trim())
      if (!pid) return false
      process.kill(pid, 0)
      return true
    } catch { return false }
  }

  function listSessions() {
    const out = []
    for (const f of fs.readdirSync(root)) {
      const m = /^neon-status(?:\.([a-zA-Z0-9_-]+))?\.json$/.exec(f)
      if (!m) continue
      const name = m[1] || 'main'
      const s = readJson(path.join(root, f))
      if (!s) continue
      const pidFile = path.join(runDir, `${name}.pid`)
      out.push({
        name,
        updated: s.updated,
        running: botAlive(name) || (!!s.running && Date.now() - s.updated < 5000),
        busy: botAlive(name) && Date.now() - s.updated >= 5000,
        mode: s.mode,
        dry_run: s.dry_run,
        level: s.state?.player?.level ?? null,
        map: s.state?.player?.map_id ?? null,
        hp: s.state?.player?.hp ?? null,
        max_hp: s.state?.player?.max_hp ?? null,
        kills: s.stats?.mobs_killed ?? 0,
        exp_per_hour: s.progress?.exp_per_hour ?? 0,
        // Every run-bot.sh writes a pid file; only one started with --supervise restarts the bot.
        supervised: (() => {
          try { return fs.readFileSync(`/proc/${fs.readFileSync(pidFile, 'utf8').trim()}/cmdline`, 'utf8').split('\0').includes('--supervise') } catch { return false }
        })(),
      })
    }
    return out
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
    // Parse first: a half-written last line must not displace the real last record.
    const rows = text.split('\n').filter(Boolean).map(l => { try { return JSON.parse(l) } catch { return null } }).filter(Boolean)
    if (rows.length <= max) return rows
    const step = Math.ceil(rows.length / max)
    return rows.filter((_, i) => i % step === 0 || i === rows.length - 1)
  }

  function startSession(session, args) {
    const pidFile = path.join(runDir, `${session}.pid`)
    if (fs.existsSync(pidFile)) {
      const pid = Number(fs.readFileSync(pidFile, 'utf8'))
      try { process.kill(pid, 0); return { ok: false, error: 'already running' } } catch { fs.rmSync(pidFile, { force: true }) }
    }
    const safe = (args || []).filter(a => typeof a === 'string' && /^[\w.:,=/@+-]{1,200}$/.test(a))
    // Starting afresh clears an old stop request so the new run is not stopped on arrival.
    mergeControl(session, { clearStop: true, paused: false })
    fs.mkdirSync(path.join(root, 'logs'), { recursive: true })
    const log = fs.openSync(path.join(root, 'logs', `${session}.web.log`), 'a')
    const child = spawn(path.join(root, 'scripts', 'run-bot.sh'), ['--supervise', '--session', session, '--', ...safe], {
      cwd: root, detached: true, stdio: ['ignore', log, log],
    })
    child.unref()
    return { ok: true, pid: child.pid }
  }

  function stopSession(session) {
    mergeControl(session, { stop: true })
    const pidFile = path.join(runDir, `${session}.pid`)
    if (fs.existsSync(pidFile)) {
      const pid = Number(fs.readFileSync(pidFile, 'utf8'))
      // Give the loader a moment to stop on its own (it releases every key), then end the supervisor.
      setTimeout(() => { try { process.kill(-pid, 'SIGTERM') } catch { try { process.kill(pid, 'SIGTERM') } catch { /* gone */ } } }, 2500)
    }
    return { ok: true }
  }

  // ---- routing -------------------------------------------------------------------------------
  const handler = async function handler(req, res, next) {
    const url = new URL(req.url, 'http://localhost')
    const p = url.pathname
    if (!p.startsWith('/api/')) return next()
    try {
      if (req.method === 'GET' && p === '/api/live') {
        const s = sessionOf(url)
        if (!s) return json(res, 400, { error: 'bad session' })
        const file = statusPath(s)
        try {
          const d = JSON.parse(fs.readFileSync(file, 'utf8'))
          d.alive = botAlive(s)
          return json(res, 200, d)
        } catch {
          return json(res, 404, { error: 'no status file yet', path: path.basename(file) })
        }
      }
      // Live view of the game window (read-only screen grabs); see server/stream.mjs.
      if (req.method === 'GET' && p === '/api/stream') return gameFor(url).stream(req, res, url)
      if (req.method === 'GET' && p === '/api/frame.jpg') return await gameFor(url).frame(req, res, url)
      if (req.method === 'GET' && p === '/api/stream/status') return json(res, 200, gameFor(url).status())
      if (req.method === 'GET' && p === '/api/sessions') return json(res, 200, listSessions())
      if (req.method === 'GET' && p === '/api/meta') return json(res, 200, meta())
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
        if (!Number.isInteger(level) || level < 1 || level > 200) return json(res, 400, { error: 'bad level' })
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
        const max = Math.min(Number(url.searchParams.get('limit')) || 400, 2000)
        return json(res, 200, tailLines(path.join(historyDir, `${s}.jsonl`), max))
      }
      if (req.method === 'POST' && p === '/api/session/start') {
        const b = await body(req)
        const s = SESSION_RE.test(b.session || 'main') ? (b.session || 'main') : null
        if (!s) return json(res, 400, { error: 'bad session' })
        return json(res, 200, startSession(s, b.args))
      }
      if (req.method === 'POST' && p === '/api/session/stop') {
        const b = await body(req)
        const s = SESSION_RE.test(b.session || 'main') ? (b.session || 'main') : null
        if (!s) return json(res, 400, { error: 'bad session' })
        return json(res, 200, stopSession(s))
      }
      if (p === '/api/profiles') {
        fs.mkdirSync(profilesDir, { recursive: true })
        if (req.method === 'GET') {
          const list = fs.readdirSync(profilesDir).filter(f => f.endsWith('.json')).map(f => ({ name: f.slice(0, -5), ...readJson(path.join(profilesDir, f), {}) }))
          return json(res, 200, list)
        }
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
      return json(res, 404, { error: 'unknown route' })
    } catch (e) {
      return json(res, 400, { error: String(e.message || e) })
    }
  }
  handler.meta = meta
  return handler
}
