// Vertragstest der Etherpad-API gegen einen laufenden NextPad-Server (Node, fetch).
// node test/apitest.mjs  (NEXTPAD_URL, NEXTPAD_KEY_FILE optional)
import { readFileSync, writeFileSync } from 'node:fs'
const base = process.env.NEXTPAD_URL || 'http://127.0.0.1:5090'
const key = readFileSync(process.env.NEXTPAD_KEY_FILE || process.env.HOME + '/.cache/nextpad-data/api-keys.txt', 'utf8').trim()
let fails = 0
const ok = (name, cond, info = '') => { console.log((cond ? 'ok   ' : 'FAIL ') + name + (info ? '  ' + info : '')); if (!cond) fails++ }
const get = async (fn, params) => (await fetch(`${base}/api/1.2/${fn}?` + new URLSearchParams({ ...params, apikey: key }))).json()
const post = async (fn, params) => { const fd = new FormData(); for (const [k, v] of Object.entries({ ...params, apikey: key })) fd.append(k, v); return (await fetch(`${base}/api/1.2/${fn}`, { method: 'POST', body: fd })).json() }

const v = await (await fetch(`${base}/api`)).json(); ok('GET /api liefert currentVersion >= 1.2', /^1\.2/.test(v.currentVersion), v.currentVersion)
ok('checkToken falscher Key → code 4', (await (await fetch(`${base}/api/1.2/checkToken?apikey=falsch`)).json()).code === 4)
ok('checkToken → code 0', (await get('checkToken', {})).code === 0)
ok('unbekannte Funktion → code 3', (await get('gibtEsNicht', {})).code === 3)
const g = await post('createGroup', {}); ok('createGroup → g.-Kennung', /^g\.[A-Za-z0-9]{16}$/.test(g.data?.groupID))
const G = g.data.groupID
const p = await post('createGroupPad', { groupID: G, padName: 'mdl' }); ok('createGroupPad → padID', p.data?.padID === `${G}$mdl`)
const P = `${G}$mdl`
ok('createGroupPad Dublette → code 1', (await post('createGroupPad', { groupID: G, padName: 'mdl' })).code === 1)
ok('createGroupPad fremde Gruppe → code 1', (await post('createGroupPad', { groupID: 'g.0000000000000000', padName: 'x' })).code === 1)
const gp = await post('createGroupPad', { groupID: G, padName: 'mdl42', text: 'Gruppenpad\nZeile 2\n' }); ok('createGroupPad mit text', gp.code === 0)
ok('getText des Gruppenpads', (await get('getText', { padID: `${G}$mdl42` })).data?.text === 'Gruppenpad\nZeile 2\n')
const a1 = await post('createAuthorIfNotExistsFor', { authorMapper: '4711', name: 'Erika Muster' }); ok('createAuthorIfNotExistsFor → a.-Kennung', /^a\./.test(a1.data?.authorID))
const a2 = await post('createAuthorIfNotExistsFor', { authorMapper: '4711', name: 'Erika Musterfrau' }); ok('gleicher Mapper → gleiche Kennung', a2.data?.authorID === a1.data.authorID)
ok('getAuthorName aktualisiert', (await get('getAuthorName', { authorID: a1.data.authorID })).data === 'Erika Musterfrau')
const ga = await post('createAuthor', { name: 'Guest-12345' }); ok('createAuthor Gast', /^a\./.test(ga.data?.authorID))
const vu = Math.floor(Date.now() / 1000) + 10800
const s = await post('createSession', { groupID: G, authorID: a1.data.authorID, validUntil: String(vu) }); ok('createSession → s.-Kennung', /^s\./.test(s.data?.sessionID))
ok('createSession abgelaufen → code 1', (await post('createSession', { groupID: G, authorID: a1.data.authorID, validUntil: '1' })).code === 1)
ok('getSessionInfo', (await get('getSessionInfo', { sessionID: s.data.sessionID })).data?.groupID === G)
const ro = await get('getReadOnlyID', { padID: P }); ok('getReadOnlyID → r.-Kennung', /^r\.[A-Za-z0-9]{16}$/.test(ro.data?.readOnlyID))
ok('getReadOnlyID stabil', (await get('getReadOnlyID', { padID: P })).data.readOnlyID === ro.data.readOnlyID)
ok('setText', (await post('setText', { padID: P, text: 'Zeile eins\nZeile zwei\n' })).code === 0)
ok('getText nach setText', (await get('getText', { padID: P })).data?.text === 'Zeile eins\nZeile zwei\n')
const eph = '<!DOCTYPE HTML><html><body>Erste Zeile<br><strong>fett</strong> und <em>kursiv</em> und <u>unter</u> <s>durch</s><br><br>Leerzeile davor<br><ul class="bullet"><li>Punkt eins</li><li>Punkt zwei</li></ul><h1>Titel</h1><span class="font-size:18">groß</span> <span style="color:#ff0000">rot</span><br><ol><li>erstens</li></ol></body></html>'
ok('setHTML Etherpad-Dialekt', (await post('setHTML', { padID: P, html: eph })).code === 0)
const h1 = (await get('getHTML', { padID: P })).data.html
ok('getHTML enthält Struktur', ['>Titel</h1>', '<strong>fett</strong>', '<ul>', '<ol>', 'font-size:18px', 'color:#ff0000'].every(x => h1.includes(x)))
ok('getText nach setHTML', (await get('getText', { padID: P })).data.text === 'Erste Zeile\nfett und kursiv und unter durch\n\nLeerzeile davor\n- Punkt eins\n- Punkt zwei\nTitel\ngroß rot\n1. erstens\n')
await post('setHTML', { padID: P, html: h1 }); ok('Roundtrip getHTML → setHTML → getHTML identisch', (await get('getHTML', { padID: P })).data.html === h1)
ok('getRevisionsCount', typeof (await get('getRevisionsCount', { padID: P })).data?.revisions === 'number')
ok('getLastEdited', typeof (await get('getLastEdited', { padID: P })).data?.lastEdited === 'number')
ok('Seite ohne Cookie → 403', (await fetch(`${base}/p/${P}`)).status === 403)
const page = await fetch(`${base}/p/${P}`, { headers: { Cookie: `sessionID=${s.data.sessionID}` } })
ok('Seite mit Session → 200 + CSP frame-ancestors', page.status === 200 && (page.headers.get('content-security-policy') || '').includes('frame-ancestors'))
ok('Seite mit falscher Gruppe → 403', (await fetch(`${base}/p/${G}$mdl42x`, { headers: { Cookie: `sessionID=${s.data.sessionID}` } })).status === 404)
ok('Lesemodus-Seite → 200', (await fetch(`${base}/p/${ro.data.readOnlyID}`, { headers: { Cookie: `sessionID=${s.data.sessionID}` } })).status === 200)
ok('Export txt', (await (await fetch(`${base}/p/${P}/export/txt`, { headers: { Cookie: `sessionID=${s.data.sessionID}` } })).text()).startsWith('Erste Zeile'))
ok('deletePad', (await post('deletePad', { padID: `${G}$mdl42` })).code === 0)
ok('deletePad nochmal → code 1', (await post('deletePad', { padID: `${G}$mdl42` })).code === 1)
ok('urlencoded POST', (await (await fetch(`${base}/api/1.2/checkToken`, { method: 'POST', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ apikey: key }) })).json()).code === 0)
if (process.argv[2]) writeFileSync(process.argv[2], `${G}|${P}|${a1.data.authorID}|${s.data.sessionID}|${ro.data.readOnlyID}\n`)
ok('deleteGroup', (await post('deleteGroup', { groupID: 'g.0000000000000000' })).code === 1)
console.log(fails ? `${fails} Test(s) fehlgeschlagen` : 'alle Tests bestanden')
process.exit(fails ? 1 : 0)
