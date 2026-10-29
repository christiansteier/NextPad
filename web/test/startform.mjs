// Startseite: leeres Feld legt ein Pad mit Zufallsname an, Feldprüfung (ungültig, g.-Präfix), gültiger Name öffnet das Pad, Vorlagen und Zeichenbrett legen Pads an,
// Fehlerseite des Servers mit Weg zurück. node test/startform.mjs <basis-url>
import { chromium } from 'playwright'
const base = process.argv[2] || 'http://127.0.0.1:5093'
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const b = await chromium.launch(); const p = await (await b.newContext({ locale: 'de-DE' })).newPage()
await p.goto(base + '/')
check('Fehlerfeld anfangs unsichtbar', await p.locator('#padname-error').isHidden())
await Promise.all([p.waitForURL(/\/p\/[^/]+$/), p.click('button[type=submit]')])
check('leer: Pad mit Zufallsname angelegt', /\/p\/[A-Za-z0-9]{16}$/.test(p.url()) && await p.locator('.ProseMirror').count() === 1, p.url())
await p.goto(base + '/')
await p.fill('#padname', 'a/b<c>'); await p.press('#padname', 'Enter')
check('ungültige Zeichen: Meldung', (await p.textContent('#padname-error')).includes('Erlaubt'))
await p.fill('#padname', 'g.test'); await p.press('#padname', 'Enter')
check('g.-Präfix: Meldung', (await p.textContent('#padname-error')).includes('g.'))
check('Fokus im Feld nach Fehler', await p.evaluate(() => document.activeElement.id) === 'padname')
const name = 'Redesign Test ' + Date.now().toString().slice(-5)
await p.fill('#padname', name); await p.press('#padname', 'Enter'); await p.waitForURL(/\/p\//)
check('gültiger Name öffnet Pad', decodeURIComponent(p.url()).endsWith('/p/' + name))
for (const [label, sel, test] of [['Gruppenarbeit', 'a[href$="vorlage=gruppenarbeit"]', 'Wer macht was'], ['Protokoll', 'a[href$="vorlage=protokoll"]', 'Tagesordnung']]) {
  await p.goto(base + '/'); await p.click(sel); await p.waitForSelector('.ProseMirror')
  await p.waitForFunction(t => document.querySelector('.ProseMirror')?.textContent.includes(t), test, { timeout: 8000 }).catch(() => {})
  check(`Vorlage ${label}`, (await p.textContent('.ProseMirror')).includes(test))
}
await p.goto(base + '/'); await p.click('a[href$="typ=ideen"]'); await p.waitForSelector('#add-form:not([hidden])', { timeout: 8000 }).catch(() => {})
check('Ideensammlung', await p.locator('#add-form:not([hidden])').count() === 1)
await p.goto(base + '/'); await p.click('a[href$="typ=zeichenbrett"]'); await p.waitForSelector('.excalidraw', { timeout: 20000 }).catch(() => {})
check('Zeichenbrett', await p.locator('.excalidraw').count() > 0)
await p.goto(base + '/open?padname=%3Cx%3E')
check('Serverfehlerseite mit Weg zurück', (await p.textContent('h1')).length > 0 && await p.locator('a[href$="/"]').count() > 0)
await p.goto(base + '/')
const icon = await p.getAttribute('link[rel=icon][type="image/svg+xml"]', 'href')
const svg = icon ? await p.request.get(new URL(icon, base).href) : null
check('Favicon verlinkt und ausgeliefert', !!svg && svg.ok() && (await svg.text()).includes('<svg'))
check('kein NextPad-Zeichen neben einem Theme-Logo', await p.locator('.site-header .logo').count() === 0 || await p.locator('.site-header .brand-mark').count() === 0)
await p.goto(base + '/hilfe')
check('Hilfe ohne Löschzusage im Probelauf', !(await p.textContent('main')).includes('Tagen ohne Öffnen'))
await b.close(); console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
