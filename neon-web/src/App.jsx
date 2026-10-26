import { useEffect, useMemo, useState } from 'react'
import { useLive, useMeta, useSessions } from './useLive.js'
import { deriveTrack, getLevelFix, setLevelFix, useHistory, useSamples } from './track.js'
import { Pill, fmt } from './ui.jsx'
import Icon from './icons.jsx'
import Dashboard from './pages/Dashboard.jsx'
import Settings from './pages/Settings.jsx'
import Plan from './pages/Plan.jsx'
import Rules from './pages/Rules.jsx'
import History from './pages/History.jsx'
import Profiles from './pages/Profiles.jsx'
import Live from './pages/Live.jsx'
import Fleet from './pages/Fleet.jsx'
import Library from './pages/Library.jsx'

const PAGES = [
  ['dashboard', 'Dashboard', Dashboard, 'dashboard', 'Live view of the running bot'],
  ['live', 'Live game', Live, 'video', 'Screen view of the game window (read-only)'],
  ['fleet', 'All bots', Fleet, 'users', 'Every running bot and its game window at a glance'],
  ['settings', 'Settings', Settings, 'settings', 'Applied to the running bot as you save'],
  ['plan', 'Maps & Schedule', Plan, 'plan', 'Map rusher and task scheduler'],
  ['rules', 'Rules', Rules, 'rules', '“When this happens, do that”'],
  ['history', 'History', History, 'history', 'Recorded every 30 seconds while the bot runs'],
  ['profiles', 'Profiles', Profiles, 'profiles', 'Saved settings and plans, share codes'],
  ['library', 'Library', Library, 'profiles', 'Plans and profiles people shared, kept by name'],
]

const parseHash = () => (window.location.hash.replace(/^#\/?/, '').split('?')[0] || 'dashboard')

function usePage() {
  const [page, setPage] = useState(parseHash)
  useEffect(() => {
    const f = () => setPage(parseHash())
    window.addEventListener('hashchange', f)
    return () => window.removeEventListener('hashchange', f)
  }, [])
  return page
}

function StatusPill({ data, error, stale, busy }) {
  if (!data) return <Pill tone="bad">{error || 'Offline'}</Pill>
  if (stale) return <Pill tone="bad">No updates: bot stopped?</Pill>
  if (busy) return <Pill tone="warn">Running: busy (idle break or talking to an NPC)</Pill>
  if (!data.running) return <Pill tone="muted">Finished</Pill>
  if (data.mode === 'paused') return <Pill tone="warn">Paused{data.pause_reason ? `: ${data.pause_reason}` : ''}</Pill>
  return <Pill tone="good">{data.dry_run ? 'Running (dry-run)' : 'Running'}</Pill>
}

function Sessions({ sessions, session, setSession }) {
  const list = [...sessions].sort((a, b) => Number(b.running) - Number(a.running) || b.updated - a.updated).slice(0, 12)
  if (!list.some(s => s.name === session)) list.unshift({ name: session })
  return (
    <div className="sess-list" role="listbox" aria-label="Sessions">
      {list.map(s => (
        <button key={s.name} role="option" aria-selected={s.name === session} className={`sess ${s.name === session ? 'on' : ''}`} onClick={() => setSession(s.name)}
          title={s.running ? `${s.name}: running, ${fmt(s.kills)} kills` : `${s.name}: idle`}>
          <i className={`dot ${s.running ? 'live' : ''}`} />
          <span>{s.name}</span>
          {s.running && <small>{fmt(s.kills)} kills</small>}
        </button>
      ))}
    </div>
  )
}

export default function App() {
  const page = usePage()
  const [session, setSession] = useState(() => { try { return localStorage.getItem('neon.session') || 'main' } catch { return 'main' } })
  const sessions = useSessions()
  const meta = useMeta()
  const live = useLive(session)
  const rows = useHistory(session)
  const samples = useSamples(session, live.data)
  const [fix, setFix] = useState(() => getLevelFix(session))

  useEffect(() => { try { localStorage.setItem('neon.session', session) } catch { /* private mode */ } setFix(getLevelFix(session)) }, [session])
  // Follow the bot: when the selected session is idle and another one is running, show that one.
  useEffect(() => {
    if (sessions.some(s => s.name === session && s.running)) return
    const running = sessions.filter(s => s.running).sort((a, b) => b.updated - a.updated)[0]
    if (running) setSession(running.name)
  }, [sessions, session])

  const track = useMemo(
    () => deriveTrack({ rows, samples, data: live.data, expToNext: meta?.expToNext, fix }),
    // samples is mutated in place; live.data changes with every poll, which is when samples grow.
    [rows, samples, live.data, meta, fix],
  )
  const correctLevel = actual => {
    const n = actual == null ? 0 : actual - (track?.derivedLevel ?? actual)
    setLevelFix(session, n); setFix(n)
  }

  const cur = PAGES.find(p => p[0] === page) || PAGES[0]
  const Page = cur[2]
  const running = sessions.filter(s => s.running).length

  return (
    <div className="shell">
      <aside className="side">
        <a className="brand" href="#/dashboard">
          <span className="brand-mark">N</span>
          <span><b>Neon</b><small>MapleStory Classic</small></span>
        </a>
        <nav className="nav" aria-label="Pages">
          {PAGES.map(([id, label, , icon]) => (
            <a key={id} href={`#/${id}`} className={page === id ? 'active' : ''} aria-current={page === id ? 'page' : undefined}>
              <Icon name={icon} /><span>{label}</span>
            </a>
          ))}
        </nav>
        <div className="side-foot">
          <div className="sub-h" style={{ padding: '0 8px' }}>Sessions {running > 0 && <span className="tag" style={{ marginLeft: 6 }}>{running} live</span>}</div>
          <Sessions sessions={sessions} session={session} setSession={setSession} />
          <p className="side-note">Panic key: <b>Pause</b> stops the bot from anywhere.</p>
        </div>
      </aside>
      <div className="main">
        <div className="mobile-top">
          <select value={session} onChange={e => setSession(e.target.value)} aria-label="Session">
            {[...new Set(['main', ...sessions.map(s => s.name), session])].map(n => <option key={n} value={n}>{n}{sessions.find(s => s.name === n)?.running ? ' (live)' : ''}</option>)}
          </select>
        </div>
        <header className="page-head">
          <div>
            <h1>{cur[1]}</h1>
            <div className="sub">{cur[4]} · session <b>{session}</b></div>
          </div>
          <div className="head-right"><StatusPill {...live} /></div>
        </header>
        <Page session={session} sessions={sessions} setSession={setSession} live={live} meta={meta} track={track} rows={rows} samples={samples} correctLevel={correctLevel} />
      </div>
    </div>
  )
}
