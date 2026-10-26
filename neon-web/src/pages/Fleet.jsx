import { useEffect, useState } from 'react'
import { useLive } from '../useLive.js'
import { Bar, Empty, Pill, fmt, mapLabel } from '../ui.jsx'
import Icon from '../icons.jsx'

// Every running bot at a glance: a small live view of its game window and the numbers that matter.
// The pictures are single frames refreshed every 1.5 s, not MJPEG streams: a browser holds only about six
// connections to one host, and one stream per bot would use them all up.
const FRAME_MS = 1500

function Frame({ session, onOpen }) {
  const [k, setK] = useState(0)
  const [ok, setOk] = useState(true)
  useEffect(() => {
    const id = setInterval(() => { if (!document.hidden) setK(x => x + 1) }, FRAME_MS)
    return () => clearInterval(id)
  }, [])
  return (
    <button className="fleet-frame" onClick={onOpen} title={`Open ${session} on the dashboard`}>
      <img src={`/api/frame.jpg?scale=50&session=${encodeURIComponent(session)}&k=${k}`} alt={`Game window of ${session}`}
        onLoad={() => setOk(true)} onError={() => setOk(false)} />
      {!ok && <span className="fleet-noframe"><Icon name="video" size={20} />No picture</span>}
    </button>
  )
}

function BotCard({ name, meta, onOpen }) {
  const { data, stale, busy } = useLive(name, 2000)
  const p = data?.state?.player || {}
  const need = p.level != null ? meta?.expToNext?.[String(p.level)] : null
  const plan = data?.plan
  const pill = !data ? <Pill tone="muted">Loading…</Pill>
    : stale ? <Pill tone="bad">Stopped</Pill>
    : busy ? <Pill tone="warn">Busy</Pill>
    : data.mode === 'paused' ? <Pill tone="warn">Paused{data.pause_reason ? `: ${data.pause_reason}` : ''}</Pill>
    : <Pill tone="good">Running</Pill>
  return (
    <section className="card fleet-card">
      <h2>
        <span className="h-title"><Icon name="users" size={16} />{name}</span>
        <span className="right">{pill}</span>
      </h2>
      <Frame session={name} onOpen={onOpen} />
      <div className="fleet-meta">
        <span><b>Lv {p.level ?? '–'}</b></span>
        <span><Icon name="map" size={13} />{mapLabel(meta, p.map_id)}</span>
        <span><Icon name="sword" size={13} />{fmt(data?.stats?.mobs_killed)} kills</span>
        <span><Icon name="bolt" size={13} />{fmt(Math.round(data?.progress?.exp_per_hour || 0))} EXP/h</span>
      </div>
      {need ? <Bar label="EXP" value={p.exp} max={need} tone="exp" /> : null}
      <Bar label="HP" value={p.hp} max={p.max_hp} tone="hp" low={30} />
      <Bar label="MP" value={p.mp} max={p.max_mp} tone="mp" />
      {plan && (
        <div className="fleet-plan">
          <div className="steps-track" aria-label={`Step ${plan.step} of ${plan.total}`}>
            {Array.from({ length: plan.total || 0 }, (_, i) => <i key={i} className={i + 1 < plan.step ? 'done' : i + 1 === plan.step ? 'cur' : ''} />)}
          </div>
          <small>Step {plan.step}/{plan.total}: {plan.what}</small>
        </div>
      )}
    </section>
  )
}

export default function Fleet({ sessions, meta, setSession }) {
  const live = sessions.filter(s => s.running).sort((a, b) => a.name.localeCompare(b.name))
  const open = name => { setSession(name); window.location.hash = '#/dashboard' }
  if (!live.length) return <main className="dash"><section className="card c12"><Empty>No bot is running.</Empty></section></main>
  return (
    <main className="fleet">
      {live.map(s => <BotCard key={s.name} name={s.name} meta={meta} onOpen={() => open(s.name)} />)}
    </main>
  )
}
