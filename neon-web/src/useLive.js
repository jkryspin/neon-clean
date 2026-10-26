import { useEffect, useRef, useState } from 'react'
import { getJson } from './api.js'

// The bot omits empty lists; give the UI a complete shape.
function normalize(d) {
  if (!d) return d
  if (d.state) {
    d.state.mobs ??= []; d.state.others ??= []; d.state.drops ??= []; d.state.footholds ??= []
  }
  d.events ??= []; d.series ??= []
  d.rules ??= []
  d.progress ??= { exp_gained: 0, exp_per_hour: 0, session_minutes: 0 }
  d.stats ??= {}
  d.stats.kills_by_mob ??= {}
  return d
}

// Polls the live status of one session. `stale` means the bot stopped writing (crashed or exited).
export function useLive(session = 'main', ms = 500) {
  const [data, setData] = useState(() => (session === 'main' ? normalize(window.__LIVE__ ?? null) : null))
  const [error, setError] = useState(null)
  const [now, setNow] = useState(Date.now())
  const first = useRef(true)

  useEffect(() => {
    let alive = true
    // Keep the inlined first paint; only clear when the user switches to another session.
    if (first.current) first.current = false
    else setData(null)
    const load = async () => {
      if (document.hidden) return
      try {
        const json = await getJson(`/api/live?session=${encodeURIComponent(session)}`)
        if (alive) { setData(normalize(json)); setError(null) }
      } catch (e) {
        if (alive) setError(e.message)
      }
      if (alive) setNow(Date.now())
    }
    load()
    const id = setInterval(load, ms)
    return () => { alive = false; clearInterval(id) }
  }, [session, ms])

  const age = data ? now - data.updated : null
  // A live bot process that is busy (idle break, NPC conversation) is not stale, just quiet.
  return { data, error, stale: age != null && age > 4000 && !data?.alive, busy: age != null && age > 4000 && !!data?.alive, age }
}

// Reference data (map and mob names, spawn lists, EXP table): fetched once.
let metaPromise = null
export function useMeta() {
  const [meta, setMeta] = useState(() => window.__META__ ?? null)
  useEffect(() => {
    if (window.__META__) return
    metaPromise ??= getJson('/api/meta')
    metaPromise.then(setMeta).catch(() => setMeta({ maps: [], mobNames: {}, expToNext: {} }))
  }, [])
  return meta
}

export function useSessions(ms = 3000) {
  const [list, setList] = useState([])
  useEffect(() => {
    let alive = true
    const load = () => !document.hidden && getJson('/api/sessions').then(l => alive && setList(l)).catch(() => {})
    load()
    const id = setInterval(load, ms)
    return () => { alive = false; clearInterval(id) }
  }, [ms])
  return list
}
