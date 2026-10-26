import { useEffect, useState } from 'react'
import { control, getJson, sendJson } from '../api.js'
import { Card, Empty } from '../ui.jsx'

// A profile is a named bundle of settings (and optionally a plan) you can load back into a bot at any time,
// or share as a code.
const encode = o => `NEONP1:${btoa(unescape(encodeURIComponent(JSON.stringify(o))))}`
const decode = c => JSON.parse(decodeURIComponent(escape(atob(c.trim().replace(/^NEONP1:/, '')))))

export default function Profiles({ session, live }) {
  const [list, setList] = useState([])
  const [name, setName] = useState('')
  const [code, setCode] = useState('')
  const [msg, setMsg] = useState(null)
  const reload = () => getJson('/api/profiles').then(setList).catch(() => {})
  useEffect(() => { reload() }, [])

  // Persisted fields only: what `control` accepts as settings.
  const current = () => {
    const c = live.data?.config
    if (!c) return null
    return {
      kami: c.kami, auto_pot: c.auto_pot, auto_ap: c.auto_ap, evasion: c.evasion, loot: c.loot, auto_revive: c.auto_revive,
      hp_pot_below: c.hp_pot_below, mp_pot_below: c.mp_pot_below, flee_hp_below: c.flee_hp_below, resume_hp_above: c.resume_hp_above,
      attack_range_x: c.attack_range_x, attack_range_y: c.attack_range_y, stand_off_x: c.stand_off_x, evasion_distance: c.evasion_distance,
      mobs: c.allowed_mobs || [], only_map: c.allowed_maps || [], home_map: c.home_map || 0, trusted: c.trusted_igns || [],
      macros: c.macros || [], rotation: c.rotation || [],
    }
  }

  const save = async () => {
    const settings = current()
    if (!settings || !name.trim()) return
    try { await sendJson('/api/profiles', { name: name.trim(), settings }); setMsg(`Saved “${name.trim()}”.`); setName(''); reload() } catch (e) { setMsg(e.message) }
  }
  const load = async p => { try { await control(session, { settings: p.settings, ...(p.plan ? { plan: p.plan } : {}) }); setMsg(`Loaded “${p.name}” into session ${session}.`) } catch (e) { setMsg(e.message) } }
  const remove = async p => { await sendJson(`/api/profiles?name=${encodeURIComponent(p.name)}`, undefined, 'DELETE'); reload() }
  const importCode = async () => {
    try { const o = decode(code); if (!o.settings) throw new Error('no settings'); await control(session, { settings: o.settings, ...(o.plan ? { plan: o.plan } : {}) }); setMsg('Imported and applied.') } catch { setMsg('That is not a valid profile code.') }
  }

  return (
    <main className="grid">
      <Card title="Save the current settings" className="span3">
        <div className="controls">
          <input placeholder="Profile name" value={name} onChange={e => setName(e.target.value)} aria-label="Profile name" />
          <button className="btn primary" disabled={!name.trim() || !live.data?.config} onClick={save}>Save</button>
          {msg && <span className="hint">{msg}</span>}
        </div>
      </Card>
      <Card title="Saved profiles" className="span2">
        {list.length === 0 && <Empty>No profiles yet.</Empty>}
        {list.map(p => (
          <div className="step" key={p.name}>
            <b>{p.name}</b>
            <span className="hint">{Object.keys(p.settings || {}).length} settings{p.plan ? ' + plan' : ''}</span>
            <button className="btn small primary" onClick={() => load(p)}>Load</button>
            <button className="btn small" onClick={() => { const c = encode({ settings: p.settings, plan: p.plan }); setCode(c); navigator.clipboard?.writeText(c).catch(() => {}) }}>Share code</button>
            <button className="btn small" onClick={() => remove(p)} aria-label={`Delete ${p.name}`}>✕</button>
          </div>
        ))}
      </Card>
      <Card title="Import a code">
        <textarea rows={5} placeholder="NEONP1:… (paste a profile code)" value={code} onChange={e => setCode(e.target.value)} aria-label="Profile code" />
        <button className="btn small" disabled={!code.trim()} onClick={importCode}>Import and apply</button>
      </Card>
    </main>
  )
}
