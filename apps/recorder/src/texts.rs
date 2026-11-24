// Part of Agent Record Adapter (see NOTICE).
//! The app's texts, in English and Spanish, chosen by the language of Windows.

pub struct Texts {
    pub title: &'static str,
    pub record: &'static str,
    pub stop: &'static str,
    pub settings: &'static str,
    pub recordings: &'static str,
    pub close: &'static str,
    pub what: &'static str,
    pub display: &'static str,
    pub video: &'static str,
    pub folder: &'static str,
    pub today: &'static str,
    pub steps: &'static str,
    pub step: &'static str,
    pub play: &'static str,
    pub open_folder: &'static str,
    pub playing: &'static str,
    pub played: &'static str,
    pub failed: &'static str,
    pub none_yet: &'static str,
    pub nothing_recorded: &'static str,
    pub irreversible: &'static str,
    pub run: &'static str,
    pub cancel: &'static str,
    pub repair: &'static str,
    pub repair_hint: &'static str,
    pub copied: &'static str,
    pub update: &'static str,
    pub version: &'static str,
    pub up_to_date: &'static str,
    pub checking: &'static str,
    pub offline: &'static str,
    pub updating: &'static str,
    pub update_failed: &'static str,
    /// Whether the texts are the Spanish ones (the repair prompt follows them).
    pub spanish: bool,
    pub no_tool: &'static str,
    pub saving: &'static str,
}

const EN: Texts = Texts {
    title: "Desktop Recorder",
    record: "Record",
    stop: "Stop",
    settings: "Settings",
    recordings: "Recordings",
    close: "Close",
    what: "Record",
    display: "Screen",
    video: "With video",
    folder: "Save in",
    today: "Today",
    steps: "steps",
    step: "step",
    play: "Play",
    open_folder: "Open folder",
    playing: "Playing in the background...",
    played: "Done",
    failed: "Stopped:",
    none_yet: "Your recordings show up here.",
    nothing_recorded: "Nothing was done in what was recorded.",
    irreversible: "It clicks something that can't be undone:",
    run: "Play it",
    cancel: "Cancel",
    repair: "Repair with AI",
    repair_hint: "Copy the instructions to repair it with your AI",
    copied: "Copied: paste it into your AI agent",
    update: "Update",
    version: "Version",
    up_to_date: "up to date",
    checking: "Checking...",
    offline: "no connection",
    updating: "Updating...",
    update_failed: "The update failed:",
    spanish: false,
    no_tool: "agent-record is missing: run npx agent-record-adapter",
    saving: "Saving...",
};

const ES: Texts = Texts {
    title: "Grabadora de escritorio",
    record: "Grabar",
    stop: "Detener",
    settings: "Ajustes",
    recordings: "Grabaciones",
    close: "Cerrar",
    what: "Qué grabar",
    display: "Pantalla",
    video: "Con video",
    folder: "Guardar en",
    today: "Hoy",
    steps: "pasos",
    step: "paso",
    play: "Reproducir",
    open_folder: "Abrir carpeta",
    playing: "Reproduciendo en segundo plano...",
    played: "Terminado",
    failed: "Se detuvo:",
    none_yet: "Aquí aparecen tus grabaciones.",
    nothing_recorded: "No se hizo nada en lo que se grabó.",
    irreversible: "Pulsa algo que no se puede deshacer:",
    run: "Reproducir",
    cancel: "Cancelar",
    repair: "Reparar con IA",
    repair_hint: "Copiar instrucciones para repararlo con tu IA",
    copied: "Copiado: pégalo en tu agente de IA",
    update: "Actualizar",
    version: "Versión",
    up_to_date: "al día",
    checking: "Buscando...",
    offline: "sin conexión",
    updating: "Actualizando...",
    update_failed: "No se pudo actualizar:",
    spanish: true,
    no_tool: "Falta agent-record: ejecuta npx agent-record-adapter",
    saving: "Guardando...",
};

pub fn texts() -> &'static Texts {
    if spanish() { &ES } else { &EN }
}

#[cfg(windows)]
fn spanish() -> bool {
    if let Ok(lang) = std::env::var("DESKTOP_RECORDER_LANG") {
        return lang.starts_with("es");
    }
    let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    language & 0x3ff == 0x0a
}

#[cfg(not(windows))]
fn spanish() -> bool {
    std::env::var("DESKTOP_RECORDER_LANG")
        .or_else(|_| std::env::var("LANG"))
        .is_ok_and(|lang| lang.starts_with("es"))
}
