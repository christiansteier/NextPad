## Files

| Location | Content |
|---|---|
| `__DATA_DIR__/data/` | Database (SQLite with WAL) |
| `__DATA_DIR__/secrets/api-keys.txt` | API keys for Moodle; line: `<key> [https://moodle-address]` (with an address, NextPad shows profile pictures from that Moodle) |
| `__DATA_DIR__/secrets/lti-platforms.json` | Optional: Moodle instances for sign-in through LTI 1.3 (format in the project's `server/README.md`) |
| `__DATA_DIR__/theme/` | Optional: `logo.png`, `font.ttf`, `background.jpg` for the HSD look (not in the package, requires permission) |
| `__INSTALL_DIR__/nextpad.env` | Generated settings; change them through the configuration panel |

After changing files in `secrets/` or `theme/`: `systemctl restart __APP__`.

## Set up Moodle

Etherpad Lite plugin (Site administration › Plugins › Activity modules › Etherpad Lite):

- Server URL: `https://__DOMAIN____PATH__/` (with a trailing slash)
- API key: first line of `secrets/api-keys.txt`
- Enter the source address of the Moodle server in the configuration panel under "Source addresses".
- Add the Moodle address under "Pages allowed to embed NextPad".

## Maintenance

```bash
sudo yunohost app shell __APP__        # the app's environment
systemctl status __APP__ ; journalctl -u __APP__ -f
```

Merging the update log and a dry run of retention by hand (with the service's settings):

```bash
sudo -u __APP__ bash -c 'set -a; . __INSTALL_DIR__/nextpad.env; __INSTALL_DIR__/nextpad compact'
sudo -u __APP__ bash -c 'set -a; . __INSTALL_DIR__/nextpad.env; NEXTPAD_RETENTION=dry-run __INSTALL_DIR__/nextpad retention-run'
```

The interface is in German and English; each person switches with "DE | EN". The email before deletion uses the language the owner last chose, otherwise both.
