import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import fs from 'node:fs'
import path from 'node:path'
import { createApi } from './server/api.mjs'

const ROOT = path.resolve(__dirname, '..')

// The bot (neon-loader) writes neon-status.json a couple of times a second and watches neon-control.json;
// the API module in server/api.mjs reads/writes those files. Nothing else needs to run.
function neonApi() {
  const api = createApi(ROOT)
  return {
    name: 'neon-api',
    // Inline the current status into the page so the first paint is never empty.
    transformIndexHtml(_html, ctx) {
      if (!ctx.server) return [] // dev server only: never bake a status snapshot into a build
      const parts = []
      const safe = v => v.replace(/</g, '\\u003c')
      try { parts.push(`window.__LIVE__=${safe(fs.readFileSync(path.join(ROOT, 'neon-status.json'), 'utf8'))}`) } catch { /* no status yet */ }
      try { parts.push(`window.__META__=${safe(JSON.stringify(api.meta()))}`) } catch { /* no reference data */ }
      return parts.length ? [{ tag: 'script', children: parts.join(';'), injectTo: 'head-prepend' }] : []
    },
    configureServer(s) { s.middlewares.use(api) },
    configurePreviewServer(s) { s.middlewares.use(api) },
  }
}

// The API can start and stop the bot, so it listens on localhost only. Set NEON_WEB_HOST=0.0.0.0 to
// expose it on your network (do that only on a network you trust).
export default defineConfig({
  plugins: [react(), neonApi()],
  // Vite refuses requests whose Host header it does not know (DNS-rebinding protection); IPs and localhost are always fine.
  server: { port: 3000, host: process.env.NEON_WEB_HOST || '127.0.0.1', allowedHosts: ['john-box.local', 'john-box'] },
  preview: { port: 3000, host: process.env.NEON_WEB_HOST || '127.0.0.1', allowedHosts: ['john-box.local', 'john-box'] },
})
