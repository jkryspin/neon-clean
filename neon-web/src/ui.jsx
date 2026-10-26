import Icon from './icons.jsx'

// Small shared pieces.
export const fmt = n => (n == null ? '–' : Number(n).toLocaleString())
export const clock = t => new Date(t).toLocaleTimeString([], { hour12: false })

export function dur(ms) {
  const s = Math.max(0, Math.floor(ms / 1000))
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}m ${String(s % 60).padStart(2, '0')}s`
}

export function minutes(m) {
  if (m == null) return '–'
  if (m < 60) return `${m} min`
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, '0')} min`
}

export const mobLabel = (meta, id) => (id == null || id === 0 ? 'Unidentified' : meta?.mobNames?.[id] || `Mob #${id}`)
export const mapLabel = (meta, id) => {
  const m = meta?.maps?.find(x => x.id === Number(id))
  return m?.name ? `${m.name}` : id != null ? `Map ${id}` : '–'
}

export function Card({ title, children, className = '', right, icon }) {
  return (
    <section className={`card ${className}`}>
      {title && <h2><span className="h-title">{icon && <Icon name={icon} size={16} />}{title}</span>{right && <span className="right">{right}</span>}</h2>}
      {children}
    </section>
  )
}

export function Bar({ label, value, max, tone, big, low }) {
  const pct = value != null && max ? Math.min(100, (value / max) * 100) : 0
  return (
    <div className="bar-row">
      <div className="bar-label"><span>{label}</span><span><b>{fmt(value)}</b> / {fmt(max)} · {max ? `${pct.toFixed(pct < 10 ? 1 : 0)}%` : '–'}</span></div>
      <div className={`bar ${big ? 'big' : ''}`} role="progressbar" aria-label={label} aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100}>
        <div className={`fill ${tone} ${low && pct < low ? 'low' : ''}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  )
}

export function Stat({ label, value, hint }) {
  return (
    <div className="stat">
      <div className="stat-value">{value}</div>
      <div className="stat-label">{label}</div>
      {hint && <div className="stat-hint">{hint}</div>}
    </div>
  )
}

export function Toggle({ label, checked, onChange, hint }) {
  return (
    <label className="toggle">
      <input type="checkbox" checked={!!checked} onChange={e => onChange(e.target.checked)} />
      <span className="switch" aria-hidden />
      <span className="toggle-text">{label}{hint && <small>{hint}</small>}</span>
    </label>
  )
}

export function Num({ label, value, onChange, min, max, step = 1, unit, hint }) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <span className="field-input">
        <input type="number" value={value ?? ''} min={min} max={max} step={step} onChange={e => onChange(e.target.value === '' ? undefined : Number(e.target.value))} />
        {unit && <em>{unit}</em>}
      </span>
      {hint && <small>{hint}</small>}
    </label>
  )
}

export function Pill({ tone, children }) {
  return <span className={`pill ${tone}`}><i />{children}</span>
}

export function Empty({ children }) {
  return <div className="empty">{children}</div>
}
