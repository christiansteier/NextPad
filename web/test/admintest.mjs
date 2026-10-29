// Verwaltung Erscheinungsbild: Zugang nur für Admins (Moodle-Rolle oder NEXTPAD_ADMINS), Speichern, Prüfungen, Wirkung, Zurücksetzen.
// Server wie für ltitest, im Design hsd und mit NEXTPAD_ADMINS=admin@example.org.
// node test/admintest.mjs <datadir> http://127.0.0.1:<port>
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'
import { chromium } from 'playwright'
import AxeBuilder from '@axe-core/playwright'

const [dir, base] = process.argv.slice(2)
const ISS = 'https://moodle.test', CLIENT = 'testclient123', DEP = '7', PUBLIC = 'https://nextpad.test'
const key = crypto.createPrivateKey(fs.readFileSync(path.join(dir, 'private.pem')))
const b64 = o => Buffer.from(JSON.stringify(o)).toString('base64url')
const jwt = c => { const h = b64({ alg: 'RS256', typ: 'JWT', kid: 'k1' }), p = b64(c); return `${h}.${p}.${crypto.sign('sha256', Buffer.from(`${h}.${p}`), key).toString('base64url')}` }
const cookiesOf = r => r.headers.getSetCookie().map(c => c.split(';')[0])
const form = o => new URLSearchParams(o).toString()
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }

/// Anmeldung über die nachgespielte Plattform; liefert das Anmelde-Cookie.
async function loginAs(sub, email, roles = []) {
  const r = await fetch(`${base}/lti/login`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: form({ iss: ISS, login_hint: sub, target_link_uri: `${PUBLIC}/lti/launch`, client_id: CLIENT, lti_deployment_id: DEP }) })
  const loc = new URL(r.headers.get('location')), bind = cookiesOf(r).find(c => c.startsWith('nextpad_lti='))
  const now = Math.floor(Date.now() / 1000)
  const token = jwt({ iss: ISS, aud: CLIENT, sub, exp: now + 300, iat: now, nonce: loc.searchParams.get('nonce'), name: `Person ${sub}`, email,
    'https://purl.imsglobal.org/spec/lti/claim/deployment_id': DEP, 'https://purl.imsglobal.org/spec/lti/claim/message_type': 'LtiResourceLinkRequest',
    'https://purl.imsglobal.org/spec/lti/claim/version': '1.3.0', 'https://purl.imsglobal.org/spec/lti/claim/roles': roles })
  const l = await fetch(`${base}/lti/launch`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded', cookie: bind }, body: form({ id_token: token, state: loc.searchParams.get('state') }) })
  return cookiesOf(l).find(c => c.startsWith('nextpad_login='))
}
const get = (p, cookie) => fetch(base + p, { redirect: 'manual', headers: cookie ? { cookie } : {} })
function save(cookie, fields, files = {}, origin) {
  const fd = new FormData()
  for (const [k, v] of Object.entries(fields)) fd.append(k, v)
  for (const [k, [bytes, type]] of Object.entries(files)) fd.append(k, new Blob([bytes], { type }), 'datei')
  return fetch(`${base}/admin/erscheinungsbild`, { method: 'POST', redirect: 'manual', body: fd, headers: { cookie, ...(origin ? { origin } : {}) } })
}

const user = await loginAs('51', 'lehrende@example.org', ['http://purl.imsglobal.org/vocab/lis/v2/membership#Instructor'])
const admin = await loginAs('52', 'chefin@example.org', ['http://purl.imsglobal.org/vocab/lis/v2/institution/person#Administrator'])
const listed = await loginAs('53', 'Admin@Example.org')
check('ohne Anmeldung 403', (await get('/admin/erscheinungsbild')).status === 403)
check('Lehrende ohne Admin-Recht 403', (await get('/admin/erscheinungsbild', user)).status === 403)
check('Moodle-Administration 200', (await get('/admin/erscheinungsbild', admin)).status === 200)
check('NEXTPAD_ADMINS (Mail, Groß/klein egal) 200', (await get('/admin/erscheinungsbild', listed)).status === 200)
check('Startseite: Verweis nur für Admins', (await (await get('/', admin)).text()).includes('/admin/erscheinungsbild') && !(await (await get('/', user)).text()).includes('/admin/erscheinungsbild'))
check('Speichern als Lehrende 403', (await save(user, { product: 'X' })).status === 403)
check('Speichern von fremder Seite 403', (await save(admin, { product: 'X' }, {}, 'https://evil.test')).status === 403)

