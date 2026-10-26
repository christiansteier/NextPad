// LTI-1.3-Anmeldung, Besitz und Aufbewahrung gegen einen lokalen Server mit nachgespielter Plattform.
// 1) node ltitest.mjs setup <datadir>   erzeugt Schlüssel, JWKS-Datei und lti-platforms.json
// 2) Server starten mit NEXTPAD_DATA=<datadir> NEXTPAD_PUBLIC_URL=https://nextpad.test NEXTPAD_ALLOW_FREE_PADS=true …
// 3) node ltitest.mjs run <datadir> http://127.0.0.1:<port>
import crypto from 'node:crypto'
import fs from 'node:fs'
import path from 'node:path'

const [mode, dir, base] = process.argv.slice(2)
const ISS = 'https://moodle.test', CLIENT = 'testclient123', DEP = '7', PUBLIC = 'https://nextpad.test'

if (mode === 'setup') {
  fs.mkdirSync(dir, { recursive: true })
  const { publicKey, privateKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 })
  const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'k1', alg: 'RS256', use: 'sig' }
  fs.writeFileSync(path.join(dir, 'jwks.json'), JSON.stringify({ keys: [jwk] }))
  fs.writeFileSync(path.join(dir, 'private.pem'), privateKey.export({ format: 'pem', type: 'pkcs8' }))
  fs.writeFileSync(path.join(dir, 'lti-platforms.json'), JSON.stringify([{ name: 'test', issuer: ISS, client_id: CLIENT, deployment_ids: [DEP], auth_url: `${ISS}/mod/lti/auth.php`, jwks_file: path.join(dir, 'jwks.json'), start_url: `${ISS}/mod/lti/launch.php?id=1` }]))
  console.log('setup ok'); process.exit(0)
}

const key = crypto.createPrivateKey(fs.readFileSync(path.join(dir, 'private.pem')))
const b64 = o => Buffer.from(typeof o === 'string' ? o : JSON.stringify(o)).toString('base64url')
function jwt(claims, { kid = 'k1', k = key } = {}) {
  const h = b64({ alg: 'RS256', typ: 'JWT', kid }), p = b64(claims)
  return `${h}.${p}.${crypto.sign('sha256', Buffer.from(`${h}.${p}`), k).toString('base64url')}`
}
let pass = 0, fail = 0
const check = (name, ok, extra = '') => { if (ok) pass++; else fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok ? '' : ' ' + extra}`) }
const cookiesOf = r => r.headers.getSetCookie().map(c => c.split(';')[0])
const form = o => new URLSearchParams(o).toString()

async function login() {
  const r = await fetch(`${base}/lti/login`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: form({ iss: ISS, login_hint: '42', target_link_uri: `${PUBLIC}/lti/launch`, lti_message_hint: 'mh', client_id: CLIENT, lti_deployment_id: DEP }) })
  const loc = new URL(r.headers.get('location') || 'http://x/')
  return { r, loc, state: loc.searchParams.get('state'), nonce: loc.searchParams.get('nonce'), cookie: cookiesOf(r).find(c => c.startsWith('nextpad_lti=')) }
}
const now = () => Math.floor(Date.now() / 1000)
const claims = (nonce, over = {}) => ({
  iss: ISS, aud: CLIENT, sub: '42', exp: now() + 300, iat: now(), nonce, name: 'Erika Muster', email: 'erika@example.org',
  'https://purl.imsglobal.org/spec/lti/claim/deployment_id': DEP,
  'https://purl.imsglobal.org/spec/lti/claim/message_type': 'LtiResourceLinkRequest',
  'https://purl.imsglobal.org/spec/lti/claim/version': '1.3.0', ...over,
})
async function launch(l, token, cookie = l.cookie) {
  return fetch(`${base}/lti/launch`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded', ...(cookie ? { cookie } : {}) }, body: form({ id_token: token, state: l.state }) })
}

// Anmeldestart
let l = await login()
check('login leitet zu Moodle', l.r.status === 303 && l.loc.origin === ISS && l.loc.pathname === '/mod/lti/auth.php')
check('login Parameter', l.loc.searchParams.get('redirect_uri') === `${PUBLIC}/lti/launch` && l.loc.searchParams.get('client_id') === CLIENT && l.loc.searchParams.get('response_mode') === 'form_post' && l.loc.searchParams.get('lti_message_hint') === 'mh' && l.loc.searchParams.get('prompt') === 'none')
check('login setzt Bindungs-Cookie', !!l.cookie && l.r.headers.getSetCookie().some(c => /SameSite=None/.test(c) && /Secure/.test(c)))
const bad = await fetch(`${base}/lti/login?iss=https://evil.test&login_hint=1&target_link_uri=${PUBLIC}/lti/launch`, { redirect: 'manual' })
check('unbekannte Plattform 403', bad.status === 403)
const badTarget = await fetch(`${base}/lti/login?iss=${ISS}&login_hint=1&target_link_uri=https://evil.test/`, { redirect: 'manual' })
check('fremde Zieladresse 400', badTarget.status === 400)

