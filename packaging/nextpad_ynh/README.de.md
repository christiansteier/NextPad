# NextPad für YunoHost

English version: [README.md](README.md)

YunoHost-Paket für NextPad. Installation aus diesem Ordner:

```bash
sudo yunohost app install ./nextpad_ynh --args "domain=pads.example.org&path=/np&init_main_permission=visitors&theme=colibris&allow_free_pads=1&addon_ideen=1&addon_quiz=1&addon_zeichenbrett=1"
```

Die Installation lädt das Programm aus den GitHub-Releases (https://github.com/christiansteier/nextpad/releases),
Datei `nextpad-<version>-amd64`; Version, Adresse und Prüfsumme stehen im `manifest.toml`.

Neues Release: Binary für Debian 12 bauen (`cargo build --release` in einem
`rust:1-bookworm`-Container), als Release-Datei hochladen, `version`, `url` und `sha256`
im Manifest anpassen, dann `yunohost app upgrade nextpad -f ./nextpad_ynh`.
