// Addons ein- und ausschalten. Zwei Läufe gegen denselben Datenbestand:
//   node test/addontest.mjs anlegen <basis-url> <datei>   Server mit allen Addons: legt Quiz, Ideensammlung und Zeichenbrett an
//   node test/addontest.mjs pruefen <basis-url> <datei>   Server mit NEXTPAD_ADDONS=ideen: Quiz und Zeichenbrett sind aus
// Beide Server mit NEXTPAD_THEME=hsd (nur dessen Startseite hat Kacheln) und NEXTPAD_ALLOW_FREE_PADS=true.
import { readFileSync, writeFileSync } from 'node:fs'
const [mode, base, file] = process.argv.slice(2)
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const neu = async typ => { const r = await fetch(`${base}/new?typ=${typ}`, { redirect: 'manual' }); return { status: r.status, pad: decodeURIComponent((r.headers.get('location') || '').split('/p/')[1] || ''), cookie: (r.headers.get('set-cookie') || '').split(';')[0] } }
const get = async (p, cookie = '') => { const r = await fetch(base + p, { headers: { cookie } }); return { status: r.status, text: await r.text() } }

if (mode === 'anlegen') {
  const quiz = await neu('quiz'), ideen = await neu('ideen'), brett = await neu('zeichenbrett')
  check('alle Addons an: Pads angelegt', [quiz, ideen, brett].every(x => x.status === 303 && x.pad), JSON.stringify([quiz.status, ideen.status, brett.status]))
  const table = '| Kategorie | Punkte | Frage | Antwort |\n| --- | --- | --- | --- |\n| A | 100 | F | X |'
  const r = await fetch(`${base}/p/${quiz.pad}/quiz/fragen`, { method: 'POST', headers: { 'Content-Type': 'application/json', Origin: new URL(base).origin, cookie: quiz.cookie }, body: JSON.stringify({ title: 'Aus-Probe', teams: 'Rot', table }) })
  check('Quiz mit Fragen', r.ok, String(r.status))
  const start = await get('/')
  check('Startseite: alle drei Kacheln', ['typ=ideen', 'typ=quiz', 'typ=zeichenbrett'].every(k => start.text.includes(k)))
  writeFileSync(file, JSON.stringify({ quiz: quiz.pad, quizCookie: quiz.cookie, ideen: ideen.pad, brett: brett.pad }))
} else {
  const p = JSON.parse(readFileSync(file, 'utf8'))
  const start = await get('/')
  check('Startseite: nur die Ideensammlung', start.text.includes('typ=ideen') && !start.text.includes('typ=quiz') && !start.text.includes('typ=zeichenbrett'))
  const hilfe = await get('/hilfe')
  check('Hilfe: Ideensammlung ja, Quiz und Zeichenbrett nein', hilfe.text.includes('<h2>Ideensammlung</h2>') && !hilfe.text.includes('<h2>Quiz-Raster</h2>') && !hilfe.text.includes('<h2>Zeichenbrett</h2>'))
  const nq = await neu('quiz')
  check('neues Quiz: abgelehnt, kein Pad angelegt', nq.status === 404 && !nq.pad, String(nq.status))
  const nu = await neu('umfrage')
  check('unbekannter Typ: 404', nu.status === 404, String(nu.status))
  const ni = await neu('ideen')
  check('neue Ideensammlung geht', ni.status === 303 && !!ni.pad, String(ni.status))
  const seite = await get(`/p/${p.quiz}`)
  check('bestehendes Quiz: Hinweis „nicht eingeschaltet“', seite.status === 404 && seite.text.includes('nicht eingeschaltet'), String(seite.status))
  check('Quiz-Schnittstelle weg', (await get(`/p/${p.quiz}/quiz`)).status === 404 && (await get('/static/quiz.js')).status === 404 && (await get(`/p/${p.quiz}/messung`)).status === 404)
  const ex = await get(`/p/${p.quiz}/export/md`)
  check('Quiz-Export: Hinweis statt Fragen', ex.status === 404 && ex.text.includes('nicht eingeschaltet') && !ex.text.includes('| A |'))
  check('bestehendes Zeichenbrett: Hinweis, Bündel weg', (await get(`/p/${p.brett}`)).text.includes('nicht eingeschaltet') && (await get('/static/board/board.js')).status === 404)
  const ideen = await get(`/p/${p.ideen}`)
  check('Ideensammlung läuft weiter', ideen.status === 200 && ideen.text.includes('/static/ideen.js'), String(ideen.status))
  check('Ideen-Schnittstelle da', (await get(`/p/${p.ideen}/ideen`)).status === 200)
  const np = await neu('')
  const text = await get(`/p/${np.pad}`, np.cookie)
  check('Text-Pad läuft', np.status === 303 && text.status === 200 && text.text.includes('/static/editor.js'), `${np.status} ${text.status}`)
}
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
