// LTI-Anmeldung gegen den Pilot mit echtem Moodle-Token (erzeugt per Moodle-Funktion auf dem Server).
// node ltipilot.mjs <public-url> <issuer> <client_id> <deployment_id> <user_id> <token-befehl>
// token-befehl: Shell-Befehl, der mit angehängter nonce das id_token ausgibt
import { execSync } from 'node:child_process'
const [pub, iss, client, dep, uid, cmd] = process.argv.slice(2)
let pass = 0, fail = 0
const check = (n, ok, x = '') => { ok ? pass++ : fail++; console.log(`${ok ? 'ok  ' : 'FAIL'} ${n}${ok ? '' : ' ' + x}`) }
const cookiesOf = r => r.headers.getSetCookie().map(c => c.split(';')[0])
const r1 = await fetch(`${pub}/lti/login`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded' }, body: new URLSearchParams({ iss, login_hint: uid, target_link_uri: `${pub}/lti/launch`, client_id: client, lti_deployment_id: dep, lti_message_hint: '{}' }) })
const loc = new URL(r1.headers.get('location'))
check('Anmeldestart leitet zu Moodle auth.php', r1.status === 303 && loc.href.startsWith(`${iss}/mod/lti/auth.php`), r1.status)
const state = loc.searchParams.get('state'), nonce = loc.searchParams.get('nonce'), bind = cookiesOf(r1).find(c => c.startsWith('nextpad_lti='))
const token = execSync(`${cmd} ${nonce}`, { encoding: 'utf8' }).trim().split('\n').pop()
check('Moodle erzeugt id_token', token.split('.').length === 3, token.slice(0, 40))
const claims = JSON.parse(Buffer.from(token.split('.')[1], 'base64url'))
check('Token enthält Name und Mail', !!claims.name && !!claims.email, Object.keys(claims).join(','))
const r2 = await fetch(`${pub}/lti/launch`, { method: 'POST', redirect: 'manual', headers: { 'content-type': 'application/x-www-form-urlencoded', cookie: bind }, body: new URLSearchParams({ id_token: token, state }) })
const login = cookiesOf(r2).find(c => c.startsWith('nextpad_login=') && c.length > 20)
check('Pilot nimmt echtes Moodle-Token an', r2.status === 303 && !!login, `${r2.status} ${(await r2.text()).replace(/<[^>]+>/g, ' ').slice(0, 160)}`)
if (login) {
  const start = await (await fetch(`${pub}/`, { headers: { cookie: login } })).text()
  check('Startseite zeigt angemeldete Person', start.includes(claims.name) && start.includes('Abmelden'))
  check('Profilbild aus Moodle', start.includes(`/user/pix.php/${claims.sub}/f1.jpg`))
  const lo = await fetch(`${pub}/logout`, { method: 'POST', redirect: 'manual', headers: { cookie: login, origin: new URL(pub).origin } })
  check('Abmelden', lo.status === 303)
}
console.log(`\n${pass} ok, ${fail} fehlgeschlagen`); process.exit(fail ? 1 : 0)
