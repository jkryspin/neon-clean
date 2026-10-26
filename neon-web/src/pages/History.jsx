import { useMemo, useState } from 'react'
import { Card, Empty, Stat, clock, fmt, minutes } from '../ui.jsx'
import LineChart from '../LineChart.jsx'

// Per-session history written by the bot every 30 s (history/<session>.jsonl). The file spans every run of
// the session; runs are split where the bot's counters restart. Level comes from the EXP walk (track.js).
function splitRuns(rows) {
  const runs = []
  let cur = null
  for (const r of rows) {
    const restart = !cur || r.t - cur.rows[cur.rows.length - 1].t > 5 * 60000 || (r.kills ?? 0) < (cur.rows[cur.rows.length - 1].kills ?? 0) || (r.attacks ?? 0) < (cur.rows[cur.rows.length - 1].attacks ?? 0)
    if (restart) { cur = { rows: [] }; runs.push(cur) }
    cur.rows.push(r)
  }
  return runs.map(run => {
    const f = run.rows[0], l = run.rows[run.rows.length - 1]
    return { start: f.t, end: l.t, rows: run.rows, kills: l.kills ?? 0, deaths: l.deaths ?? 0, exp: l.exp_gained ?? 0, map: l.map }
  })
}

export default function History({ session, rows, track, meta }) {
  const [range, setRange] = useState('all')
  const runs = useMemo(() => (rows ? splitRuns(rows) : []), [rows])
  if (!rows) return <Empty>Loading…</Empty>
  if (rows.length < 2) return <Empty>No history yet: the bot records a point every 30 seconds while it runs.</Empty>

  const cut = range === 'all' ? 0 : Date.now() - Number(range) * 3600000
  const view = rows.filter(r => r.t >= cut)
  const last = rows[rows.length - 1], first = view[0] || rows[0]
  const series = key => view.filter(r => r[key] != null).map(r => ({ x: r.t, y: Number(r[key]) }))
  const spanMin = Math.round((last.t - first.t) / 60000)
  const totalKills = runs.reduce((a, r) => a + r.kills, 0)
  const totalDeaths = runs.reduce((a, r) => a + r.deaths, 0)
  const ups = (track?.levelUps || []).filter(u => u.t >= first.t)
  const mapName = id => meta?.maps?.find(m => m.id === Number(id))?.name || (id ? `Map ${id}` : '–')
  const ranges = [['1', '1 h'], ['3', '3 h'], ['12', '12 h'], ['all', 'All']]

  return (
    <main className="grid">
      <Card title={`Session “${session}”`} icon="history" className="span3" right={
        <span className="tabs">{ranges.map(([k, l]) => <button key={k} className={range === k ? 'on' : ''} onClick={() => setRange(k)}>{l}</button>)}</span>
      }>
        <div className="stats wide">
          <Stat label="Recorded" value={minutes(spanMin)} hint={`${runs.length} run${runs.length === 1 ? '' : 's'}`} />
          <Stat label="Level now" value={track?.level ?? '–'} hint={ups.length ? `+${ups.length} level${ups.length > 1 ? 's' : ''} in this range` : 'from the EXP history'} />
          <Stat label="EXP gained (last run)" value={fmt(last.exp_gained)} />
          <Stat label="EXP/hour (now)" value={fmt(last.exp_per_hour)} />
          <Stat label="Kills (all runs)" value={fmt(totalKills)} />
          <Stat label="Deaths (all runs)" value={fmt(totalDeaths)} />
        </div>
      </Card>
      <Card title="EXP within the level" icon="bolt" className="span2"><LineChart points={series('exp')} label="EXP" refY={track?.need ?? undefined} refLabel={track?.need ? `Lv ${track.level} needs ${fmt(track.need)}` : undefined} /></Card>
      <Card title="EXP per hour" icon="pulse"><LineChart points={series('exp_per_hour')} label="EXP per hour" zero /></Card>
      <Card title="Kills (per run)" icon="sword" className="span2"><LineChart points={series('kills')} label="Kills" color="good" step zero /></Card>
      <Card title="HP" icon="heart"><LineChart points={series('hp')} label="HP" color="hp" zero /></Card>
      <Card title="Runs" icon="list" className="span3">
        <div className="table-wrap">
          <table className="table">
            <thead><tr><th>Started</th><th className="r">Length</th><th>Map at end</th><th className="r">Kills</th><th className="r">EXP</th><th className="r">EXP/h</th><th className="r">Deaths</th></tr></thead>
            <tbody>
              {[...runs].reverse().map(r => {
                const h = (r.end - r.start) / 3600000
                return (
                  <tr key={r.start}>
                    <td>{new Date(r.start).toLocaleDateString([], { month: 'short', day: 'numeric' })} {clock(r.start)}</td>
                    <td className="r">{minutes(Math.round((r.end - r.start) / 60000))}</td>
                    <td>{mapName(r.map)}</td>
                    <td className="r">{fmt(r.kills)}</td>
                    <td className="r">{fmt(r.exp)}</td>
                    <td className="r">{h > 0.02 ? fmt(Math.round(r.exp / h)) : '–'}</td>
                    <td className="r">{fmt(r.deaths)}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      </Card>
    </main>
  )
}
