import { useEffect, useId, useRef, useState } from 'react'

// Plain-SVG time chart with an area fill, a crosshair and a tooltip.
// points: [{x, y}] (x = ms timestamp). Drawn at the element's real pixel width so text never stretches.
const hhmm = t => new Date(t).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })

function niceTicks(lo, hi, n = 3) {
  const span = hi - lo || 1
  const raw = span / n
  const mag = 10 ** Math.floor(Math.log10(raw))
  const step = [1, 2, 2.5, 5, 10].map(m => m * mag).find(s => s >= raw) || raw
  const out = []
  for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) out.push(+v.toFixed(6))
  return out
}

export default function LineChart({ points, label, unit = '', height = 160, ticks, format = v => Math.round(v).toLocaleString(), color = 'accent', area = true, refY, refLabel, step = false, zero = false }) {
  const wrap = useRef(null)
  const [w, setW] = useState(600)
  const [hover, setHover] = useState(null)
  const gid = useId().replace(/:/g, '')
  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const ro = new ResizeObserver(([e]) => setW(Math.max(200, Math.round(e.contentRect.width))))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  if (!points || points.length < 2) return <div className={`chart-wrap c-${color}`} ref={wrap}><div className="empty chart-empty" style={{ height }}>Collecting samples…</div></div>
  const H = height, PL = 8, PR = 52, PT = 10, PB = 22
  const xs = points.map(p => p.x), ys = points.map(p => p.y)
  const x0 = Math.min(...xs), x1 = Math.max(...xs)
  let y0 = Math.min(...ys), y1 = Math.max(...ys)
  if (zero) y0 = Math.min(0, y0)
  if (refY != null) { y0 = Math.min(y0, refY); y1 = Math.max(y1, refY) }
  if (ticks?.length) { y0 = Math.min(y0, ...ticks); y1 = Math.max(y1, ...ticks) }
  if (y1 === y0) y1 = y0 + 1
  const pad = (y1 - y0) * 0.08
  if (!ticks?.length) { y1 += pad; if (!zero) y0 -= pad }
  const X = x => PL + ((x - x0) / Math.max(x1 - x0, 1)) * (w - PL - PR)
  const Y = y => PT + (1 - (y - y0) / (y1 - y0)) * (H - PT - PB)
  let d = ''
  points.forEach((p, i) => {
    if (!i) d += `M${X(p.x).toFixed(1)},${Y(p.y).toFixed(1)}`
    else if (step) d += `H${X(p.x).toFixed(1)}V${Y(p.y).toFixed(1)}`
    else d += `L${X(p.x).toFixed(1)},${Y(p.y).toFixed(1)}`
  })
  const base = Y(Math.max(y0, Math.min(y1, zero ? 0 : y0)))
  const fill = `${d}L${X(x1).toFixed(1)},${base}L${X(x0).toFixed(1)},${base}Z`
  const grid = ticks?.length ? ticks : niceTicks(y0, y1)
  const xt = [0, 1 / 3, 2 / 3, 1].map(f => x0 + f * (x1 - x0))

  const onMove = e => {
    const r = e.currentTarget.getBoundingClientRect()
    const mx = e.clientX - r.left
    let best = 0, bd = Infinity
    points.forEach((p, i) => { const dd = Math.abs(X(p.x) - mx); if (dd < bd) { bd = dd; best = i } })
    setHover(best)
  }
  const hp = hover != null ? points[hover] : null

  return (
    <div className={`chart-wrap c-${color}`} ref={wrap}>
      <svg width={w} height={H} className="chart" role="img" aria-label={label} onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
        <defs>
          <linearGradient id={`g${gid}`} x1="0" x2="0" y1="0" y2="1">
            <stop offset="0" stopColor="currentColor" stopOpacity=".32" />
            <stop offset="1" stopColor="currentColor" stopOpacity="0" />
          </linearGradient>
        </defs>
        {grid.filter(t => t >= y0 - 1e-9 && t <= y1 + 1e-9).map(t => (
          <g key={t}>
            <line x1={PL} x2={w - PR} y1={Y(t)} y2={Y(t)} className="grid-line" />
            <text x={w - PR + 6} y={Y(t) + 3.5} className="tick">{format(t)}{unit}</text>
          </g>
        ))}
        {xt.map((t, i) => <text key={i} x={X(t)} y={H - 6} textAnchor={i === 0 ? 'start' : i === 3 ? 'end' : 'middle'} className="tick">{hhmm(t)}</text>)}
        {refY != null && <><line x1={PL} x2={w - PR} y1={Y(refY)} y2={Y(refY)} className="ref-line" />{refLabel && <text x={PL + 4} y={Y(refY) - 4} className="tick">{refLabel}</text>}</>}
        {area && <path d={fill} fill={`url(#g${gid})`} className="area" />}
        <path d={d} className="line" />
        {hp && <>
          <line x1={X(hp.x)} x2={X(hp.x)} y1={PT} y2={H - PB} className="cross" />
          <circle cx={X(hp.x)} cy={Y(hp.y)} r="4.5" className="cross-dot" />
        </>}
      </svg>
      {hp && (
        <div className="tip" style={{ left: Math.min(Math.max(X(hp.x), 60), w - 60), top: Math.max(0, Y(hp.y) - 46) }}>
          <b>{format(hp.y)}{unit}</b><span>{hhmm(hp.x)}</span>
        </div>
      )}
    </div>
  )
}

// Tiny trend line for stat tiles.
export function Sparkline({ points, color = 'accent', height = 28 }) {
  if (!points || points.length < 2) return <svg className={`spark c-${color}`} height={height} />
  const W = 100
  const xs = points.map(p => p.x), ys = points.map(p => p.y)
  const x0 = Math.min(...xs), x1 = Math.max(...xs), y0 = Math.min(...ys), y1 = Math.max(...ys)
  const X = x => ((x - x0) / Math.max(x1 - x0, 1)) * W
  const Y = y => height - 2 - ((y - y0) / Math.max(y1 - y0, 1e-9)) * (height - 4)
  const d = points.map((p, i) => `${i ? 'L' : 'M'}${X(p.x).toFixed(1)},${(y1 === y0 ? height / 2 : Y(p.y)).toFixed(1)}`).join('')
  return (
    <svg className={`spark c-${color}`} viewBox={`0 0 ${W} ${height}`} preserveAspectRatio="none" height={height} aria-hidden>
      <path d={d} vectorEffect="non-scaling-stroke" />
    </svg>
  )
}
