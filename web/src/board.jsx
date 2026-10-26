// Zeichenbrett in NextPad: Excalidraw, synchronisiert über denselben Yjs-Server wie die Text-Pads.
// Jedes Element liegt unter seiner Kennung in der Y.Map „board-elements“; gleichzeitige Änderungen
// führt Excalidraw selbst zusammen (reconcileElements, Version je Element).
import { useEffect, useRef, useCallback } from 'react'
import { createRoot } from 'react-dom/client'
import { Excalidraw, MainMenu, WelcomeScreen, LiveCollaborationTrigger, CaptureUpdateAction, reconcileElements } from '@excalidraw/excalidraw'
import '@excalidraw/excalidraw/index.css'
import * as Y from 'yjs'
import { WebsocketProvider } from 'y-websocket'
import { batchRemoteUpdates } from './yjs-batch.js'

const cfg = JSON.parse(document.getElementById('nextpad-config').textContent)
const ydoc = new Y.Doc()
const wsBase = (location.protocol === 'https:' ? 'wss://' : 'ws://') + location.host + cfg.basePath + '/ws'
const provider = new WebsocketProvider(wsBase, encodeURIComponent(cfg.urlId), ydoc, { disableBc: true })
batchRemoteUpdates(provider)
const yElements = ydoc.getMap('board-elements')
const myName = cfg.author.guest ? 'Gast' : (cfg.author.name || 'Gast')
provider.awareness.setLocalStateField('user', { name: myName, color: cfg.author.color, id: cfg.author.id, avatar: cfg.author.avatar || null })
window.nextpad = { ydoc, provider, yElements }

// Startdaten erst nach dem ersten Abgleich mit dem Server (sonst überschreibt Excalidraws leerer Startzustand den Bestand)
const known = new Map() // Kennung → zuletzt gesehene Version (eigene oder fremde)
const initialData = new Promise(resolve => {
  const done = () => { const els = [...yElements.values()]; for (const el of els) known.set(el.id, el.version); resolve({ elements: els, scrollToContent: true }) }
  if (provider.synced) done(); else provider.once('sync', done)
  setTimeout(done, 8000) // ohne Verbindung trotzdem starten
})

// Pastellfarben der Autoren als Füllung, abgedunkelt als Zeigerfarbe (sonst auf Weiß kaum sichtbar)
function darker(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex || '')
  if (!m) return '#555555'
  const n = parseInt(m[1], 16)
  const c = [n >> 16, (n >> 8) & 255, n & 255].map(v => Math.round(v * 0.55).toString(16).padStart(2, '0'))
  return '#' + c.join('')
}

function rememberBoard() {
  try {
    let l = JSON.parse(localStorage.getItem('nextpad.recent') || '[]'); if (!Array.isArray(l)) l = []
    l = l.filter(e => e && e.id !== cfg.urlId)
    l.unshift({ id: cfg.urlId, t: Date.now(), title: 'Zeichenbrett' })
    localStorage.setItem('nextpad.recent', JSON.stringify(l.slice(0, 10)))
  } catch {}
}

function Board() {
  const api = useRef(null)
  const lastPointer = useRef(0)

  const applyRemote = useCallback(() => {
    const a = api.current
    if (!a) return
    const remote = [...yElements.values()]
    for (const el of remote) known.set(el.id, el.version)
    const merged = reconcileElements(a.getSceneElementsIncludingDeleted(), remote, a.getAppState())
    a.updateScene({ elements: merged, captureUpdate: CaptureUpdateAction.NEVER })
  }, [])

  const renderCollaborators = useCallback(() => {
    const a = api.current
    if (!a) return
    const m = new Map()
    provider.awareness.getStates().forEach((st, cid) => {
      if (cid === ydoc.clientID || !st.user) return
      m.set(String(cid), {
        id: st.user.id, username: st.user.name || 'Gast', avatarUrl: st.user.avatar || undefined,
        color: { background: st.user.color || '#ededed', stroke: darker(st.user.color) },
        pointer: st.pointer?.pointer, button: st.pointer?.button,
      })
    })
    a.updateScene({ collaborators: m })
  }, [])

  useEffect(() => {
    const onY = (_ev, tr) => { if (!tr.local) applyRemote() }
    const onSync = synced => { if (synced) rememberBoard() }
    yElements.observe(onY)
    provider.on('sync', onSync)
    provider.awareness.on('change', renderCollaborators)
    return () => { yElements.unobserve(onY); provider.off('sync', onSync); provider.awareness.off('change', renderCollaborators) }
  }, [applyRemote, renderCollaborators])

  const onChange = useCallback(elements => {
    if (cfg.readonly) return
    const changed = elements.filter(el => known.get(el.id) !== el.version)
    if (!changed.length) return
    ydoc.transact(() => {
      for (const el of changed) { yElements.set(el.id, JSON.parse(JSON.stringify(el))); known.set(el.id, el.version) }
    })
  }, [])

  const onPointerUpdate = useCallback(({ pointer, button }) => {
    const now = Date.now()
    if (now - lastPointer.current < 50) return
    lastPointer.current = now
    provider.awareness.setLocalStateField('pointer', { pointer, button })
  }, [])

  const share = useCallback(async () => {
    const url = `${location.origin}${cfg.basePath}/p/${encodeURIComponent(cfg.urlId)}`
    try { await navigator.clipboard.writeText(url); alert('Link kopiert:\n' + url) } catch { prompt('Link zum Zeichenbrett:', url) }
  }, [])

  const dark = window.matchMedia && matchMedia('(prefers-color-scheme: dark)').matches
  return (
    <Excalidraw
      excalidrawAPI={a => { api.current = a; window.nextpad.api = a }}
      initialData={initialData}
      onChange={onChange}
      onPointerUpdate={onPointerUpdate}
      isCollaborating={true}
      viewModeEnabled={!!cfg.readonly}
      langCode="de-DE"
      theme={dark ? 'dark' : 'light'}
      name="Zeichenbrett"
      UIOptions={{ tools: { image: false }, canvasActions: { loadScene: false, saveToActiveFile: false } }}
      renderTopRightUI={() => <LiveCollaborationTrigger isCollaborating={true} onSelect={share} />}
    >
      <MainMenu>
        <MainMenu.DefaultItems.SaveAsImage />
        <MainMenu.DefaultItems.Export />
        {!cfg.readonly && <MainMenu.DefaultItems.ClearCanvas />}
        <MainMenu.Separator />
        <MainMenu.DefaultItems.ToggleTheme />
        <MainMenu.DefaultItems.ChangeCanvasBackground />
        <MainMenu.Separator />
        <MainMenu.Item onSelect={() => { location.href = `${cfg.basePath}/` }}>Zur Startseite</MainMenu.Item>
        <MainMenu.DefaultItems.Help />
      </MainMenu>
      <WelcomeScreen>
        <WelcomeScreen.Hints.ToolbarHint />
        <WelcomeScreen.Hints.MenuHint />
        <WelcomeScreen.Center>
          <WelcomeScreen.Center.Heading>NextPad · Zeichenbrett</WelcomeScreen.Center.Heading>
        </WelcomeScreen.Center>
      </WelcomeScreen>
    </Excalidraw>
  )
}

createRoot(document.getElementById('board')).render(<Board />)
