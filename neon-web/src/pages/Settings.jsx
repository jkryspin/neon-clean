import { useEffect, useRef, useState } from 'react'
import { control } from '../api.js'
import { Card, Empty, Num, Toggle, mobLabel } from '../ui.jsx'

// Settings the bot accepts live. The form starts from what the running bot reports and sends only what you change.
const NUM = [
  ['flee_hp_below', 'Retreat below', '%', 'Stop fighting and recover on a rope'],
  ['resume_hp_above', 'Resume above', '%', 'Go back to fighting'],
  ['hp_pot_below', 'HP potion below', '%'],
  ['mp_pot_below', 'MP potion below', '%'],
  ['attack_range_x', 'Attack from within', 'px', 'Beginner punch reaches 64 px'],
  ['attack_range_y', 'Vertical reach', 'px'],
  ['stand_off_x', 'Walk up to', 'px', 'Stand this far from a mob'],
  ['evasion_distance', 'Stranger distance', 'px'],
  ['max_deaths', 'Stop after deaths', '', '0 = never stop for this'],
]
const TOGGLES = [
  ['kami', 'Fight mobs', 'Walk to and attack mobs'],
  ['auto_pot', 'Auto potions'],
  ['auto_ap', 'Auto stat points'],
  ['loot', 'Loot drops', 'Off if a pet picks items up'],
  ['evasion', 'Pause for strangers', 'Hold still when another player comes close'],
  ['auto_revive', 'Auto revive', 'Click “revive in town” (max 3 per hour)'],
]

const blankMacro = () => ({ label: '', key: '', every_ms: 60000, min_mp_pct: 0, only_when_fighting: false, enabled: true })
const blankSkill = () => ({ label: '', key: '', cooldown_ms: 1000, min_mp_pct: 0, enabled: true })

function fromConfig(c) {
  if (!c) return null
  return {
    ...Object.fromEntries([...NUM, ...TOGGLES].map(([k]) => [k, c[k]])),
    mobs: c.allowed_mobs ? [...c.allowed_mobs].sort((a, b) => a - b) : [],
    only_map: c.allowed_maps ? [...c.allowed_maps] : [],
    home_map: c.home_map ?? 0,
    trusted: [...(c.trusted_igns || [])],
    macros: c.macros || [],
    rotation: c.rotation || [],
  }
}

function ListEditor({ rows, set, blank, fields, title, empty }) {
  const upd = (i, k, v) => set(rows.map((r, j) => (j === i ? { ...r, [k]: v } : r)))
  return (
    <>
      {rows.length === 0 && <Empty>{empty}</Empty>}
      {rows.map((r, i) => (
        <div className="row-edit" key={i}>
          <Toggle label="" checked={r.enabled} onChange={v => upd(i, 'enabled', v)} />
          <input placeholder="Label" value={r.label} onChange={e => upd(i, 'label', e.target.value)} aria-label={`${title} label`} />
          <input placeholder="Key" className="short" value={r.key} onChange={e => upd(i, 'key', e.target.value)} aria-label="Key" />
          {fields.map(f => (
            <label key={f.k} className="mini">{f.label}
              {f.bool
                ? <input type="checkbox" checked={!!r[f.k]} onChange={e => upd(i, f.k, e.target.checked)} />
                : <input type="number" value={r[f.k] ?? 0} onChange={e => upd(i, f.k, Number(e.target.value))} />}
            </label>
          ))}
          <button className="btn small" onClick={() => set(rows.filter((_, j) => j !== i))} aria-label="Remove">✕</button>
        </div>
      ))}
      <button className="btn small" onClick={() => set([...rows, blank()])}>+ Add</button>
    </>
  )
}

