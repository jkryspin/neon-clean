import { useEffect, useMemo, useState } from 'react'
import { control, getJson } from '../api.js'
import { Card, Empty, mapLabel, mobLabel } from '../ui.jsx'
import { parseIds, classify, summarize } from '../keepList.js'

// Preview of a shop step's keep rule on a pasted list of item ids (one tab, in slot order).
function KeepPreview({ keep, keepItems }) {
  const [text, setText] = useState('')
  const rows = classify(parseIds(text), keep || 0, keepItems || [])
  const { kept, sold } = summarize(rows)
  return (
    <div className="span-all">
      <label className="mini">preview: item ids in slot order<input type="text" value={text} onChange={e => setText(e.target.value)} /></label>
      {rows.length > 0 && <small>{kept} kept, {sold} sold: {rows.map(r => `${r.id}${r.action === 'keep' ? ' ✓' : ''}`).join(', ')}</small>}
    </div>
  )
}

const STEP_KINDS = [['travel', 'Travel to a map'], ['grind', 'Grind in a map'], ['rest', 'Rest'], ['shop', 'Shop trip (sell / buy)'], ['quest', 'Quest (talk to the NPC)']]
const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun']
const SELL_TABS = [['equip', 'Equip'], ['use', 'Use'], ['setup', 'Set-up'], ['etc', 'Etc']]
// Henesys Department Store: Luna at this spot of the 1422x900 window, shop row 0 = Red Potion.
const newStep = kind => (kind === 'shop' ? { kind, map: 10001042, npc: [452, 518], sell: ['etc'], keep: 0, keep_items: [], buy: [{ slot: 0, qty: 50 }] } : kind === 'quest' ? { kind, quest: 0, complete: false } : kind === 'travel' ? { kind, map: 0 } : kind === 'grind' ? { kind, map: 0, until_level: null, minutes: null, kills: null } : { kind, minutes: 5 })
const tzOffset = () => -new Date().getTimezoneOffset()

// A share code is the plan as JSON, base64 encoded: paste one in to use someone else's schedule.
const encode = plan => `NEON1:${btoa(unescape(encodeURIComponent(JSON.stringify(plan))))}`
function decode(code) {
  const body = code.trim().replace(/^NEON1:/, '')
  return JSON.parse(decodeURIComponent(escape(atob(body))))
}

function MapPicker({ meta, value, onChange }) {
  const [q, setQ] = useState('')
  const matches = useMemo(() => {
    const s = q.trim().toLowerCase()
    if (!meta) return []
    const list = s
      ? meta.maps.filter(m => `${m.name} ${m.street} ${m.id} ${Object.keys(m.mobs).map(id => meta.mobNames[id] || '').join(' ')}`.toLowerCase().includes(s))
      : meta.maps.filter(m => Object.keys(m.mobs).length > 0)
    return list.slice(0, 30)
  }, [q, meta])
  return (
    <div className="picker">
      <input placeholder="Search maps or mobs (e.g. snail, Henesys)" value={q} onChange={e => setQ(e.target.value)} aria-label="Search maps" />
      <div className="picker-list" role="listbox">
        {matches.map(m => (
          <button key={m.id} role="option" aria-selected={value === m.id} className={`pick ${value === m.id ? 'on' : ''}`} onClick={() => onChange(m.id)}>
            <b>{m.name || `Map ${m.id}`}</b> <small>{m.id}</small>
            <span>{Object.entries(m.mobs).slice(0, 5).map(([id, n]) => `${mobLabel(meta, Number(id))} ×${n}`).join(', ') || 'no mobs'}</span>
          </button>
        ))}
        {matches.length === 0 && <Empty>No match</Empty>}
      </div>
    </div>
  )
}

