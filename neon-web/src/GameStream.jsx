import { useEffect, useRef, useState } from 'react'
import { getJson } from './api.js'
import Icon from './icons.jsx'

// Live view of the game window (MJPEG from /api/stream). Read-only: the server only grabs the screen.
// Paused means no connection, so the server stops capturing when nobody watches. Hidden tabs pause too.
const store = {
  get: (k, d) => { try { return localStorage.getItem(k) ?? d } catch { return d } },
  set: (k, v) => { try { localStorage.setItem(k, v) } catch { /* private mode */ } },
}
const FPS = [2, 4, 8]
const SCALES = [[50, 'Low'], [75, 'Medium'], [100, 'Full']]

function useVisible() {
  const [v, setV] = useState(!document.hidden)
  useEffect(() => {
    const f = () => setV(!document.hidden)
    document.addEventListener('visibilitychange', f)
    return () => document.removeEventListener('visibilitychange', f)
  }, [])
  return v
}

export default function GameStream({ big = false, session = '' }) {
  const q = session ? `&session=${encodeURIComponent(session)}` : ''
  const phone = typeof window !== 'undefined' && window.innerWidth < 760
  const [playing, setPlaying] = useState(() => store.get('neon.live.play', phone ? '0' : '1') === '1')
  const [fps, setFps] = useState(() => Number(store.get('neon.live.fps', phone ? '2' : '4')))
  const [scale, setScale] = useState(() => Number(store.get('neon.live.scale', phone ? '50' : '75')))
  const [key, setKey] = useState(0)
  const [err, setErr] = useState(null)
  const [loaded, setLoaded] = useState(false)
  const [st, setSt] = useState(null)
  const visible = useVisible()
  const box = useRef(null)
  const on = playing && visible

  const upd = (setter, k) => v => { setter(v); store.set(k, String(v)); setLoaded(false) }
  const toggle = () => { const n = !playing; setPlaying(n); store.set('neon.live.play', n ? '1' : '0'); setErr(null); setLoaded(false) }

  useEffect(() => {
    if (!on) { setSt(null); return }
    let alive = true
    const load = () => getJson(`/api/stream/status?x=1${q}`).then(s => alive && setSt(s)).catch(() => {})
    const id = setInterval(load, 2000)
    load()
    return () => { alive = false; clearInterval(id) }
  }, [on, q])

  // Retry a dropped stream (dev server reload, network blip) after a short wait.
  useEffect(() => {
    if (!err || !on) return
    const id = setTimeout(() => { setErr(null); setKey(k => k + 1) }, 3000)
    return () => clearTimeout(id)
  }, [err, on])

  const full = () => {
    const el = box.current
    if (!el) return
    if (document.fullscreenElement) document.exitFullscreen?.()
    else (el.requestFullscreen || el.webkitRequestFullscreen)?.call(el)
  }

  // Chrome does not always fire load for multipart images, so the server's frame clock counts too.
  const showing = loaded || (st?.capturing && st.last_frame_ms != null && st.last_frame_ms < 3000 && !st.error)
  const src = on && !err ? `/api/stream?fps=${fps}&scale=${scale}&k=${key}${q}` : null
  return (
    <div className={`live ${big ? 'big' : ''}`}>
      <div className="live-box" ref={box} onDoubleClick={full}>
        {src && <img src={src} alt="Live view of the game window" onLoad={() => setLoaded(true)} onError={() => { setErr('Stream interrupted, reconnecting…'); setLoaded(false) }} />}
        {on && !showing && !err && <div className="live-overlay"><span className="spinner" />Connecting to the game window…</div>}
        {on && err && <div className="live-overlay"><Icon name="alert" size={22} />{err}</div>}
        {!on && (
          <button className="live-overlay play" onClick={toggle} aria-label="Start the live view">
            <span className="play-btn"><Icon name="play" size={26} /></span>
            <span>{playing ? 'Paused while this tab is hidden' : 'Live view paused: nothing is being captured'}</span>
          </button>
        )}
        {on && showing && <span className="live-badge"><i />LIVE</span>}
        {on && st?.error && <span className="live-warn">{st.error}</span>}
        <div className="live-bar">
          <button className="btn small" onClick={toggle} aria-label={playing ? 'Pause live view' : 'Play live view'}>
            <Icon name={playing ? 'pause' : 'play'} size={13} />{playing ? 'Pause' : 'Play'}
          </button>
          <span className="tabs" role="group" aria-label="Frames per second">
            {FPS.map(f => <button key={f} className={fps === f ? 'on' : ''} onClick={() => upd(setFps, 'neon.live.fps')(f)}>{f} fps</button>)}
          </span>
          <span className="tabs" role="group" aria-label="Quality">
            {SCALES.map(([s, l]) => <button key={s} className={scale === s ? 'on' : ''} onClick={() => upd(setScale, 'neon.live.scale')(s)} title={`${s}% of 1366×768`}>{l}</button>)}
          </span>
          <span className="live-stat num">{on && st ? (big ? `${st.fps || '…'} fps · ${st.viewers} viewer${st.viewers === 1 ? '' : 's'}${st.bytes ? ` · ${Math.round(st.bytes / 1024)} KB/frame` : ''}` : `${st.fps || '…'} fps · ${st.viewers} watching`) : ''}</span>
          <a className="btn small ghost" href={`/api/frame.jpg?scale=100${q}`} target="_blank" rel="noreferrer" title="Open a full-size screenshot"><Icon name="camera" size={14} /></a>
          <button className="btn small ghost" onClick={full} title="Fullscreen" aria-label="Fullscreen"><Icon name="expand" size={14} /></button>
        </div>
      </div>
    </div>
  )
}
