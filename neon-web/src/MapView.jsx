import { useEffect, useRef, useState } from 'react'

const css = name => getComputedStyle(document.documentElement).getPropertyValue(name).trim()
const ok = ([x, y]) => Number.isFinite(x) && Number.isFinite(y) && Math.abs(x) < 20000 && Math.abs(y) < 20000

// Top-down view of the map the bot sees: platforms, ropes, mobs (wanted ones bright, labelled by name), other
// players with names, drops, me, and the shadow simulation's idea of where I am (dashed ghost).
// "Follow" zooms to a window around the character; hovering shows what is under the pointer.
export default function MapView({ state, geometry, sim, mobName = id => `Mob #${id}`, wanted = null, mode = 'fit' }) {
  const ref = useRef(null)
  const hits = useRef([])
  const [size, setSize] = useState(0)
  const [tip, setTip] = useState(null)

  useEffect(() => {
    const c = ref.current
    if (!c) return
    const ro = new ResizeObserver(() => setSize(c.clientWidth * 10000 + c.clientHeight))
    ro.observe(c)
    return () => ro.disconnect()
  }, [])

  useEffect(() => {
    const c = ref.current
    if (!c || !state) return
    const dpr = window.devicePixelRatio || 1
    const w = c.clientWidth, h = c.clientHeight
    if (!w || !h) return
    c.width = w * dpr; c.height = h * dpr
    const g = c.getContext('2d')
    g.scale(dpr, dpr)
    g.clearRect(0, 0, w, h)

    const me = state.player
    const plats = geometry?.platforms ?? []
    const lads = geometry?.ladders ?? []
    let minX, maxX, minY, maxY
    if (mode === 'follow' && ok([me.x, me.y])) {
      const vw = 1100, vh = vw * (h / w)
      minX = me.x - vw / 2; maxX = me.x + vw / 2; minY = me.y - vh * 0.6; maxY = me.y + vh * 0.4
    } else {
      const pts = [[me.x, me.y]]
      state.mobs.forEach(m => pts.push([m.x, m.y]))
      state.others.forEach(o => pts.push([o.x, o.y]))
      state.drops.forEach(d => pts.push([d.x, d.y]))
      plats.forEach(([x1, y1, x2, y2]) => { pts.push([x1, y1]); pts.push([x2, y2]) })
      const good = pts.filter(ok)
      if (!good.length) return
      minX = Math.min(...good.map(p => p[0])); maxX = Math.max(...good.map(p => p[0]))
      minY = Math.min(...good.map(p => p[1])) - 40; maxY = Math.max(...good.map(p => p[1]))
    }
    const pad = 22
    const s = Math.min((w - pad * 2) / Math.max(maxX - minX, 300), (h - pad * 2) / Math.max(maxY - minY, 200))
    const ox = (w - (maxX - minX) * s) / 2, oy = (h - (maxY - minY) * s) / 2
    const X = x => ox + (x - minX) * s
    const Y = y => oy + (y - minY) * s
    const hitList = []

    // Faint grid every 200 world px.
    g.strokeStyle = css('--grid'); g.lineWidth = 1
    for (let gx = Math.ceil(minX / 200) * 200; gx < maxX + 400; gx += 200) { g.beginPath(); g.moveTo(X(gx), 0); g.lineTo(X(gx), h); g.stroke() }
    for (let gy = Math.ceil(minY / 200) * 200; gy < maxY + 400; gy += 200) { g.beginPath(); g.moveTo(0, Y(gy)); g.lineTo(w, Y(gy)); g.stroke() }

    // Platforms: a soft glow under a crisp line.
    g.lineCap = 'round'
    g.strokeStyle = css('--plat-glow'); g.lineWidth = 6
    plats.forEach(([x1, y1, x2, y2]) => { g.beginPath(); g.moveTo(X(x1), Y(y1)); g.lineTo(X(x2), Y(y2)); g.stroke() })
    g.strokeStyle = css('--plat'); g.lineWidth = 2
    plats.forEach(([x1, y1, x2, y2]) => { g.beginPath(); g.moveTo(X(x1), Y(y1)); g.lineTo(X(x2), Y(y2)); g.stroke() })
    g.lineWidth = 2; g.strokeStyle = css('--rope'); g.setLineDash([4, 3])
    lads.forEach(([x, y1, y2]) => { g.beginPath(); g.moveTo(X(x), Y(y1)); g.lineTo(X(x), Y(y2)); g.stroke() })
    g.setLineDash([])

    const font = (px, weight = 500) => { g.font = `${weight} ${px}px Inter, system-ui, sans-serif` }
    const label = (text, x, y, color) => {
      font(11, 600)
      const tw = g.measureText(text).width
      g.fillStyle = css('--map-label-bg'); g.beginPath(); g.roundRect(x - tw / 2 - 5, y - 13, tw + 10, 16, 5); g.fill()
      g.fillStyle = color; g.textAlign = 'center'; g.fillText(text, x, y - 1)
    }

    const bad = css('--bad'), dim = css('--mob-dim'), warn = css('--warn'), other = css('--other'), accent = css('--accent')
    state.drops.forEach(d => {
      const x = X(d.x), y = Y(d.y) - 5
      g.fillStyle = warn; g.beginPath(); g.moveTo(x, y - 5); g.lineTo(x + 5, y); g.lineTo(x, y + 5); g.lineTo(x - 5, y); g.closePath(); g.fill()
      hitList.push({ x, y, text: `Drop at (${d.x}, ${d.y})${d.is_meso ? ' · mesos' : ''}` })
    })
    // Mobs: wanted ones bright; label the nearest few of them.
    const dist = m => Math.hypot(m.x - me.x, m.y - me.y)
    const isWanted = m => !wanted || !wanted.length || wanted.includes(m.id)
    const near = [...state.mobs].filter(isWanted).sort((a, b) => dist(a) - dist(b)).slice(0, state.mobs.length > 14 ? 4 : 14)
    state.mobs.forEach(m => {
      const x = X(m.x), y = Y(m.y) - 7, want = isWanted(m)
      g.fillStyle = want ? bad : dim
      g.beginPath(); g.arc(x, y, want ? 5.5 : 4.5, 0, 7); g.fill()
      if (want) { g.strokeStyle = css('--map-bg'); g.lineWidth = 2; g.stroke() }
      hitList.push({ x, y, text: `${mobName(m.id)} (${m.x}, ${m.y})${want ? '' : ' · not a target'}` })
    })
    near.forEach(m => label(mobName(m.id), X(m.x), Y(m.y) - 15, css('--fg')))
    state.others.forEach(o => {
      const x = X(o.x), y = Y(o.y) - 8
      g.fillStyle = other; g.beginPath(); g.roundRect(x - 5, y - 5, 10, 10, 3); g.fill()
      if (o.name) label(o.name, x, y - 9, other)
      hitList.push({ x, y, text: `${o.name || 'Player'} (${o.x}, ${o.y})` })
    })
    if (sim && ok([sim.x, sim.y])) {
      g.strokeStyle = css('--muted'); g.lineWidth = 1.5
      g.beginPath(); g.moveTo(X(me.x), Y(me.y - 8)); g.lineTo(X(sim.x), Y(sim.y - 8)); g.stroke()
      g.setLineDash([3, 3])
      g.beginPath(); g.arc(X(sim.x), Y(sim.y - 8), 9, 0, 7); g.stroke()
      g.setLineDash([])
      hitList.push({ x: X(sim.x), y: Y(sim.y - 8), text: `Simulated position (${Math.round(sim.x)}, ${Math.round(sim.y)})` })
    }
    if (ok([me.x, me.y])) {
      const x = X(me.x), y = Y(me.y) - 9
      const glow = g.createRadialGradient(x, y, 0, x, y, 26)
      glow.addColorStop(0, css('--accent-glow')); glow.addColorStop(1, 'transparent')
      g.fillStyle = glow; g.beginPath(); g.arc(x, y, 26, 0, 7); g.fill()
      g.fillStyle = accent; g.beginPath(); g.arc(x, y, 7, 0, 7); g.fill()
      g.strokeStyle = '#fff'; g.lineWidth = 2; g.stroke()
      label('You', x, y - 12, accent)
      hitList.push({ x, y, text: `You (${me.x}, ${me.y})` })
    }
    hits.current = hitList
  }, [state, geometry, sim, mode, size, wanted, mobName])

  const onMove = e => {
    const r = e.currentTarget.getBoundingClientRect()
    const mx = e.clientX - r.left, my = e.clientY - r.top
    let best = null, bd = 14
    for (const hI of hits.current) { const d = Math.hypot(hI.x - mx, hI.y - my); if (d < bd) { bd = d; best = hI } }
    setTip(best ? { x: best.x, y: best.y, text: best.text } : null)
  }

  return (
    <div className="map-wrap">
      <canvas ref={ref} className="map" aria-label="Map view" onMouseMove={onMove} onMouseLeave={() => setTip(null)} />
      {tip && <div className="tip map-tip" style={{ left: tip.x, top: tip.y - 40 }}>{tip.text}</div>}
    </div>
  )
}
