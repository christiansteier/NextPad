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
{ const t = await bad.text(); check('Fehlerseite ohne Angabe deutsch', t.includes('lang="de"') && t.includes('nicht freigeschaltet'), t.slice(0, 300)) }
{ const r = await fetch(`${base}/lti/login?iss=https://evil.test&login_hint=1&target_link_uri=${PUBLIC}/lti/launch`, { redirect: 'manual', headers: { 'accept-language': 'en-GB,en;q=0.9' } }); const t = await r.text()
  check('Fehlerseite englisch nach Browser', r.status === 403 && t.includes('lang="en"') && t.includes('not enabled for NextPad') && t.includes('Zur Startseite') === false, t.slice(0, 300)) }
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
{ const li = await login(); const r = await fetch(`${base}/lti/launch`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded', cookie: `${li.cookie}; nextpad-lang=en` }, body: form({ error: 'login_required', state: li.state }) }); const t = await r.text()
  check('Moodle-Fehler in der eigenen Wahl englisch', r.status === 403 && t.includes('Moodle reports: not signed in'), t.slice(0, 300)) }

// erfolgreiche Anmeldung
l = await login()
const tok = jwt(claims(l.nonce))
const ok = await launch(l, tok)
const loginCookie = cookiesOf(ok).find(c => c.startsWith('nextpad_login=') && c.length > 20)
check('Anmeldung erfolgreich 303', ok.status === 303 && !!loginCookie, `Status ${ok.status}`)
const replay = await launch(l, tok)
check('state nicht wiederverwendbar 400', replay.status === 400)

// Sprache aus Moodle (launch_presentation.locale)
const LP = 'https://purl.imsglobal.org/spec/lti/claim/launch_presentation'
const sprachCookie = r => r.headers.getSetCookie().find(c => c.startsWith('nextpad-lang-moodle='))
check('ohne locale kein Sprach-Cookie', !sprachCookie(ok))
for (const [locale, want] of [['en_GB', 'nextpad-lang-moodle=en;'], ['de', 'nextpad-lang-moodle=de;'], ['fr', 'nextpad-lang-moodle=;']]) {
  const li = await login(); const r = await launch(li, jwt(claims(li.nonce, { [LP]: { locale } })))
  const c = sprachCookie(r) || ''
  check(`locale ${locale}: Sprach-Cookie ${want}`, r.status === 303 && c.startsWith(want) && /SameSite=None/.test(c) && (want.endsWith('=;') ? /Max-Age=0/.test(c) : true), c)
}

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

// Sprachwahl bei Anmeldung über Moodle: für die Mail vor dem Löschen gemerkt
const { DatabaseSync } = await import('node:sqlite')
const spracheDb = () => { const db = new DatabaseSync(path.join(dir, 'nextpad.sqlite'), { readOnly: true }); try { return db.prepare('SELECT a.sprache AS s FROM pads p JOIN authors a ON a.id=p.owner_id WHERE p.id=?').get(padId)?.s ?? null } finally { db.close() } }
check('ohne Wahl keine Sprache gemerkt', spracheDb() === null, String(spracheDb()))
const wahl = await fetch(`${base}/sprache/en?zurueck=${encodeURIComponent('/')}`, { redirect: 'manual', headers: { cookie: loginCookie } })
check('Wahl bei Anmeldung gemerkt', wahl.status === 303 && spracheDb() === 'en', `${wahl.status} ${spracheDb()}`)
{ const r = await fetch(`${base}/sprache/de?zurueck=${encodeURIComponent('/')}`, { redirect: 'manual', headers: { cookie: loginCookie, 'sec-fetch-site': 'cross-site' } })
  check('fremde Seite ändert gespeicherte Sprache nicht', r.status === 303 && spracheDb() === 'en' && /nextpad-lang=de/.test(r.headers.get('set-cookie') || ''), `${r.status} ${spracheDb()}`) }
{ const r = await fetch(`${base}/sprache/de?zurueck=${encodeURIComponent('/')}`, { redirect: 'manual', headers: { cookie: loginCookie, 'sec-fetch-site': 'same-origin' } })
  check('eigene Seite ändert gespeicherte Sprache', r.status === 303 && spracheDb() === 'de', `${r.status} ${spracheDb()}`) }

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

// Ideensammlung als Kursaktivität: eigene Gruppe je Aktivität, Zugang nur über den Start aus Moodle
const C = 'https://purl.imsglobal.org/spec/lti/claim/'
const run = Date.now().toString(36)
const act = (sub, role, rl, extra = {}) => ({ sub, name: sub === '42' ? 'Erika Muster' : 'Max Lernt', [C + 'roles']: [`http://purl.imsglobal.org/vocab/lis/v2/membership#${role}`], [C + 'context']: { id: 'kurs-' + run, title: 'Testkurs' }, [C + 'resource_link']: { id: rl, title: 'Was soll in die nächste Sitzung?' }, [C + 'custom']: { typ: 'ideen' }, ...extra })
async function startAct(over, cookie) {
  const li = await login(); const r = await launch(li, jwt(claims(li.nonce, over)), [li.cookie, cookie].filter(Boolean).join('; '))
  const sc = r.headers.getSetCookie().find(c => c.startsWith('sessionID='))
  return { r, loc: r.headers.get('location') || '', sc, sid: sc ? sc.split(';')[0] : '' }
}
const t = await startAct(act('42', 'Instructor', 'rl1-' + run))
check('Aktivität: Lehrende landen in der Sammlung', t.r.status === 303 && /\/p\/g\.[A-Za-z0-9]{16}%24aktivitaet$/.test(t.loc), t.loc)
check('Aktivität: Sitzungs-Cookie SameSite=None', /SameSite=None/.test(t.sc || '') && /Secure/.test(t.sc || '') && /HttpOnly/.test(t.sc || ''))
const apiT = base + t.loc.replace(/^\//, '/').replace(/^.*?\/p\//, '/p/') + '/ideen'
const stT = await (await fetch(apiT, { headers: { cookie: t.sid } })).json()
check('Aktivität: Lehrende moderieren, löschen nicht', stT.owner === true && stT.canDelete === false, JSON.stringify({ o: stT.owner, d: stT.canDelete }))
check('Aktivität: Frage aus dem Titel', stT.prompt === 'Was soll in die nächste Sitzung?')
const s1 = await startAct(act('43', 'Learner', 'rl1-' + run))
check('Aktivität: Studierende landen in derselben Sammlung', s1.loc === t.loc)
const stS = await (await fetch(apiT, { headers: { cookie: s1.sid } })).json()
check('Aktivität: Studierende moderieren nicht', stS.owner === false && stS.canWrite === true)
check('Aktivität: ohne Start aus Moodle kein Zugang', (await fetch(apiT)).status === 403 && (await fetch(apiT.replace(/\/ideen$/, ''))).status === 403)
const addS = await fetch(apiT, { method: 'POST', headers: { cookie: s1.sid, origin, 'content-type': 'application/json' }, body: JSON.stringify({ text: 'Mehr Übungen' }) })
check('Aktivität: Studierende tragen ein', addS.ok)
const modS = await fetch(apiT + '/1/moderation', { method: 'POST', headers: { cookie: s1.sid, origin, 'content-type': 'application/json' }, body: '{"done":true}' })
check('Aktivität: Moderation durch Studierende 403', modS.status === 403)
const modT = await fetch(apiT + '/1/moderation', { method: 'POST', headers: { cookie: t.sid, origin, 'content-type': 'application/json' }, body: '{"done":true}' })
check('Aktivität: Moderation durch Lehrende', modT.ok)
const delT = await fetch(apiT.replace(/\/ideen$/, '/delete'), { method: 'POST', headers: { cookie: t.sid, origin } })
check('Aktivität: Löschen über NextPad 403', delT.status === 403)
const s2 = await startAct(act('43', 'Learner', 'rl2-' + run), s1.sid)
check('Aktivität: zweite Aktivität eigene Sammlung', s2.r.status === 303 && s2.loc !== t.loc)
check('Aktivität: Cookie hält beide Sitzungen', s2.sid.includes(',') && (await fetch(apiT, { headers: { cookie: s2.sid } })).ok)
const s3 = await startAct({ ...act('43', 'Learner', 'rl3-' + run, { [C + 'custom']: {} }), [C + 'target_link_uri']: `${PUBLIC}/lti/ideen` })
check('Aktivität: auch über die Zieladresse …/lti/ideen', s3.r.status === 303 && s3.loc.includes('aktivitaet'))
// Text-Pad als Kursaktivität
const tp = await startAct(act('42', 'Instructor', 'rl5-' + run, { [C + 'custom']: { typ: 'pad' } }))
check('Text-Pad-Aktivität: Lehrende landen im Pad', tp.r.status === 303 && /%24aktivitaet$/.test(tp.loc) && tp.loc !== t.loc, tp.loc)
const padUrl = base + tp.loc.replace(/^.*?\/p\//, '/p/')
const padPage = await (await fetch(padUrl, { headers: { cookie: tp.sid } })).text()
check('Text-Pad-Aktivität: Editor, keine Sammlung und kein Quiz', padPage.includes('nextpad-config') && !padPage.includes('ideen.js') && !padPage.includes('quiz.js'))
const sp = await startAct(act('43', 'Learner', 'rl5-' + run, { [C + 'custom']: { typ: 'pad' } }))
check('Text-Pad-Aktivität: Studierende im selben Pad, mit Schreibrecht', sp.loc === tp.loc && !(await (await fetch(padUrl, { headers: { cookie: sp.sid } })).text()).includes('"readonly":true'))
check('Text-Pad-Aktivität: ohne Start aus Moodle kein Zugang', (await fetch(padUrl)).status === 403)
check('Text-Pad-Aktivität: Export mit Sitzung', (await fetch(padUrl + '/export/md', { headers: { cookie: sp.sid } })).ok)
const tp2 = await startAct({ ...act('43', 'Learner', 'rl6-' + run, { [C + 'custom']: {} }), [C + 'target_link_uri']: `${PUBLIC}/lti/pad` })
check('Text-Pad-Aktivität: auch über die Zieladresse …/lti/pad', tp2.r.status === 303 && tp2.loc.includes('aktivitaet') && tp2.loc !== tp.loc)
// Quiz als Kursaktivität: Lehrende leiten, Studierende bekommen Teamwahl und Buzzer
const tq = await startAct(act('42', 'Instructor', 'rl7-' + run, { [C + 'custom']: { typ: 'quiz' } }))
const quizUrl = base + tq.loc.replace(/^.*?\/p\//, '/p/')
const postQ = (u, cookie, body) => fetch(u, { method: 'POST', headers: { cookie, origin, 'content-type': 'application/json' }, body: JSON.stringify(body) })
const setupQ = await postQ(quizUrl + '/quiz/fragen', tq.sid, { title: 'Wiederholung', teams: 'Rot, Blau', table: '| Audio | 100 | Wie lang? | 5 Minuten |' })
check('Quiz-Aktivität: Lehrende richten ein', setupQ.ok, String(setupQ.status))
check('Quiz-Aktivität: Studierende können das Spiel nicht starten', (await postQ(quizUrl + '/quiz/spiel', (await startAct(act('43', 'Learner', 'rl7-' + run, { [C + 'custom']: { typ: 'quiz' } }))).sid, { aktion: 'start' })).status === 403)
check('Quiz-Aktivität: vor dem Start lässt sich keine Frage öffnen', (await postQ(quizUrl + '/quiz/feld', tq.sid, { field: 1 })).status === 409)
check('Quiz-Aktivität: Lehrende starten das Spiel', (await postQ(quizUrl + '/quiz/spiel', tq.sid, { aktion: 'start' })).ok)
const sq = await startAct(act('43', 'Learner', 'rl7-' + run, { [C + 'custom']: { typ: 'quiz' } }))
const stQ = await (await fetch(quizUrl + '/quiz', { headers: { cookie: sq.sid } })).json()
check('Quiz-Aktivität: Studierende leiten nicht, sehen Teams, keine Antworten', sq.loc === tq.loc && stQ.lead === false && stQ.teams.length === 2 && !JSON.stringify(stQ).includes('5 Minuten'))
check('Quiz-Aktivität: Studierende können nicht werten', (await postQ(quizUrl + '/quiz/feld', sq.sid, { field: 1 })).status === 403)
const pageQ = await (await fetch(quizUrl, { headers: { cookie: sq.sid } })).text()
check('Quiz-Aktivität: Seite mit Buzzer und Teamwahl', pageQ.includes('id="buzz-btn"') && pageQ.includes('id="buzz-wahl"'))
check('ohne Kursnummer kein „Zurück zum Kurs“', pageQ.includes('"course":null'))
const kursNr = String(4000 + Math.floor(Math.random() * 1000))
for (const [typ, name] of [['pad', 'Text'], ['quiz', 'Quiz'], ['ideen', 'Ideensammlung']]) {
  const sk = await startAct(act('43', 'Learner', `rk-${typ}-` + run, { [C + 'context']: { id: kursNr, title: 'Kurs mit Nummer' }, [C + 'custom']: { typ } }))
  const pk = await (await fetch(base + sk.loc.replace(/^.*?\/p\//, '/p/'), { headers: { cookie: sk.sid } })).text()
  check(`${name}-Aktivität: „Zurück zum Kurs“ führt auf die Kursseite`, pk.includes(`"course":"${ISS}/course/view.php?id=${kursNr}"`), sk.loc)
}
await postQ(quizUrl + '/quiz/feld', tq.sid, { field: 1 })
const zeitQ = await (await fetch(quizUrl + '/messung/zeit', { headers: { cookie: sq.sid } })).json()
const buzzQ = await postQ(quizUrl + '/quiz/buzzer', sq.sid, { dev: 'lerngeraet01', team: 2, field: 1, t: zeitQ.t, off: 0, rtt: 20 })
check('Quiz-Aktivität: Studierende buzzern', buzzQ.ok && (await buzzQ.json()).neu === true)
const leadQ = await (await fetch(quizUrl + '/quiz', { headers: { cookie: tq.sid } })).json()
check('Quiz-Aktivität: Leitung sieht die Reihenfolge', leadQ.buzz && leadQ.buzz.reihe[0]?.name === 'Blau', JSON.stringify(leadQ.buzz))
check('Quiz-Aktivität: ohne Start aus Moodle kein Buzzer', (await fetch(quizUrl + '/quiz/buzzer', { method: 'POST', headers: { origin, 'content-type': 'application/json' }, body: '{}' })).status === 403)
// QR-Code einer Moodle-Aktivität führt zur Aktivität in Moodle
const nr = String(Date.now() % 1000000)
const tm = await startAct(act('42', 'Instructor', nr, { [C + 'custom']: { typ: 'quiz' } }))
const qrPage = await (await fetch(base + tm.loc.replace(/^.*?\/p\//, '/p/'), { headers: { cookie: tm.sid } })).text()
check('Moodle-Quiz: QR-Code zeigt auf die Aktivität in Moodle', qrPage.includes(`${ISS}/mod/lti/view.php?l=${nr}`) && qrPage.includes('Mitmachen über Moodle') && qrPage.includes('<svg'))
const qrMess = await (await fetch(base + tm.loc.replace(/^.*?\/p\//, '/p/') + '/messung', { headers: { cookie: tm.sid } })).text()
check('Moodle-Quiz: auch auf der Messseite', qrMess.includes(`${ISS}/mod/lti/view.php?l=${nr}`))
const ti = await startAct(act('42', 'Instructor', String(Number(nr) + 1)))
const wall = await (await fetch(base + ti.loc.replace(/^.*?\/p\//, '/p/') + '/leinwand', { headers: { cookie: ti.sid } })).text()
check('Moodle-Ideensammlung: Leinwand mit QR zur Aktivität', wall.includes(`${ISS}/mod/lti/view.php?l=${Number(nr) + 1}`))
const s4 = await startAct({ ...act('43', 'Learner', 'x'), [C + 'resource_link']: undefined })
check('Aktivität ohne Ressourcen-Link 400', s4.r.status === 400, `Status ${s4.r.status}`)

fs.writeFileSync(path.join(dir, 'testpad.txt'), padId)
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`)
process.exit(fail ? 1 : 0)
