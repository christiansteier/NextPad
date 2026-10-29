# NextPad for YunoHost

Deutsche Fassung: [README.de.md](README.de.md)

YunoHost package for NextPad. Install from this folder:

```bash
sudo yunohost app install ./nextpad_ynh --args "domain=pads.example.org&path=/np&init_main_permission=visitors&theme=colibris&allow_free_pads=1&addon_ideen=1&addon_quiz=1&addon_zeichenbrett=1"
```

The installation downloads the program from the GitHub releases (https://github.com/christiansteier/nextpad/releases),
file `nextpad-<version>-amd64`; version, address and checksum are in `manifest.toml`.

New release: build the binary for Debian 12 (`cargo build --release` in a
`rust:1-bookworm` container), upload it as a release file, adjust `version`, `url` and `sha256`
in the manifest, then `yunohost app upgrade nextpad -f ./nextpad_ynh`.
