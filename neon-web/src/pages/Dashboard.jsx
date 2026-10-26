import { useCallback, useMemo, useState } from 'react'
import { control, sendJson } from '../api.js'
import { Bar, Card, Empty, clock, fmt, mapLabel, minutes, mobLabel } from '../ui.jsx'
import Icon from '../icons.jsx'
import LineChart, { Sparkline } from '../LineChart.jsx'
import MapView from '../MapView.jsx'
import GameStream from '../GameStream.jsx'
import { EVENT_KINDS, eventKind, potionTrouble } from '../track.js'

const store = {
  get: (k, d) => { try { return localStorage.getItem(k) ?? d } catch { return d } },
  set: (k, v) => { try { localStorage.setItem(k, v) } catch { /* private mode */ } },
}
const short = ms => {
  if (ms == null) return '–'
  const sec = Math.max(0, Math.round(ms / 1000))
  if (sec < 60) return `${sec}s`
  if (sec < 3600) return `${Math.floor(sec / 60)}m ${String(sec % 60).padStart(2, '0')}s`
  return `${Math.floor(sec / 3600)}h ${String(Math.floor((sec % 3600) / 60)).padStart(2, '0')}m`
}
const etaText = m => (m == null ? '–' : m < 1 ? '< 1 min' : minutes(Math.round(m)))
const humanize = (meta, text) => (text || '')
  .replace(/(\d+)x mob (\d+)/g, (_, n, id) => `${n}× ${mobLabel(meta, Number(id))}`)
  .replace(/mob (\d+)/g, (_, id) => mobLabel(meta, Number(id)))
  .replace(/map (\d{6,})/g, (_, id) => mapLabel(meta, Number(id)))

function ControlBar({ session, data, stale }) {
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState(null)
  const alive = data && data.running && !stale
  const paused = data?.mode === 'paused' && /control panel/.test(data.pause_reason || '')
  const act = async fn => {
    setBusy(true); setErr(null)
    try { await fn() } catch (e) { setErr(e.message) }
    setBusy(false)
  }
  return (
    <div className="controls" role="group" aria-label="Bot controls">
      {!alive && <button className="btn primary" disabled={busy} onClick={() => act(() => sendJson('/api/session/start', { session }))}><Icon name="play" size={14} />Start bot</button>}
      {alive && !paused && <button className="btn" disabled={busy} onClick={() => act(() => control(session, { paused: true }))}><Icon name="pause" size={14} />Pause</button>}
      {alive && paused && <button className="btn primary" disabled={busy} onClick={() => act(() => control(session, { paused: false }))}><Icon name="play" size={14} />Resume</button>}
      {alive && <button className="btn danger" disabled={busy} onClick={() => act(() => sendJson('/api/session/stop', { session }))}><Icon name="stop" size={13} />Stop</button>}
      {err && <span className="error">{err}</span>}
    </div>
  )
}

function Kpi({ icon, label, value, unit, hint, tone = '', spark, sparkColor }) {
  return (
    <div className={`kpi ${tone ? `t-${tone}` : ''}`}>
      <div className="kpi-head"><span className="kpi-ico"><Icon name={icon} size={15} /></span>{label}</div>
      <div className="kpi-value">{value}{unit && <small>{unit}</small>}</div>
      <div className="kpi-hint">{hint || ' '}</div>
      {spark !== undefined && <Sparkline points={spark} color={sparkColor || 'accent'} />}
    </div>
  )
}

function LevelFix({ track, correctLevel, onDone }) {
  const [v, setV] = useState(track.level ?? '')
  return (
    <div className="level-fix">
      Actual level:
      <input type="number" min="1" max="200" value={v} onChange={e => setV(e.target.value === '' ? '' : Number(e.target.value))} aria-label="Actual level" autoFocus />
      <button className="btn small primary" disabled={!v} onClick={() => { correctLevel(Number(v)); onDone() }}>Save</button>
      {track.fix !== 0 && <button className="btn small" onClick={() => { correctLevel(null); onDone() }}>Use derived ({track.derivedLevel})</button>}
      <button className="btn small ghost" onClick={onDone}>Cancel</button>
    </div>
  )
}

