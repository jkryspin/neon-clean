// Session tracking derived in the browser from the bot's history (history/<session>.jsonl, a row every 30 s),
// short-interval samples taken while the panel is open, and the live status.
//
// Why derive the level: the level read from memory does not change after a level-up, but EXP (from the
// network) is current within the level. Walking the EXP history gives the real level: every drop in EXP that
// is not explained by a death is a level-up, and EXP at or above the table's "to next level" means the
// level is at least one higher. The user can still correct it (stored per session in this browser).
import { useEffect, useRef, useState } from 'react'
import { getJson } from './api.js'

export function useHistory(session, ms = 30000) {
  const [rows, setRows] = useState(null)
  useEffect(() => {
    let alive = true
    setRows(null)
    const load = () => !document.hidden && getJson(`/api/history?session=${encodeURIComponent(session)}&limit=2000`)
      .then(r => alive && setRows(r)).catch(() => alive && setRows(r => r ?? []))
    load()
    const id = setInterval(load, ms)
    return () => { alive = false; clearInterval(id) }
  }, [session, ms])
  return rows
}

// Samples every ~2 s while the panel is open (kept for 2 hours), so charts move between history rows.
export function useSamples(session, data) {
  const ref = useRef({ key: null, list: [] })
  if (data) {
    const key = `${session}:${data.started}`
    if (ref.current.key !== key) ref.current = { key, list: [] }
    const list = ref.current.list
    const last = list[list.length - 1]
    if (!last || data.updated - last.t >= 2000) {
      const p = data.state?.player || {}
      list.push({
        t: data.updated, exp: p.exp ?? null, hp: p.hp ?? null, max_hp: p.max_hp ?? null, mp: p.mp ?? null, max_mp: p.max_mp ?? null,
        kills: data.stats?.mobs_killed ?? 0, deaths: data.stats?.deaths ?? 0, mobs: data.state?.mobs?.length ?? 0, level: p.level ?? null,
        hp_pots: data.stats?.hp_pots ?? 0,
      })
      if (list.length > 3600) list.splice(0, list.length - 3600)
    }
  }
  return ref.current.list
}

const fixKey = s => `neon.levelfix.${s}`
export function getLevelFix(session) {
  try { return Number(localStorage.getItem(fixKey(session))) || 0 } catch { return 0 }
}
export function setLevelFix(session, n) {
  try { n ? localStorage.setItem(fixKey(session), String(n)) : localStorage.removeItem(fixKey(session)) } catch { /* private mode */ }
}

const H = 3600000