export default function Settings({ session, live, meta }) {
  const { data } = live
  const [form, setForm] = useState(() => fromConfig(data?.config))
  const [dirty, setDirty] = useState(false)
  const [msg, setMsg] = useState(null)
  const seeded = useRef(!!form)

  useEffect(() => {
    if (data?.config && (!seeded.current || !dirty)) {
      setForm(fromConfig(data.config)); seeded.current = true
    }
  }, [data?.config, dirty])

  if (!form) return <Empty>Start the bot (or wait for its first status) to edit its settings.</Empty>
  const set = (k, v) => { setForm(f => ({ ...f, [k]: v })); setDirty(true); setMsg(null) }
  const here = meta?.maps?.find(m => m.id === data?.state?.player?.map_id)
  const mobIds = [...new Set([...(here ? Object.keys(here.mobs).map(Number) : []), ...form.mobs])].sort((a, b) => a - b)
  const toggleMob = id => set('mobs', form.mobs.includes(id) ? form.mobs.filter(x => x !== id) : [...form.mobs, id].sort((a, b) => a - b))

  const apply = async () => {
    try {
      await control(session, { settings: form })
      setDirty(false); setMsg('Sent: the bot applies it within a second.')
    } catch (e) { setMsg(`Failed: ${e.message}`) }
  }

  return (
    <>
      <div className="controls">
        <button className="btn primary" onClick={apply} disabled={!dirty}>Apply to the running bot</button>
        {dirty && <button className="btn" onClick={() => { setForm(fromConfig(data.config)); setDirty(false) }}>Discard changes</button>}
        {msg && <span className="hint">{msg}</span>}
      </div>
      <main className="grid">
        <Card title="Features">
          <div className="toggles">{TOGGLES.map(([k, label, hint]) => <Toggle key={k} label={label} hint={hint} checked={form[k]} onChange={v => set(k, v)} />)}</div>
        </Card>

        <Card title="Combat and safety" className="span2">
          <div className="fields">{NUM.map(([k, label, unit, hint]) => <Num key={k} label={label} unit={unit} hint={hint} value={form[k]} onChange={v => set(k, v)} />)}</div>
        </Card>

        <Card title="Which mobs to fight" className="span2">
          <p className="hint">{form.mobs.length ? 'Only the ticked mobs.' : 'Anything that is not blocked (none ticked = everything).'} Mobs on {here?.name || 'this map'} first.</p>
          <div className="chips">
            {mobIds.map(id => (
              <button key={id} className={`chip ${form.mobs.includes(id) ? 'on' : ''}`} onClick={() => toggleMob(id)} aria-pressed={form.mobs.includes(id)}>
                {mobLabel(meta, id)}{here?.mobs?.[id] ? <small>{here.mobs[id]}</small> : null}
              </button>
            ))}
            {mobIds.length === 0 && <Empty>No spawn data for this map.</Empty>}
          </div>
        </Card>

        <Card title="Friends">
          <p className="hint">Names that never make the bot pause (one per line).</p>
          <textarea rows={4} value={form.trusted.join('\n')} onChange={e => set('trusted', e.target.value.split('\n').map(s => s.trim()).filter(Boolean))} aria-label="Friend list" />
        </Card>

        <Card title="Timed macros (buffs, pet food…)" className="span3">
          <ListEditor rows={form.macros} set={v => set('macros', v)} blank={blankMacro} title="Macro" empty="No macros. A macro presses a key every N ms."
            fields={[{ k: 'every_ms', label: 'every ms' }, { k: 'min_mp_pct', label: 'min MP %' }, { k: 'only_when_fighting', label: 'only in a fight', bool: true }]} />
        </Card>

        <Card title="Attack skill rotation" className="span3">
          <ListEditor rows={form.rotation} set={v => set('rotation', v)} blank={blankSkill} title="Skill" empty="No skills: the basic attack is used. A listed skill replaces it whenever it is off cooldown."
            fields={[{ k: 'cooldown_ms', label: 'cooldown ms' }, { k: 'min_mp_pct', label: 'min MP %' }]} />
        </Card>
      </main>
    </>
  )
}