function QuestPicker({ level, value, onChange }) {
  const [rows, setRows] = useState(null)
  const [q, setQ] = useState('')
  useEffect(() => { getJson(`/api/quests?level=${level || 0}`).then(setRows).catch(() => setRows([])) }, [level])
  const list = (rows || []).filter(r => `${r.name} ${r.startNpc} ${r.mapName} ${r.id}`.toLowerCase().includes(q.trim().toLowerCase())).slice(0, 30)
  return (
    <div className="picker">
      <input placeholder="Search quests that pay mesos (name, NPC, town)" value={q} onChange={e => setQ(e.target.value)} aria-label="Search quests" />
      <div className="picker-list" role="listbox">
        {list.map(r => (
          <button key={r.id} role="option" aria-selected={value === r.id} className={`pick ${value === r.id ? 'on' : ''}`} onClick={() => onChange(r)}>
            <b>{r.name}</b> <small>{r.id} · level {r.lvmin}{r.endLv > r.lvmin ? `–${r.endLv} to finish` : ''} · {r.money} mesos · {r.exp} exp</small>
            <span>{r.startNpc} in {r.mapName || 'an unknown map'}{r.easy ? ' · talk only' : ` · needs ${r.needs.map(n => `${n.count}× ${n.kind} ${n.id}`).join(', ')}`}{r.prereq.length ? ` · after quest ${r.prereq.join(', ')}` : ''}{r.reachable ? '' : ' · not reachable'}</span>
          </button>
        ))}
        {rows && list.length === 0 && <Empty>No quest matches</Empty>}
      </div>
    </div>
  )
}

