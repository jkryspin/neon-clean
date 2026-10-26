// Keep-list helpers for the shop step. `classify` mirrors neon-loader's `next_sale` rule: a row is kept when it is
// among the first `keep` rows of its tab, or its item id is in `keepItems`; everything else would be sold.

/** "2000000, 2000001 1002001" -> [2000000, 2000001, 1002001] (deduplicated, positive integers only). */
export function parseIds(text) {
  const ids = String(text ?? '')
    .split(/[\s,;]+/)
    .filter(Boolean)
    .map(Number)
    .filter(n => Number.isInteger(n) && n > 0)
  return [...new Set(ids)]
}

/** `items` is a tab's item ids in slot order (null for an unknown id). Returns [{ index, id, action }]. */
export function classify(items, keep = 0, keepItems = []) {
  const protectedIds = new Set(keepItems)
  return items.map((id, index) => ({
    index,
    id,
    action: index < keep || (id != null && protectedIds.has(id)) ? 'keep' : 'sell',
  }))
}

export function summarize(rows) {
  const kept = rows.filter(r => r.action === 'keep').length
  return { kept, sold: rows.length - kept }
}
