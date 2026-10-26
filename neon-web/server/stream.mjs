// Live view of the game window for the panel: MJPEG over HTTP, read-only.
//
// Frames come from ImageMagick's `import -window <id>` (a screen grab of the X window; nothing is sent to the
// game). One shared capture loop runs only while at least one viewer is connected and fans every frame out to
// all of them; the last viewer leaving stops it. Each grab is a separate niced process, so the event loop is
// never blocked. Viewers can ask for a rate and a scale; the loop uses the highest that anyone asked for.

import fs from 'node:fs'
import { spawn } from 'node:child_process'

const BOUNDARY = 'neonframe'
const MAX_FPS = 8
const SCALES = [50, 75, 100]

function xEnv(display) {
  const env = { ...process.env, DISPLAY: display || process.env.DISPLAY || ':0' }
  // Instance displays (Xvfb, neon-instances.json) need no cookie; the desktop's Xwayland does.
  if (display && display !== ':0') delete env.XAUTHORITY
  else if (!process.env.XAUTHORITY) {
    // Xwayland under mutter keeps its cookie in a randomly named file that changes per login.
    const dir = `/run/user/${process.getuid?.() ?? 1000}`
    try {
      const f = fs.readdirSync(dir).filter(n => n.startsWith('.mutter-Xwaylandauth.')).sort()[0]
      if (f) env.XAUTHORITY = `${dir}/${f}`
    } catch { /* no runtime dir */ }
  }
  return env
}

// Runs a command with a time limit and returns stdout as a Buffer (rejects on failure).
function run(cmd, args, { timeout = 4000, env } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { env, stdio: ['ignore', 'pipe', 'pipe'] })
    const out = [], err = []
    const timer = setTimeout(() => child.kill('SIGKILL'), timeout)
    child.stdout.on('data', c => out.push(c))
    child.stderr.on('data', c => err.push(c))
    child.on('error', e => { clearTimeout(timer); reject(e) })
    child.on('close', code => {
      clearTimeout(timer)
      if (code === 0) resolve(Buffer.concat(out))
      else reject(new Error(Buffer.concat(err).toString().trim().split('\n')[0] || `${cmd} exited with ${code}`))
    })
  })
}

