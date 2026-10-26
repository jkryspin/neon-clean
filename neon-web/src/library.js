// A local library of share codes: plans (`NEON1:`) and profiles (`NEONP1:`). Entries live in the browser; the
// codes themselves are what gets shared, pasted into chat or a file.

const PREFIXES = { plan: 'NEON1:', profile: 'NEONP1:' }

const toB64 = o => btoa(unescape(encodeURIComponent(JSON.stringify(o))))
const fromB64 = s => JSON.parse(decodeURIComponent(escape(atob(s))))

export function encodeCode(kind, obj) {
  if (!PREFIXES[kind]) throw new Error(`unknown kind '${kind}'`)
  return PREFIXES[kind] + toB64(obj)
}

/** -> { kind, value } or throws. "NEONP1:" is tested first: "NEON1:" is not a prefix of it, but be explicit. */
export function decodeCode(code) {
  const c = String(code ?? '').trim()
  for (const kind of ['profile', 'plan']) {
    if (c.startsWith(PREFIXES[kind])) {
      let value
      try { value = fromB64(c.slice(PREFIXES[kind].length)) } catch { throw new Error('the code is damaged') }
      if (kind === 'plan' && !Array.isArray(value?.steps)) throw new Error('the plan has no steps')
      if (kind === 'profile' && !value?.settings) throw new Error('the profile has no settings')
      return { kind, value }
    }
  }
  throw new Error('not a NEON1: or NEONP1: code')
}

/** Add (or replace by name) an entry; newest first. */
export function addEntry(list, name, code, now = Date.now()) {
  const n = String(name ?? '').trim()
  if (!n) throw new Error('give it a name')
  const { kind } = decodeCode(code)
  return [{ name: n, kind, code: code.trim(), added: now }, ...list.filter(e => e.name !== n)]
}

export const removeEntry = (list, name) => list.filter(e => e.name !== name)

/** The control patch that applies an entry to a running bot. */
export function patchFor(entry) {
  const { kind, value } = decodeCode(entry.code)
  return kind === 'plan' ? { plan: value } : { settings: value.settings, ...(value.plan ? { plan: value.plan } : {}) }
}