function Hero({ data, meta, track, correctLevel }) {
  const [editing, setEditing] = useState(false)
  const p = data.state.player
  const hpPct = p.max_hp ? (100 * p.hp) / p.max_hp : null
  return (
    <Card className="c8 hero-card">
      <div className="hero">
        <div className="avatar" style={{ '--p': track?.pct ?? 0 }} title={track?.pct != null ? `${track.pct.toFixed(2)}% to level ${track.level + 1}` : undefined}>
          <div><small>Level</small><b>{track?.level ?? '–'}</b></div>
          <button className="btn small lv-edit" onClick={() => setEditing(e => !e)} aria-label="Correct the level" title="Correct the level"><Icon name="edit" size={13} /></button>
        </div>
        <div className="hero-body">
          <div className="hero-top">
            <h3><Icon name="map" size={18} />{mapLabel(meta, p.map_id)}</h3>
            <span className="hint num">map {p.map_id ?? '–'}</span>
          </div>
          <div className="hero-meta">
            <span className="num"><Icon name="target" size={13} />({fmt(p.x)}, {fmt(p.y)})</span>
            <span>{p.dead ? 'dead' : p.grounded === false ? 'airborne / on a rope' : 'on the ground'}</span>
            {track?.levelApprox && <span className="tag muted" title="The level read from memory does not update after a level-up; the level shown is worked out from the EXP history.">memory says Lv {track.memLevel ?? '–'}</span>}
            {track?.fix !== 0 && track?.fix != null && <span className="tag warn">corrected by you ({track.fix > 0 ? '+' : ''}{track.fix})</span>}
          </div>
          {editing && <LevelFix track={track} correctLevel={correctLevel} onDone={() => setEditing(false)} />}
          {track?.exp != null
            ? <Bar label="EXP" value={track.exp} max={track.need} tone="exp" big />
            : <div className="bar-row"><div className="bar-label"><span>EXP</span><span>waiting for the first EXP packet…</span></div><div className="bar big" /></div>}
          <Bar label="HP" value={p.hp} max={p.max_hp} tone="hp" low={30} />
          <Bar label="MP" value={p.mp} max={p.max_mp} tone="mp" />
          <div className="eta">
            <span><b className="num">{fmt(track?.toNext)}</b> EXP to Lv {(track?.level ?? 0) + 1}</span>
            <span>next level in <b>{etaText(track?.etaNextMin)}</b></span>
            {track?.target && <span>Lv {track.target} in <b>{etaText(track.etaTargetMin)}</b></span>}
            {hpPct != null && hpPct < 30 && <span style={{ color: 'var(--bad)' }}><b style={{ color: 'inherit' }}>Low HP</b></span>}
          </div>
        </div>
      </div>
    </Card>
  )
}