/// One capture per X display: the desktop client on :0, each instance on its own Xvfb display.
export function createStream(display) {
  let env = xEnv(display)
  let win = null, winAt = 0
  const viewers = new Set() // { res, fps, scale }
  let loop = false
  let last = null // { buf, t, scale }
  let lastError = null
  let frames = 0, rateT = Date.now(), rateFps = 0
  let placeholder = null

  // The game's client window: named "MapleStory" with class maplestory.exe (the other match is mutter's frame).
  async function findWindow() {
    if (win && Date.now() - winAt < 15000) return win
    env = xEnv(display)
    const ids = (await run('xdotool', ['search', '--name', '^MapleStory$'], { env, timeout: 2000 }).catch(() => Buffer.from(''))).toString().split(/\s+/).filter(Boolean)
    let pick = null
    for (const id of ids) {
      const cls = (await run('xprop', ['-id', id, 'WM_CLASS'], { env, timeout: 2000 }).catch(() => Buffer.from(''))).toString()
      if (/maplestory\.exe/i.test(cls)) { pick = id; break }
    }
    win = pick || ids[0] || null
    winAt = Date.now()
    return win
  }

  async function grab(scale) {
    const id = await findWindow()
    if (!id) throw new Error('game window not found')
    const args = ['-n', '15', 'import', '-silent', '-window', id]
    if (scale < 100) args.push('-resize', `${scale}%`)
    args.push('-quality', '60', 'jpeg:-')
    try {
      const buf = await run('nice', args, { env, timeout: 4000 })
      if (buf.length < 100) throw new Error('empty frame')
      return buf
    } catch (e) {
      win = null // the window may have been recreated (client restart); look it up again next time
      throw e
    }
  }

  async function getPlaceholder() {
    if (placeholder) return placeholder
    placeholder = await run('nice', ['-n', '15', 'convert', '-size', '960x540', 'xc:#0e0c17', '-fill', '#9a93b3', '-gravity', 'center',
      '-pointsize', '30', '-annotate', '0', 'Game window not found', 'jpeg:-'], { timeout: 4000 }).catch(() => null)
    return placeholder
  }

  function send(v, buf) {
    // A slow viewer skips frames instead of buffering them without limit.
    if (v.res.writableLength > 1.5 * 1024 * 1024) return
    v.res.write(`--${BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: ${buf.length}\r\n\r\n`)
    v.res.write(buf)
    v.res.write('\r\n')
  }

  async function runLoop() {
    if (loop) return
    loop = true
    while (viewers.size) {
      const fps = Math.max(...[...viewers].map(v => v.fps))
      const scale = Math.max(...[...viewers].map(v => v.scale))
      const t0 = Date.now()
      let buf = null
      try {
        buf = await grab(scale)
        last = { buf, t: Date.now(), scale }
        lastError = null
        frames += 1
      } catch (e) {
        lastError = String(e.message || e)
        buf = await getPlaceholder()
      }
      if (buf) for (const v of viewers) send(v, buf)
      const now = Date.now()
      if (now - rateT >= 2000) { rateFps = (frames * 1000) / (now - rateT); frames = 0; rateT = now }
      const wait = lastError ? 2000 : Math.max(10, 1000 / fps - (now - t0))
      await new Promise(r => setTimeout(r, wait))
    }
    loop = false
    rateFps = 0
  }

  const clampFps = v => Math.min(MAX_FPS, Math.max(1, Number(v) || 4))
  const clampScale = v => SCALES.includes(Number(v)) ? Number(v) : 75

  // GET /api/stream?fps=4&scale=75
  function stream(req, res, url) {
    res.statusCode = 200
    res.setHeader('Content-Type', `multipart/x-mixed-replace; boundary=${BOUNDARY}`)
    res.setHeader('Cache-Control', 'no-store, no-cache, must-revalidate')
    res.setHeader('Pragma', 'no-cache')
    res.setHeader('Connection', 'close')
    res.setHeader('X-Accel-Buffering', 'no')
    const v = { res, fps: clampFps(url.searchParams.get('fps')), scale: clampScale(url.searchParams.get('scale')) }
    viewers.add(v)
    // Start with the latest frame so the picture appears at once.
    if (last && Date.now() - last.t < 2000) send(v, last.buf)
    const drop = () => viewers.delete(v)
    req.on('close', drop)
    res.on('close', drop)
    res.on('error', drop)
    runLoop()
  }

  // GET /api/frame.jpg?scale=100: one frame, from the running loop when it is fresh.
  let pending = null
  async function frame(req, res, url) {
    const scale = clampScale(url.searchParams.get('scale') || 100)
    try {
      let buf
      if (last && Date.now() - last.t < 1000 && last.scale >= scale) buf = last.buf
      else {
        pending ??= grab(scale).finally(() => { pending = null })
        buf = await pending
      }
      res.statusCode = 200
      res.setHeader('Content-Type', 'image/jpeg')
      res.setHeader('Cache-Control', 'no-store')
      res.setHeader('Content-Length', buf.length)
      res.end(buf)
    } catch (e) {
      res.statusCode = 503
      res.setHeader('Content-Type', 'application/json')
      res.end(JSON.stringify({ error: String(e.message || e) }))
    }
  }

  function status() {
    return {
      viewers: viewers.size, capturing: loop, fps: Math.round(rateFps * 10) / 10, window: win, error: lastError,
      last_frame_ms: last ? Date.now() - last.t : null, bytes: last?.buf.length ?? null, scale: last?.scale ?? null,
    }
  }

  return { stream, frame, status }
}
