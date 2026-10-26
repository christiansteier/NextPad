// Abmelden auf drei Wegen: Startseite, Pad (Teilnehmende), Adresse /logout
import { chromium } from 'playwright'
import { execSync } from 'node:child_process'
const base = 'http://127.0.0.1:5093', db = process.env.NEXTPAD_DB || process.env.HOME + '/.cache/nextpad-data-5093/nextpad.sqlite'
const login = () => execSync(`sqlite3 ${db} "insert or replace into logins (token, author_id, created, valid_until) values ('testlogin0123456789abcdef','a.7GWLCPklETZvcgvM',0,99999999999);"`)
const b = await chromium.launch(); let ok = 0, fail = 0
const check = (n, c) => { c ? ok++ : fail++; console.log((c ? 'ok   ' : 'FAIL ') + n) }
async function ctx() { login(); const c = await b.newContext(); await c.addCookies([{ name: 'nextpad_login', value: 'testlogin0123456789abcdef', domain: '127.0.0.1', path: '/' }]); return c.newPage() }
let p = await ctx(); await p.goto(base + '/')
check('Startseite zeigt Anmeldung', await p.locator('text=Über Moodle angemeldet als').count() === 1)
check('Vorlagen aufgeklappt', await p.locator('.new-pad[open]').count() === 1 && await p.locator('a[href$="vorlage=protokoll"]').isVisible())
await p.click('button:has-text("Abmelden")'); await p.waitForLoadState()
check('Startseite: abgemeldet', await p.locator('text=Über Moodle angemeldet als').count() === 0)
p = await ctx(); await p.goto(base + '/open?padname=abmeldetest'); await p.waitForSelector('.ProseMirror')
await p.click('button[title="Teilnehmende"]')
check('Pad zeigt Abmelden', await p.locator('.popup .login button').isVisible())
await p.click('.popup .login button'); await p.waitForURL(base + '/')
check('Pad: abgemeldet, zurück zur Startseite', await p.locator('text=Über Moodle angemeldet als').count() === 0)
p = await ctx(); const r = await p.goto(base + '/logout')
check('GET /logout zeigt Seite statt 405', r.status() === 200 && await p.locator('h1:has-text("Abmelden")').count() === 1)
await p.click('main button:has-text("Abmelden")'); await p.waitForURL(base + '/')
check('/logout: abgemeldet', await p.locator('text=Über Moodle angemeldet als').count() === 0)
await b.close(); console.log(`${ok} ok, ${fail} fehlgeschlagen`)