function PlanCard({ data, meta, track }) {
  const plan = data.plan
  const b = data.stats
  return (
    <Card className="c4" title="Session" icon="clock" right={<span className="num">started {clock(data.started)}</span>}>
      <div className="stats" style={{ marginBottom: 14 }}>
        <div><div className="stat-value">{short(track?.runMs)}</div><div className="stat-label">running</div></div>
        <div><div className="stat-value">{fmt(Math.round(track?.expGained ?? 0))}</div><div className="stat-label">EXP this run{track?.levelUps?.filter(u => u.t >= data.started).length ? ` · +${track.levelUps.filter(u => u.t >= data.started).length} lv` : ''}</div></div>
      </div>
      {plan ? (
        <>
          <div className="sub-h" style={{ marginTop: 0 }}>Plan</div>
          <div className="plan-name">{plan.name || 'Unnamed plan'}</div>
          <div className="steps-track" aria-label={`Step ${plan.step} of ${plan.total}`}>
            {Array.from({ length: plan.total || 0 }, (_, i) => <i key={i} className={i + 1 < plan.step ? 'done' : i + 1 === plan.step ? 'cur' : ''} />)}
          </div>
          <div className="plan-what"><span className="tag">Step {plan.step}/{plan.total}</span> {humanize(meta, plan.what)}</div>
        </>
      ) : <Empty>No plan: grinding with the current settings. Build one in Maps &amp; Schedule.</Empty>}
      <div className="sub-h">Run</div>
      <dl className="kv">
        <dt>Attacks</dt><dd>{fmt(b.attacks)}</dd>
        <dt>Loot presses</dt><dd>{fmt(b.loot_presses)}</dd>
        <dt>AP spent</dt><dd>{fmt(b.ap_spent)}</dd>
        <dt>EXP per kill</dt><dd>{track?.expPerKill != null ? track.expPerKill.toFixed(1) : '–'}</dd>
      </dl>
    </Card>
  )
}

function Nearby({ st, meta, wanted }) {
  const me = st.player
  const counts = {}
  st.mobs.forEach(m => { const k = m.id ?? 0; counts[k] = (counts[k] || 0) + 1 })
  const rows = Object.entries(counts).map(([id, n]) => [Number(id), n]).sort((a, c) => c[1] - a[1])
  const players = [...st.others].map(o => ({ ...o, d: Math.round(Math.hypot(o.x - me.x, o.y - me.y)) })).sort((a, c) => a.d - c.d)
  return (
    <Card className="c4" title="Around you" icon="users">
      <ul className="around">
        <li><b>{st.mobs.length}</b>mobs</li>
        <li><b>{st.others.length}</b>players</li>
        <li><b>{st.drops.length}</b>drops</li>
      </ul>
      {rows.length > 0 && <>
        <div className="sub-h">Mobs on the map</div>
        <ul className="mobs">{rows.map(([id, n]) => (
          <li key={id}><span>{mobLabel(meta, id)} {!id ? null : wanted?.length && !wanted.includes(id) ? <span className="tag muted">ignored</span> : wanted?.length ? <span className="tag">target</span> : null}</span><b className="num">{n}</b></li>
        ))}</ul>
      </>}
      {players.length > 0 && <>
        <div className="sub-h">Players</div>
        <ul className="mobs scroll">{players.map((o, i) => <li key={i}><span className={o.name ? '' : 'muted'}>{o.name || 'name not seen yet'}</span><span className="muted num">{fmt(o.d)} px</span></li>)}</ul>
      </>}
    </Card>
  )
}

function Charts({ data, track }) {
  const [tab, setTab] = useState(() => store.get('neon.chart', 'exp'))
  const pick = t => { setTab(t); store.set('neon.chart', t) }
  const p = data.state?.player || {}
  const tabs = [['exp', 'EXP'], ['kills', 'Kills'], ['hp', 'HP']]
  return (
    <Card className="c8" title="This run" icon="pulse" right={
      <span className="tabs" role="tablist">{tabs.map(([k, l]) => <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? 'on' : ''} onClick={() => pick(k)}>{l}</button>)}</span>
    }>
      {tab === 'exp' && <>
        <div className="chart-sum">
          <span><b>{fmt(Math.round(track.expGained))}</b> EXP gained</span>
          <span><b>{fmt(Math.round(track.expPerHour))}</b>/h last 10 min</span>
          <span><b>{fmt(Math.round(track.expPerHourAvg))}</b>/h run average</span>
        </div>
        <LineChart points={track.expSeries} label="EXP gained this run" zero />
      </>}
      {tab === 'kills' && <>
        <div className="chart-sum">
          <span><b>{fmt(track.kills)}</b> kills</span>
          <span><b>{track.killsPerHourNow != null ? fmt(Math.round(track.killsPerHourNow)) : '–'}</b>/h last 10 min</span>
          <span><b>{fmt(Math.round(track.killsPerHour))}</b>/h run average</span>
        </div>
        <LineChart points={track.killSeries} label="Kills this run" color="good" step zero />
      </>}
      {tab === 'hp' && <>
        <div className="chart-sum">
          <span><b>{fmt(p.hp)}</b> / {fmt(p.max_hp)} HP now</span>
          <span><b>{fmt(data.stats.hp_pots)}</b> HP potions</span>
          <span>retreat below <b>{data.config?.flee_hp_below ?? '–'}%</b></span>
        </div>
        <LineChart points={track.hpSeries} label="HP" color="hp" refY={p.max_hp} refLabel="max" zero />
      </>}
    </Card>
  )
}