// Everything the dashboard shows about progress. Pure: same inputs, same numbers.
export function deriveTrack({ rows, samples, data, expToNext = {}, fix = 0, windowMin = 10 }) {
  if (!data) return null
  const need = L => expToNext[String(L)] ?? null
  const p = data.state?.player || {}
  const memLevel = p.level ?? null
  const now = data.updated

  // One time-ordered stream: history rows, then panel samples after the last row, then the live value.
  const pts = []
  for (const r of rows || []) pts.push({ t: r.t, exp: r.exp, deaths: r.deaths ?? 0, kills: r.kills ?? 0, hp: r.hp, level: r.level, run: r.t >= data.started })
  const lastRowT = pts.length ? pts[pts.length - 1].t : -Infinity
  for (const s of samples || []) if (s.t > lastRowT) pts.push({ ...s, run: true })
  if (!pts.length || pts[pts.length - 1].t < now) {
    pts.push({ t: now, exp: p.exp ?? null, deaths: data.stats?.deaths ?? 0, kills: data.stats?.mobs_killed ?? 0, hp: p.hp, max_hp: p.max_hp, level: memLevel, run: true })
  }

  let L = null, prev = null, cum = 0
  const ups = [], expSeries = []
  for (const q of pts) {
    if (q.level != null) L = Math.max(L ?? 0, q.level)
    if (q.exp == null) continue
    if (L == null) L = 1
    if (prev) {
      const died = q.deaths > prev.deaths
      if (q.exp < prev.exp && !died) {
        cum += Math.max(0, (need(L) ?? prev.exp) - prev.exp) + q.exp
        L += 1
        ups.push({ t: q.t, level: L })
      } else if (q.exp > prev.exp) cum += q.exp - prev.exp
    }
    while (need(L) != null && q.exp >= need(L)) L += 1
    expSeries.push({ x: q.t, y: cum, run: q.run })
    prev = q
  }
  const derived = L ?? memLevel
  const level = derived != null ? derived + fix : null

  const runExp = expSeries.filter(s => s.run)
  const cumNow = expSeries.length ? expSeries[expSeries.length - 1].y : 0
  const runStartCum = runExp.length ? runExp[0].y : cumNow
  const expGained = cumNow - runStartCum
  const runMs = Math.max(0, now - data.started)

  const rateOver = (series, key = 'y') => {
    if (series.length < 2) return null
    const from = now - windowMin * 60000
    const first = series.find(s => s.x >= from) || series[0]
    const last = series[series.length - 1]
    const dt = last.x - first.x
    if (dt < 60000) return null
    return ((last[key] - first[key]) / dt) * H
  }
  // The character's EXP carries across bot restarts, so the recent rate may look back past the run start.
  const expPerHour = rateOver(expSeries) ?? (data.progress?.exp_per_hour || 0)
  const expPerHourAvg = runMs > 60000 ? (expGained / runMs) * H : expPerHour

  const exp = p.exp ?? null
  const needNow = level != null ? need(level) : null
  const pct = exp != null && needNow ? Math.min(100, (exp / needNow) * 100) : null
  const toNext = exp != null && needNow ? Math.max(0, needNow - exp) : null
  const etaNextMin = toNext != null && expPerHour > 0 ? (toNext / expPerHour) * 60 : null
  const target = data.target_level || null
  let toTarget = null
  if (target && level != null && toNext != null && target > level) {
    toTarget = toNext
    for (let l = level + 1; l < target; l++) toTarget += need(l) ?? 0
  }
  const etaTargetMin = toTarget != null && expPerHour > 0 ? (toTarget / expPerHour) * 60 : null

  // Kills: rows and samples of this run carry the bot's running total.
  const killSeries = pts.filter(q => q.run && q.kills != null).map(q => ({ x: q.t, y: q.kills }))
  const kills = data.stats?.mobs_killed ?? 0
  const killsPerHourNow = rateOver(killSeries)
  const killsPerHour = runMs > 60000 ? (kills / runMs) * H : (data.kills_per_hour || 0)

  const hpSeries = pts.filter(q => q.run && q.hp != null).map(q => ({ x: q.t, y: q.hp }))
  const lastKill = [...(data.events || [])].reverse().find(e => /^kill #/.test(e.msg))

  return {
    level, memLevel, derivedLevel: derived, fix, levelUps: ups.filter(u => u.t >= data.started - 6 * H),
    levelApprox: memLevel !== level,
    exp, need: needNow, pct, toNext, etaNextMin, target, etaTargetMin,
    expGained, expPerHour, expPerHourAvg, expSeries: runExp.map(({ x, y }) => ({ x, y: y - runStartCum })),
    kills, killsPerHour, killsPerHourNow, killSeries, hpSeries,
    runMs, lastKillAgoMs: lastKill ? now - lastKill.t : null,
    expPerKill: kills > 0 ? expGained / kills : null,
  }
}

// HP potions pressed again and again while HP does not go up: out of potions, or the key is not bound.
export function potionTrouble(data, samples, windowMs = 30000) {
  if (!data) return null
  const now = data.updated
  const presses = (data.events || []).filter(e => e.msg === 'used HP potion' && now - e.t <= windowMs).length
  if (presses < 8) return null
  const recent = samples.filter(s => now - s.t <= windowMs && s.hp != null)
  if (recent.length < 3) return null
  const start = recent[0].hp
  const best = Math.max(...recent.map(s => s.hp))
  if (best > start + 3) return null
  const p = data.state?.player || {}
  return { presses, hp: p.hp, max_hp: p.max_hp, pct: p.max_hp ? Math.round((100 * p.hp) / p.max_hp) : null }
}

// Event categories for the activity feed.
export const EVENT_KINDS = [
  ['kill', 'Kills', /^kill #/],
  ['potion', 'Potions', /potion/i],
  ['alert', 'Alerts', /pause|stop|stranger|lie detector|unavailable|lag|hp \d+%|recovered|died|death|revive|skip|stuck|ignoring|level/i],
  ['loot', 'Loot', /loot|pick/i],
  ['plan', 'Plan', /plan|rule/i],
  ['travel', 'Travel', /map|portal|travel|go to|rush/i],
]
export function eventKind(msg) {
  for (const [k, , re] of EVENT_KINDS) if (re.test(msg)) return k
  return 'other'
}
