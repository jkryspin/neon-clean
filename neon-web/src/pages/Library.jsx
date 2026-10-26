import { useEffect, useState } from 'react'
import { control } from '../api.js'
import { Card, Empty } from '../ui.jsx'
import { addEntry, removeEntry, patchFor } from '../library.js'

const KEY = 'neon.library'
const load = () => { try { return JSON.parse(localStorage.getItem(KEY)) || [] } catch { return [] } }
const store = list => { try { localStorage.setItem(KEY, JSON.stringify(list)) } catch { /* private window: the list lives for this visit only */ } }

// Plans and profiles other people shared, kept by name. Applying one sends it to the running bot.
export default function Library({ session }) {
  const [list, setList] = useState(load)
  const [name, setName] = useState('')
  const [code, setCode] = useState('')
  const [msg, setMsg] = useState(null)
  useEffect(() => { store(list) }, [list])

  const save = () => {
    try { setList(addEntry(list, name, code)); setName(''); setCode(''); setMsg('Saved.') } catch (e) { setMsg(e.message) }
  }
  const apply = async e => {
    try { await control(session, patchFor(e)); setMsg(`Applied “${e.name}” to session ${session}.`) } catch (err) { setMsg(`Failed: ${err.message}`) }
  }
  const copy = async e => {
    try { await navigator.clipboard.writeText(e.code); setMsg('Code copied.') } catch { setMsg('Copy failed: select the code and copy it by hand.') }
  }

  return (
    <>
      <Card title="Saved codes">
        {list.length === 0 && <Empty>Nothing saved yet. Paste a plan (NEON1:) or profile (NEONP1:) code below.</Empty>}
        {list.map(e => (
          <div className="controls" key={e.name}>
            <strong>{e.name}</strong> <small>{e.kind}, {new Date(e.added).toLocaleDateString()}</small>
            <button className="btn small" onClick={() => apply(e)}>Apply</button>
            <button className="btn small" onClick={() => copy(e)}>Copy code</button>
            <button className="btn small" onClick={() => setList(removeEntry(list, e.name))} aria-label={`Delete ${e.name}`}>✕</button>
          </div>
        ))}
      </Card>
      <Card title="Add a code">
        <input placeholder="Name" value={name} onChange={e => setName(e.target.value)} aria-label="Name" />
        <textarea rows={4} placeholder="NEON1:… or NEONP1:…" value={code} onChange={e => setCode(e.target.value)} aria-label="Share code" />
        <div className="controls"><button className="btn" onClick={save}>Save</button></div>
        {msg && <p role="status">{msg}</p>}
      </Card>
    </>
  )
}
