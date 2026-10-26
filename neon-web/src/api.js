// Thin helpers for the local API (server/api.mjs).
export async function getJson(url) {
  const res = await fetch(url, { cache: 'no-store' })
  if (!res.ok) throw new Error((await res.json().catch(() => ({}))).error || `HTTP ${res.status}`)
  return res.json()
}

export async function sendJson(url, body, method = 'POST') {
  const res = await fetch(url, { method, headers: { 'Content-Type': 'application/json' }, body: body === undefined ? undefined : JSON.stringify(body) })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`)
  return data
}

export const control = (session, patch) => sendJson(`/api/control?session=${encodeURIComponent(session)}`, patch)