function KillsByMob({ data, meta, track }) {
  const rows = Object.entries(data.stats.kills_by_mob || {}).map(([id, n]) => [Number(id), n]).sort((a, c) => c[1] - a[1])
  const total = rows.reduce((a, [, n]) => a + n, 0)
  const top = rows[0]?.[1] || 1
  const hours = (track?.runMs || 0) / 3600000
  return (
    <Card className="c4" title="Kills by mob" icon="sword" right={<span className="num">{fmt(total)} total</span>}>
      {rows.length === 0 ? <Empty>No kills yet</Empty> : (
        <ul className="hbars">{rows.map(([id, n]) => (
          <li key={id} title={`${mobLabel(meta, id)}: ${n} kills (${Math.round((100 * n) / total)}%)`}>
            <span className="hb-name">{mobLabel(meta, id)}</span>
            <span className="hb-val"><b>{fmt(n)}</b>{hours > 0.02 ? `${Math.round(n / hours)}/h` : `${Math.round((100 * n) / total)}%`}</span>
            <span className="hb-track"><span className="hb-fill" style={{ width: `${(100 * n) / top}%`, display: 'block' }} /></span>
          </li>
        ))}</ul>
      )}
    </Card>
  )
}

const KIND_ICON = { kill: 'sword', potion: 'flask', loot: 'bag', travel: 'route', plan: 'list', alert: 'alert', other: 'dot' }

function Activity({ events }) {
  const [filter, setFilter] = useState(() => store.get('neon.feed', 'all'))
  const [q, setQ] = useState('')
  const pick = f => { setFilter(f); store.set('neon.feed', f) }
  const tagged = useMemo(() => events.map(e => ({ ...e, kind: eventKind(e.msg) })), [events])
  const counts = {}
  tagged.forEach(e => { counts[e.kind] = (counts[e.kind] || 0) + 1 })
  // Newest first, and runs of the same message collapse into one row with a count.
  const rows = []
  for (const e of [...tagged].reverse()) {
    if (filter !== 'all' && e.kind !== filter) continue
    if (q && !e.msg.toLowerCase().includes(q.toLowerCase())) continue
    const last = rows[rows.length - 1]
    if (last && last.msg === e.msg) { last.n += 1; last.first = e.t; continue }
    rows.push({ ...e, n: 1, first: e.t })
  }
  return (
    <Card className="c12" title="Activity" icon="list" right={<span className="num">last {events.length} events</span>}>
      <div className="feed-tools">
        <div className="chips">
          <button className={`chip small ${filter === 'all' ? 'on' : ''}`} onClick={() => pick('all')}>All<small>{events.length}</small></button>
          {EVENT_KINDS.map(([k, label]) => counts[k] ? <button key={k} className={`chip small ${filter === k ? 'on' : ''}`} onClick={() => pick(k)}>{label}<small>{counts[k]}</small></button> : null)}
        </div>
        <input type="search" placeholder="Search events" value={q} onChange={e => setQ(e.target.value)} aria-label="Search events" />
      </div>
      <ul className="events">
        {rows.map((e, i) => (
          <li key={`${e.t}-${i}`}>
            <time>{clock(e.t)}</time>
            <span className={`ev-ico ev-${e.kind}`}><Icon name={KIND_ICON[e.kind]} size={13} /></span>
            <span className="msg">{e.msg}</span>
            {e.n > 1 && <span className="rep" title={`${e.n} times since ${clock(e.first)}`}>×{e.n}</span>}
          </li>
        ))}
        {rows.length === 0 && <li className="empty">Nothing here yet</li>}
      </ul>
    </Card>
  )
}