export default function Plan({ session, live, meta, track }) {
  const here = live.data?.state?.player?.map_id
  const [plan, setPlan] = useState({ name: 'My plan', steps: [], repeat: false, windows: [], utc_offset_min: tzOffset() })
  const [pick, setPick] = useState(null) // index of the step whose map is being chosen
  const [rushTo, setRushTo] = useState(0)
  const [code, setCode] = useState('')
  const [msg, setMsg] = useState(null)
  const [best, setBest] = useState(null)
  const level = track?.level ?? live.data?.state?.player?.level
  const [route, setRoute] = useState(null)
  useEffect(() => {
    if (!rushTo || !level || !here) { setRoute(null); return }
    getJson(`/api/route?level=${level}&from=${here}&to=${rushTo}`).then(setRoute).catch(() => setRoute(null))
  }, [rushTo, level, here])
  const suggest = () => getJson(`/api/best-maps?level=${level}&from=${here || 10001000}`).then(setBest).catch(e => setMsg(e.message))

  const upd = patch => setPlan(p => ({ ...p, ...patch }))
  const updStep = (i, patch) => upd({ steps: plan.steps.map((s, j) => (j === i ? { ...s, ...patch } : s)) })
  const send = async p => {
    try { await control(session, { plan: p }); setMsg(p.steps.length ? `Plan '${p.name}' sent to the bot.` : 'Plan cleared.') } catch (e) { setMsg(`Failed: ${e.message}`) }
  }
  const rush = async () => {
    if (!rushTo) return
    await send({ name: `Rush to ${mapLabel(meta, rushTo)}`, steps: [{ kind: 'travel', map: rushTo }], repeat: false, windows: [], utc_offset_min: tzOffset() })
  }
  const importCode = () => { try { setPlan(decode(code)); setMsg('Imported.') } catch { setMsg('That is not a valid share code.') } }

  return (
    <main className="grid">
      <Card title="Map rusher" className="span3">
        <p className="hint">Walk to any map through portals, then stand there. You are in <b>{mapLabel(meta, here)}</b>.</p>
        <div className="controls">
          <button className="btn small" disabled={!level} onClick={suggest}>Suggest maps for level {level ?? '…'}</button>
          <span className="hint">Ranked by EXP per HP of mobs weaker than you, penalising mobs that out-level you, near first.</span>
        </div>
        {best && (
          <div className="picker-list" aria-label="Suggested maps">
            {best.map(b => (
              <button key={b.id} className={`pick ${rushTo === b.id ? 'on' : ''}`} onClick={() => setRushTo(b.id)}>
                <b>{b.name || `Map ${b.id}`}</b> <small>{b.id} · {b.hops} portal{b.hops === 1 ? '' : 's'} away · score {b.score}</small>
                <span>safe: {b.safeMobs.join(', ')}{b.danger ? ` · ${b.dangerPct}% dangerous (${b.dangerMobs.join(', ')})` : ''}</span>
              </button>
            ))}
            {best.length === 0 && <Empty>No map in the data suits this level.</Empty>}
          </div>
        )}
        <MapPicker meta={meta} value={rushTo} onChange={setRushTo} />
        {route && (
          <div className={`notice ${route.risky ? 'alarm' : ''}`}>
            {!route.reachable ? 'No portal route from here.' : <>Route: {route.steps.map(s => s.name || s.id).join(' → ')}.{' '}
              {route.risky ? <b>Passes through maps with mobs that out-level you: {route.steps.slice(1, -1).filter(s => s.dangerous).map(s => `${s.name} (${s.dangerMobs.join(', ')})`).join('; ')}. The bot only walks there, but a low-level character can die on the way.</b> : 'No dangerous maps on the way.'}</>}
          </div>
        )}
        <div className="controls">
          <button className="btn primary" disabled={!rushTo} onClick={rush}>Rush to {rushTo ? mapLabel(meta, rushTo) : '…'}</button>
          {msg && <span className="hint">{msg}</span>}
        </div>
      </Card>

      <Card title="Task scheduler" className="span3">
        <div className="fields">
          <label className="field"><span className="field-label">Plan name</span><span className="field-input"><input value={plan.name} onChange={e => upd({ name: e.target.value })} /></span></label>
          <label className="toggle"><input type="checkbox" checked={plan.repeat} onChange={e => upd({ repeat: e.target.checked })} /><span className="switch" /><span className="toggle-text">Repeat when finished</span></label>
        </div>

        <h3>Steps</h3>
        {plan.steps.length === 0 && <Empty>No steps yet. A plan runs its steps in order.</Empty>}
        {plan.steps.map((s, i) => (
          <div className="step" key={i}>
            <b>{i + 1}.</b>
            <span>{STEP_KINDS.find(k => k[0] === s.kind)?.[1]}</span>
            {s.kind === 'quest' && (
              <>
                <button className="btn small" onClick={() => setPick(pick === i ? null : i)}>{s.quest ? `Quest ${s.quest}${s.name ? `: ${s.name}` : ''}` : 'Choose quest…'}</button>
                <label className="mini">hand in<input type="checkbox" checked={!!s.complete} onChange={e => updStep(i, { complete: e.target.checked })} /></label>
              </>
            )}
            {s.kind !== 'rest' && s.kind !== 'quest' && (
              <button className="btn small" onClick={() => setPick(pick === i ? null : i)}>{s.map ? `${mapLabel(meta, s.map)} (${s.map})` : 'Choose map…'}</button>
            )}
            {s.kind === 'grind' && (
              <>
                <label className="mini">until level<input type="number" value={s.until_level ?? ''} onChange={e => updStep(i, { until_level: e.target.value ? Number(e.target.value) : null })} /></label>
                <label className="mini">minutes<input type="number" value={s.minutes ?? ''} onChange={e => updStep(i, { minutes: e.target.value ? Number(e.target.value) : null })} /></label>
                <label className="mini">kills<input type="number" value={s.kills ?? ''} onChange={e => updStep(i, { kills: e.target.value ? Number(e.target.value) : null })} /></label>
              </>
            )}
            {s.kind === 'shop' && (
              <>
                {SELL_TABS.map(([t, label]) => (
                  <label className="mini" key={t}>sell {label}<input type="checkbox" checked={(s.sell || []).includes(t)} onChange={e => updStep(i, { sell: e.target.checked ? [...(s.sell || []), t] : (s.sell || []).filter(x => x !== t) })} /></label>
                ))}
                <label className="mini">keep top rows<input type="number" min="0" max="5" value={s.keep ?? 0} onChange={e => updStep(i, { keep: Number(e.target.value) })} /></label>
                <label className="mini">keep item ids<input type="text" placeholder="2000000, 2000001" value={(s.keep_items || []).join(', ')} onChange={e => updStep(i, { keep_items: parseIds(e.target.value) })} /></label>
                <KeepPreview keep={s.keep} keepItems={s.keep_items} />
                <label className="mini">buy row 1 ×<input type="number" min="0" value={(s.buy && s.buy[0] && s.buy[0].qty) || 0} onChange={e => updStep(i, { buy: Number(e.target.value) > 0 ? [{ slot: 0, qty: Number(e.target.value) }] : [] })} /></label>
              </>
            )}
            {s.kind === 'rest' && <label className="mini">minutes<input type="number" value={s.minutes} onChange={e => updStep(i, { minutes: Number(e.target.value) })} /></label>}
            <button className="btn small" onClick={() => upd({ steps: plan.steps.filter((_, j) => j !== i) })} aria-label="Remove step">✕</button>
            {pick === i && s.kind === 'quest' && <div className="span-all"><QuestPicker level={level} value={s.quest} onChange={r => { updStep(i, { quest: r.id, name: r.name }); setPick(null) }} /></div>}
            {pick === i && s.kind !== 'quest' && <div className="span-all"><MapPicker meta={meta} value={s.map} onChange={id => { updStep(i, { map: id }); setPick(null) }} /></div>}
          </div>
        ))}
        <div className="controls">{STEP_KINDS.map(([k, label]) => <button key={k} className="btn small" onClick={() => upd({ steps: [...plan.steps, newStep(k)] })}>+ {label}</button>)}</div>

        <h3>Time windows <small>(empty = always on; times are your local time)</small></h3>
        {plan.windows.map((w, i) => (
          <div className="step" key={i}>
            <input type="time" value={w.start} onChange={e => upd({ windows: plan.windows.map((x, j) => (j === i ? { ...x, start: e.target.value } : x)) })} aria-label="Start" />
            <span>to</span>
            <input type="time" value={w.end} onChange={e => upd({ windows: plan.windows.map((x, j) => (j === i ? { ...x, end: e.target.value } : x)) })} aria-label="End" />
            <span className="days">{DAYS.map((d, di) => (
              <button key={d} className={`chip small ${w.days.includes(di) ? 'on' : ''}`} aria-pressed={w.days.includes(di)}
                onClick={() => upd({ windows: plan.windows.map((x, j) => (j === i ? { ...x, days: x.days.includes(di) ? x.days.filter(v => v !== di) : [...x.days, di] } : x)) })}>{d}</button>
            ))}</span>
            <button className="btn small" onClick={() => upd({ windows: plan.windows.filter((_, j) => j !== i) })} aria-label="Remove window">✕</button>
          </div>
        ))}
        <button className="btn small" onClick={() => upd({ windows: [...plan.windows, { days: [], start: '18:00', end: '23:00' }] })}>+ Add window</button>

        <div className="controls">
          <button className="btn primary" disabled={!plan.steps.length} onClick={() => send({ ...plan, utc_offset_min: tzOffset() })}>Run this plan</button>
          <button className="btn" onClick={() => send({ name: '', steps: [], repeat: false, windows: [], utc_offset_min: 0 })}>Clear the running plan</button>
        </div>

        <h3>Share</h3>
        <div className="share">
          <button className="btn small" disabled={!plan.steps.length} onClick={() => { setCode(encode(plan)); navigator.clipboard?.writeText(encode(plan)).catch(() => {}) }}>Make a share code</button>
          <textarea rows={3} placeholder="NEON1:… (paste a code and press Import)" value={code} onChange={e => setCode(e.target.value)} aria-label="Share code" />
          <button className="btn small" disabled={!code.trim()} onClick={importCode}>Import</button>
        </div>
      </Card>
    </main>
  )
}