// Fehlerfälle beim Rücksprung (jeweils eigene Anmeldung, state ist einmalig)
const cases = [
  ['falsche nonce 403', c => claims('falsch'), {}, 403],
  ['falscher Empfänger 403', c => claims(c, { aud: 'anderer' }), {}, 403],
  ['abgelaufen 403', c => claims(c, { exp: now() - 3600, iat: now() - 7200 }), {}, 403],
  ['falsches Deployment 403', c => claims(c, { 'https://purl.imsglobal.org/spec/lti/claim/deployment_id': '99' }), {}, 403],
  ['falscher Nachrichtentyp 403', c => claims(c, { 'https://purl.imsglobal.org/spec/lti/claim/message_type': 'LtiDeepLinkingRequest' }), {}, 403],
  ['fremder Schlüssel 403', c => claims(c), { k: crypto.generateKeyPairSync('rsa', { modulusLength: 2048 }).privateKey }, 403],
]
for (const [name, mk, opt, want] of cases) {
  const li = await login(); const r = await launch(li, jwt(mk(li.nonce), opt)); check(name, r.status === want, `Status ${r.status}`)
}
{ const li = await login(); const r = await launch(li, jwt(claims(li.nonce)), null); check('ohne Bindungs-Cookie 400', r.status === 400, `Status ${r.status}`) }
{ const li = await login(); const r = await fetch(`${base}/lti/launch`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded', cookie: li.cookie }, body: form({ error: 'login_required', state: li.state }) }); check('Moodle-Fehler login_required 403', r.status === 403) }

// erfolgreiche Anmeldung
l = await login()
const tok = jwt(claims(l.nonce))
const ok = await launch(l, tok)
const loginCookie = cookiesOf(ok).find(c => c.startsWith('nextpad_login=') && c.length > 20)
check('Anmeldung erfolgreich 303', ok.status === 303 && !!loginCookie, `Status ${ok.status}`)
const replay = await launch(l, tok)
check('state nicht wiederverwendbar 400', replay.status === 400)

// Startseite angemeldet
const start = await (await fetch(`${base}/`, { headers: { cookie: loginCookie } })).text()
check('Startseite zeigt Namen', start.includes('Erika Muster') && start.includes('Abmelden'))
check('Startseite zeigt Profilbild', start.includes('https://moodle.test/user/pix.php/42/f1.jpg'))
const anon = await (await fetch(`${base}/`)).text()
check('Startseite ohne Anmeldung zeigt Moodle-Knopf', anon.includes('Mit Moodle anmelden') && anon.includes('mod/lti/launch.php?id=1'))
check('Startseite ohne autofocus', !anon.includes('autofocus'))

// neues Pad als Angemeldete: Besitz, Name, Bild
const np = await fetch(`${base}/new?vorlage=protokoll`, { redirect: 'manual', headers: { cookie: loginCookie } })
const padPath = np.headers.get('location'); const padId = decodeURIComponent(padPath.split('/p/')[1])
const page = await (await fetch(`${base}/p/${encodeURIComponent(padId)}`, { headers: { cookie: loginCookie } })).text()
const cfg = JSON.parse(page.match(/<script type="application\/json" id="nextpad-config">(.*?)<\/script>/s)[1].replace(/<\\\//g, '</'))
check('Pad: angemeldet, Besitzer, Name, Bild', cfg.login === true && cfg.owner === true && cfg.author.name === 'Erika Muster' && cfg.author.avatar === 'https://moodle.test/user/pix.php/42/f1.jpg', JSON.stringify(cfg.author))
check('Pad: Frist bekannt', typeof cfg.expires === 'number' && cfg.expires > Date.now() + 300 * 86400000)
const html = await (await fetch(`${base}/p/${encodeURIComponent(padId)}/export/html`, { headers: { cookie: loginCookie } })).text()
check('Vorlage Protokoll übernommen', html.includes('Protokoll') && html.includes('Tagesordnung'))

// Besitzer-Aktionen
const origin = new URL(base).origin
const ext = await fetch(`${base}/p/${encodeURIComponent(padId)}/extend`, { method: 'POST', headers: { cookie: loginCookie, origin } })
const extJ = ext.ok ? await ext.json() : {}
check('Besitzer verlängert', ext.ok && extJ.until > cfg.expires, `Status ${ext.status}`)
const extNoOrigin = await fetch(`${base}/p/${encodeURIComponent(padId)}/extend`, { method: 'POST', headers: { cookie: loginCookie } })
check('Verlängern ohne Origin 403', extNoOrigin.status === 403)
const guestPage = await fetch(`${base}/p/${encodeURIComponent(padId)}`)
const guestCookie = cookiesOf(guestPage).find(c => c.startsWith('nextpad_author='))
const gcfg = JSON.parse((await guestPage.text()).match(/id="nextpad-config">(.*?)<\/script>/s)[1].replace(/<\\\//g, '</'))
check('Gast ist nicht Besitzer', gcfg.owner === false && gcfg.login === false)
const delGuest = await fetch(`${base}/p/${encodeURIComponent(padId)}/delete`, { method: 'POST', headers: { cookie: guestCookie, origin } })
check('Gast darf nicht löschen 403', delGuest.status === 403)

// Gast legt Pad an und besitzt es
const gp = await fetch(`${base}/open?padname=gast-test-${Date.now()}`, { redirect: 'manual' })
const gc = cookiesOf(gp).find(c => c.startsWith('nextpad_author='))
const gid = decodeURIComponent(gp.headers.get('location').split('/p/')[1])
const gcfg2 = JSON.parse((await (await fetch(`${base}/p/${encodeURIComponent(gid)}`, { headers: { cookie: gc } })).text()).match(/id="nextpad-config">(.*?)<\/script>/s)[1].replace(/<\\\//g, '</'))
check('Gast besitzt sein Pad', !!gc && gcfg2.owner === true && gcfg2.login === false)
const gdel = await fetch(`${base}/p/${encodeURIComponent(gid)}/delete`, { method: 'POST', headers: { cookie: gc, origin } })
check('Gast löscht eigenes Pad', gdel.ok)
check('gelöschtes Pad 404', (await fetch(`${base}/p/${encodeURIComponent(gid)}`)).status === 404)

// Hilfe und Verlängerungslink
check('Hilfeseite', (await fetch(`${base}/hilfe`)).ok)
check('ungültiger Verlängerungslink 404', (await fetch(`${base}/verlaengern?t=abc`)).status === 404)
check('JWKS des Werkzeugs', (await (await fetch(`${base}/lti/jwks`)).json()).keys.length === 0)

// Abmelden
const lo = await fetch(`${base}/logout`, { method: 'POST', redirect: 'manual', headers: { cookie: loginCookie, origin } })
check('Abmelden', lo.status === 303 && lo.headers.getSetCookie().some(c => c.startsWith('nextpad_login=;')))
const after = await (await fetch(`${base}/`, { headers: { cookie: loginCookie } })).text()
check('nach Abmelden nicht mehr angemeldet', !after.includes('Erika Muster'))

fs.writeFileSync(path.join(dir, 'testpad.txt'), padId)
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`)
process.exit(fail ? 1 : 0)