function Health({ data, track }) {
  const b = data.stats
  const caps = data.state?.caps || {}
  const missing = Object.entries(caps).filter(([, v]) => !v).map(([k]) => k)
  const fpsTone = data.fps >= 40 ? 'var(--good)' : data.fps >= 20 ? 'var(--warn)' : 'var(--bad)'
  return (
    <Card className="c4" title="Level-ups & pipeline" icon="wifi">
      {track?.levelUps?.length ? (
        <ul className="ups">{[...track.levelUps].reverse().slice(0, 5).map(u => <li key={u.t}><span>Reached <b>Lv {u.level + (track.fix || 0)}</b></span><span className="hint num">{new Date(u.t).toLocaleString([], { hour12: false, month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}</span></li>)}</ul>
      ) : <Empty>No level-up seen in the recorded history yet.</Empty>}
      <div className="sub-h">Pipeline</div>
      <dl className="kv">
        <dt>Stream</dt><dd><span style={{ color: fpsTone }}>●</span> {Math.round(data.fps)} frames/s</dd>
        <dt>Latency</dt><dd>{data.latency_ms?.toFixed(1)} ms</dd>
        <dt>Bad position reads</dt><dd>{fmt(b.bad_positions)}</dd>
        <dt>Map changes</dt><dd>{fmt(b.map_changes)}</dd>
        <dt>Drops gone</dt><dd>{fmt(b.drops_gone)}</dd>
        <dt>Mode</dt><dd>{data.mode}{data.dry_run ? ' (dry-run)' : ''}</dd>
        {missing.length > 0 && <><dt>Not available</dt><dd title={missing.join(', ')}>{missing.join(', ')}</dd></>}
      </dl>
      {data.sim?.summary && <p className="hint" style={{ marginTop: 10 }} title={data.sim.summary}>{data.sim.summary.replace(/^sim: /, 'Movement sim: ')}</p>}
    </Card>
  )
}

export default function Dashboard({ session, live, meta, track, samples, correctLevel }) {
  const { data, error, stale } = live
  const [mapMode, setMapMode] = useState(() => store.get('neon.mapmode', 'fit'))
  const wanted = data?.config?.allowed_mobs || null
  const mobName = useCallback(id => mobLabel(meta, id), [meta])
  if (!data) {
    return (
      <>
        <ControlBar session={session} data={null} stale={false} />
        <Empty>{error || 'Loading…'}</Empty>
      </>
    )
  }
  const st = data.state, p = st?.player, b = data.stats
  const pots = potionTrouble(data, samples || [])
  const hours = (track?.runMs || 0) / 3600000
  const exp = track?.expSeries, kills = track?.killSeries
  const setMode = m => { setMapMode(m); store.set('neon.mapmode', m) }

  return (
    <>
      <ControlBar session={session} data={data} stale={stale} />
      {data.mode === 'paused' && data.pause_reason && /lie detector/i.test(data.pause_reason) && (
        <div className="notice alarm"><Icon name="alert" />Lie detector active. The bot has stopped touching the game: answer it yourself.</div>
      )}
      {pots && (
        <div className="notice alarm"><Icon name="flask" /><span><b>HP potions are not healing.</b> {pots.presses} presses in the last 30 s while HP stayed at {fmt(pots.hp)}/{fmt(pots.max_hp)} ({pots.pct}%). Out of potions, or the potion key ({data.config?.keys?.hp_pot || '?'}) is not bound?</span></div>
      )}
      {stale && <div className="notice warn"><Icon name="alert" />No status update for {Math.round(live.age / 1000)} s. The bot may have stopped or crashed.</div>}
      {data.config?.home_map && p?.map_id !== data.config.home_map && data.mode !== 'paused' && (
        <div className="notice"><Icon name="route" />Travelling to {mapLabel(meta, data.config.home_map)} (map {data.config.home_map})…</div>
      )}
      <main className="dash">
        {p && <Hero data={data} meta={meta} track={track} correctLevel={correctLevel} />}
        <PlanCard data={data} meta={meta} track={track} />

        <div className="kpis">
          <Kpi icon="bolt" label="EXP / hour" value={fmt(Math.round(track?.expPerHour || 0))} hint={`avg ${fmt(Math.round(track?.expPerHourAvg || 0))}/h this run`} spark={exp} />
          <Kpi icon="sword" label="Kills / hour" tone="good" value={fmt(Math.round(track?.killsPerHour || 0))} hint={`${fmt(b.mobs_killed)} kills · last ${track?.lastKillAgoMs != null ? `${short(track.lastKillAgoMs)} ago` : '–'}`} spark={kills} sparkColor="good" />
          <Kpi icon="trophy" label="Next level" tone="info" value={etaText(track?.etaNextMin)} hint={track?.pct != null ? `${track.pct.toFixed(1)}% of Lv ${track.level}` : 'needs EXP data'} />
          <Kpi icon="clock" label="Session" value={short(track?.runMs)} hint={`since ${clock(data.started)}`} />
          <Kpi icon="flask" label="Potions" tone="hp" value={fmt(b.hp_pots ?? 0)} unit={b.mp_pots ? ` / ${fmt(b.mp_pots)}` : ' HP'} hint={hours > 0.02 ? `${Math.round((b.hp_pots || 0) / hours)}/h${b.mp_pots ? ` · ${b.mp_pots} MP` : ''}` : 'HP / MP'} />
          <Kpi icon="skull" label="Deaths" tone={b.deaths ? 'warn' : ''} value={fmt(b.deaths ?? 0)} hint={b.revives ? `${b.revives} revives` : data.config?.max_deaths ? `stops after ${data.config.max_deaths}` : 'no limit set'} />
        </div>

        <Card className="c8" title="Live game" icon="video" right={<a href="#/live" className="hint">open large view</a>}>
          <GameStream session={session} />
        </Card>
        {st && <Nearby st={st} meta={meta} wanted={wanted} />}

        {st && (
          <Card className="c8" title="Map" icon="map" right={
            <span className="tabs">{[['fit', 'Whole map'], ['follow', 'Follow']].map(([k, l]) => <button key={k} className={mapMode === k ? 'on' : ''} onClick={() => setMode(k)}>{l}</button>)}</span>
          }>
            <MapView state={st} geometry={data.geometry} sim={data.sim} mobName={mobName} wanted={wanted} mode={mapMode} />
            <div className="map-foot">
              <span className="legend"><span><i className="me" />you</span><span><i className="mob" />target</span>{wanted?.length ? <span><i className="mobd" />other mob</span> : null}<span><i className="oth" />player</span><span><i className="drp" />drop</span><span><i className="lad" />rope</span>{data.sim && <span><i className="sim" />simulated</span>}</span>
              <span className="hint">{data.geometry?.platforms?.length ?? 0} platforms · {data.geometry?.ladders?.length ?? 0} ropes</span>
            </div>
          </Card>
        )}
        <KillsByMob data={data} meta={meta} track={track} />

        {track && <Charts data={data} track={track} />}
        <Health data={data} track={track} />

        <Activity events={data.events} />
      </main>
    </>
  )
}
