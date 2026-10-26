import { useEffect, useRef, useState } from 'react'
import { control } from '../api.js'
import { Card, Empty, Toggle } from '../ui.jsx'

// "When X, do Y": a safe scripting layer. Triggers read the game state; actions press a key, pause/stop the bot,
// send an alert or change settings. No code runs: rules are plain data the bot interprets.
const TRIGGERS = {
  hp_below: { label: 'HP is below', params: [['pct', '%', 50]] },
  hp_above: { label: 'HP is above', params: [['pct', '%', 90]] },
  mp_below: { label: 'MP is below', params: [['pct', '%', 30]] },
  level_at_least: { label: 'Level reaches', params: [['level', 'level', 10]] },
  in_map: { label: 'Is in map', params: [['map', 'map id', 10001030]] },
  not_in_map: { label: 'Is not in map', params: [['map', 'map id', 10001030]] },
  mobs_near: { label: 'Mobs within range', params: [['count', 'at least', 3], ['within_px', 'px', 150]] },
  no_mobs_for: { label: 'No mobs nearby for', params: [['ms', 'ms', 10000], ['within_px', 'px', 400]] },
  stranger_near: { label: 'A stranger is within', params: [['within_px', 'px', 300]] },
  every_ms: { label: 'Every', params: [['ms', 'ms', 60000]] },
}
const ACTIONS = {
  tap: { label: 'Press key', params: [['key', 'key', 'F5', 'text']] },
  pause: { label: 'Pause the bot', params: [] },
  stop: { label: 'Stop the bot', params: [] },
  notify: { label: 'Send an alert', params: [['text', 'message', 'rule fired', 'text']] },
  set: { label: 'Change settings (JSON)', params: [['settings', 'JSON', '{"mobs":[2,3]}', 'json']] },
}

const blank = () => ({ name: 'New rule', enabled: true, when: { kind: 'hp_below', pct: 50 }, then: [{ kind: 'notify', text: 'low HP' }], cooldown_ms: 5000, edge: true })
const withDefaults = (table, kind) => Object.fromEntries([['kind', kind], ...table[kind].params.map(([k, , d, type]) => [k, type === 'json' ? JSON.parse(d) : d])])

function Fields({ table, value, onChange }) {
  const spec = table[value.kind]
  return (
    <>
      <select value={value.kind} onChange={e => onChange(withDefaults(table, e.target.value))} aria-label="Kind">
        {Object.entries(table).map(([k, v]) => <option key={k} value={k}>{v.label}</option>)}
      </select>
      {spec.params.map(([k, label, , type]) => (
        <label className="mini" key={k}>{label}
          {type === 'json'
            ? <JsonInput value={value[k]} onChange={v => onChange({ ...value, [k]: v })} />
            : type === 'text'
              ? <input value={value[k] ?? ''} onChange={e => onChange({ ...value, [k]: e.target.value })} />
              : <input type="number" value={value[k] ?? ''} onChange={e => onChange({ ...value, [k]: Number(e.target.value) })} />}
        </label>
      ))}
    </>
  )
}

function JsonInput({ value, onChange }) {
  const [text, setText] = useState(JSON.stringify(value))
  const [bad, setBad] = useState(false)
  return <input className={bad ? 'bad' : ''} value={text} aria-invalid={bad} onChange={e => {
    setText(e.target.value)
    try { onChange(JSON.parse(e.target.value)); setBad(false) } catch { setBad(true) }
  }} />
}

export default function Rules({ session, live }) {
  const running = live.data?.rules
  const [rules, setRules] = useState(() => running ?? [])
  const [dirty, setDirty] = useState(false)
  const [msg, setMsg] = useState(null)
  const seeded = useRef(!!running?.length)
  useEffect(() => { if (running && !dirty && !seeded.current) { setRules(running); seeded.current = true } }, [running, dirty])

  const edit = next => { setRules(next); setDirty(true); setMsg(null) }
  const upd = (i, patch) => edit(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)))
  const apply = async () => {
    try { await control(session, { rules }); setDirty(false); setMsg(`Sent ${rules.length} rule(s).`) } catch (e) { setMsg(`Failed: ${e.message}`) }
  }

  return (
    <>
      <div className="controls">
        <button className="btn primary" disabled={!dirty} onClick={apply}>Apply rules to the running bot</button>
        <button className="btn small" onClick={() => edit([...rules, blank()])}>+ Add rule</button>
        {msg && <span className="hint">{msg}</span>}
      </div>
      <main className="grid">
        <Card title="Rules" className="span3">
          <p className="hint">A rule fires when its condition <b>becomes</b> true (untick “only on change” to repeat while it stays true), then waits for its cooldown.</p>
          {rules.length === 0 && <Empty>No rules. Example: when HP drops below 40%, send an alert; every minute, press a buff key.</Empty>}
          {rules.map((r, i) => (
            <div className="rule" key={i}>
              <div className="row-edit">
                <Toggle label="" checked={r.enabled} onChange={v => upd(i, { enabled: v })} />
                <input value={r.name} onChange={e => upd(i, { name: e.target.value })} aria-label="Rule name" />
                <button className="btn small" onClick={() => edit(rules.filter((_, j) => j !== i))} aria-label="Delete rule">✕</button>
              </div>
              <div className="row-edit"><b>When</b><Fields table={TRIGGERS} value={r.when} onChange={w => upd(i, { when: w })} /></div>
              {r.then.map((a, ai) => (
                <div className="row-edit" key={ai}>
                  <b>{ai === 0 ? 'Then' : 'and'}</b>
                  <Fields table={ACTIONS} value={a} onChange={na => upd(i, { then: r.then.map((x, k) => (k === ai ? na : x)) })} />
                  <button className="btn small" onClick={() => upd(i, { then: r.then.filter((_, k) => k !== ai) })} aria-label="Remove action">✕</button>
                </div>
              ))}
              <div className="row-edit">
                <button className="btn small" onClick={() => upd(i, { then: [...r.then, withDefaults(ACTIONS, 'notify')] })}>+ action</button>
                <label className="mini">cooldown ms<input type="number" value={r.cooldown_ms} onChange={e => upd(i, { cooldown_ms: Number(e.target.value) })} /></label>
                <label className="mini">only on change<input type="checkbox" checked={r.edge} onChange={e => upd(i, { edge: e.target.checked })} /></label>
              </div>
            </div>
          ))}
        </Card>
      </main>
    </>
  )
}
