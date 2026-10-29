#!/bin/bash
# Gemeinsame Funktionen des NextPad-Pakets

# Standardwerte der Einstellungen (Konfigurationsfenster), nur gesetzt, wenn noch leer
nextpad_settings_defaults() {
    ynh_app_setting_set_default --key=api_allowed --value="127.0.0.1,::1"
    ynh_app_setting_set_default --key=frame_ancestors --value="'self'"
    ynh_app_setting_set_default --key=theme --value="colibris"
    ynh_app_setting_set_default --key=allow_free_pads --value="1"
    ynh_app_setting_set_default --key=free_pads_per_hour --value="20"
    ynh_app_setting_set_default --key=retention --value="off"
    ynh_app_setting_set_default --key=history_days --value="30"
    ynh_app_setting_set_default --key=site_name --value=""
    ynh_app_setting_set_default --key=footer_links --value=""
    ynh_app_setting_set_default --key=background_credit --value=""
    ynh_app_setting_set_default --key=background_credit_url --value=""
    ynh_app_setting_set_default --key=admins --value=""
    # Addons: bestehende Installationen behalten alle
    ynh_app_setting_set_default --key=addon_ideen --value="1"
    ynh_app_setting_set_default --key=addon_quiz --value="1"
    ynh_app_setting_set_default --key=addon_zeichenbrett --value="1"
}

# Umgebungsdatei aus den Einstellungen schreiben
nextpad_write_config() {
    if [ "$path" = "/" ]; then base_path=""; else base_path="$path"; fi
    if [ "${allow_free_pads:-1}" = "1" ] || [ "${allow_free_pads:-}" = "true" ]; then free_pads="true"; else free_pads="false"; fi
    # NEXTPAD_ADDONS aus den drei Schaltern; immer aus den gespeicherten Einstellungen, auch beim Upgrade
    addons=""
    for a in ideen quiz zeichenbrett; do
        v=$(ynh_app_setting_get --key="addon_$a")
        if [ "$v" = "1" ] || [ "$v" = "true" ]; then addons="${addons:+$addons,}$a"; fi
    done
    ynh_config_add --template="nextpad.env" --destination="$install_dir/nextpad.env"
    chown root:"$app" "$install_dir/nextpad.env"
    chmod 640 "$install_dir/nextpad.env"
}

# API-Schlüssel für mod_etherpadlite erzeugen, wenn noch keiner da ist
nextpad_api_key() {
    if [ ! -s "$data_dir/secrets/api-keys.txt" ]; then
        local key
        key="$(ynh_string_random --length=48)"
        printf '# Ein Schlüssel je Zeile, optional danach die Moodle-Adresse: <schlüssel> https://moodle.example.org\n%s\n' "$key" > "$data_dir/secrets/api-keys.txt"
    fi
}

nextpad_set_permissions() {
    chown root:"$app" "$install_dir/nextpad"
    chmod 750 "$install_dir/nextpad"
    chown -R "$app:$app" "$data_dir"
    chmod 750 "$data_dir" "$data_dir/data" "$data_dir/secrets" "$data_dir/theme"
    find "$data_dir/secrets" -type f -exec chmod 640 {} +
}