// Englisch: Seite, Umschalter, Abweisung, Fehlermeldung beim Speichern
const en = { 'accept-language': 'en-GB' }
const enPage = await (await fetch(base + '/admin/erscheinungsbild', { headers: { cookie: admin, ...en } })).text()
check('Seite englisch mit Umschalter', enPage.includes('<html lang="en"') && enPage.includes('<h1>Appearance</h1>') && enPage.includes('Reset everything to the default') && enPage.includes('class="sprachwahl"') && !enPage.includes('Speichern'))
check('Seite deutsch', (await (await get('/admin/erscheinungsbild', admin)).text()).includes('<h1>Erscheinungsbild</h1>'))
const enDenied = await fetch(base + '/admin/erscheinungsbild', { headers: { cookie: user, ...en } })
check('Abweisung englisch', enDenied.status === 403 && (await enDenied.text()).includes('This page is for admins only.'))
const enBad = new FormData(); enBad.append('accent', 'rot')
const enBadText = await (await fetch(base + '/admin/erscheinungsbild', { method: 'POST', body: enBad, headers: { cookie: admin, ...en } })).text()
check('Fehlermeldung englisch', enBadText.includes('Not saved.') && enBadText.includes('is not a colour in the format #rrggbb.'))

const bad = await save(admin, { accent: 'rot', product: 'Nicht' })
check('ungültige Farbe: 400 mit Meldung', bad.status === 400 && (await bad.text()).includes('Nicht gespeichert'))
const evil = await save(admin, {}, { logo_file: [Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>'), 'image/svg+xml'] })
check('SVG mit Skript abgelehnt', evil.status === 400)
check('nach Fehler unverändert', !(await (await get('/')).text()).includes('Nicht'))

const svg = Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><rect width="40" height="20" fill="#007a96"/></svg>')
const ok = await save(admin, { product: 'Testpads', tagline: 'Schreiben im Test', institution: 'Testhochschule', accent: '#007A96', color1: '#7fd4ff', logo: 'vorgabe', background: 'vorgabe' }, { logo_file: [svg, 'image/svg+xml'] })
check('Speichern 303 mit Rückmeldung', ok.status === 303 && ok.headers.get('location').endsWith('?gespeichert=1'))
const start = await (await get('/')).text()
check('Name, Untertitel, Einrichtung im Kopf', start.includes('<strong>Testpads</strong>') && start.includes('Schreiben im Test') && start.includes('alt="Testhochschule"'))
check('eingetragener Untertitel bleibt auf Englisch unübersetzt', (await (await fetch(base + '/', { headers: en })).text()).includes('Schreiben im Test'))
check('Farben als Stilangaben', start.includes('--hsd-red:#007a96') && start.includes('--hsd-blue:#7fd4ff') && /--line-blue:#[0-9a-f]{6}/.test(start))
const logo = await get('/theme/logo.png')
check('eigenes Logo als SVG, abgeschottet', logo.status === 200 && logo.headers.get('content-type') === 'image/svg+xml' && (logo.headers.get('content-security-policy') || '').includes('sandbox'))
check('kein NextPad-Zeichen neben eigenem Logo', !start.includes('brand-mark'))
const page = await (await get('/admin/erscheinungsbild', admin)).text()
check('Seite zeigt Kontrasthinweis bei hellem Blau nicht für Linie', !page.includes('Farbe 1, Linie'))

// Wirkung im Pad und im PDF
const pad = 'Erscheinungstest' + Date.now().toString().slice(-5)
const padHtml = await (await fetch(`${base}/open?padname=${pad}`)).text()
check('Editor übernimmt Farben', padHtml.includes('--primary:#007a96'))
const pdf = await get(`/p/${pad}/export/pdf`)
check('PDF mit SVG-Logo', pdf.status === 200 && (await pdf.arrayBuffer()).byteLength > 1000)
// Logo oben links im Pad führt zur Startseite
{
  const lb = await chromium.launch(); const lp = await (await lb.newContext({ locale: 'de-DE' })).newPage()
  await lp.goto(`${base}/p/${pad}`); await lp.waitForSelector('.ProseMirror')
  const home = lp.locator('#toolbar a.toolbar-home')
  const box = await home.count() ? await home.boundingBox() : null
  check('Pad: Logo ist Verweis zur Startseite', await home.count() === 1 && (await home.getAttribute('aria-label')) === 'Testhochschule, Startseite' && await home.locator('img.toolbar-logo').count() === 1 && box.height >= 44 && box.width >= 44, box && `${box.width}x${box.height}`)
  await home.click(); await lp.waitForURL(u => new URL(u).pathname.replace(/\/$/, '') === new URL(base).pathname.replace(/\/$/, ''), { timeout: 5000 }).catch(() => {})
  check('Pad: Klick aufs Logo öffnet die Startseite', new URL(lp.url()).pathname.replace(/\/$/, '') === new URL(base).pathname.replace(/\/$/, '') && (await lp.locator('main').count()) === 1, lp.url())
  await lb.close()
}

// kein Logo: Zeichen und Name als Verweis
await save(admin, { product: 'Testpads', logo: 'keins', background: 'vorgabe' })
const noLogo = await (await get('/')).text()
check('Kein Logo: Zeichen und Name verlinkt', noLogo.includes('class="product product-link"') && noLogo.includes('brand-mark') && !noLogo.includes('class="logo"'))

// Bedienung im Browser: Farbwähler füllt das Textfeld, eigene Abstufungen erst nach Haken
const b = await chromium.launch(); const ctx = await b.newContext()
await ctx.addCookies([{ name: 'nextpad_login', value: admin.split('=')[1], url: base }])
const p = await ctx.newPage(); await p.goto(base + '/admin/erscheinungsbild')
for (const scheme of ['light', 'dark']) {
  await p.emulateMedia({ colorScheme: scheme }); await p.waitForTimeout(400)
  const axe = await new AxeBuilder({ page: p }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze()
  check(`axe ${scheme}`, axe.violations.length === 0, axe.violations.map(v => v.id + ': ' + v.nodes.slice(0, 4).map(n => n.target.join(' ') + ' ' + (n.any[0]?.message || '')).join(' | ')).join('; '))
  if (process.env.SHOT_DIR) await p.screenshot({ path: `${process.env.SHOT_DIR}/admin-${scheme}.png`, fullPage: true })
}
await p.emulateMedia({ colorScheme: 'light' })
check('Abstufungen anfangs verborgen', await p.locator('#shades').isHidden())
await p.check('#f-custom'); check('Abstufungen nach Haken sichtbar', await p.locator('#shades').isVisible())
await p.locator('#c-color2').evaluate(e => { e.value = '#123456'; e.dispatchEvent(new Event('input')) })
check('Farbwähler füllt Textfeld', await p.inputValue('#f-color2') === '#123456')
check('jedes Feld hat einen Namen', await p.evaluate(() => [...document.querySelectorAll('input:not([type=hidden])')].every(i => i.labels?.length || i.getAttribute('aria-label'))))
await b.close()

const reset = await fetch(`${base}/admin/erscheinungsbild/vorgabe`, { method: 'POST', redirect: 'manual', headers: { cookie: admin } })
const after = await (await get('/')).text()
check('Zurücksetzen: Vorgabe gilt wieder', reset.status === 303 && after.includes('<strong>NextPad</strong>') && !after.includes('--hsd-red:'))
const afterEn = await (await fetch(base + '/', { headers: en })).text()
check('Vorgabe-Untertitel je Sprache', after.includes('Gemeinsam schreiben an der HSD') && afterEn.includes('Writing together at HSD') && !afterEn.includes('Gemeinsam schreiben'))
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
