// Part of Agent Record Adapter (see NOTICE).
//! Repairing a recording's script with the person's own AI agent: a prompt, ready to paste, that
//! names the `script-repair` skill, the files and where the last run stopped. The app copies it;
//! it can also open Claude with it already written (nothing is sent until the person sends it).

use std::path::Path;

/// The whole output of the last play, written next to the script.
pub const LOG: &str = "last-run.log";

/// The line that tells where the last play stopped, if it failed: "Step 4 (Click "New script")
/// failed: ...", or the task failing before its first step.
pub fn failure(dir: &Path) -> Option<String> {
    let log = std::fs::read_to_string(dir.join(LOG)).ok()?;
    log.lines()
        .rev()
        .find(|line| {
            (line.starts_with("Step ") && line.contains(") failed: "))
                || line.starts_with("The task failed")
        })
        .map(|line| line.trim().to_owned())
}

/// How the last play ended, for the prompt.
enum LastRun {
    Failed(String),
    Finished,
    Never,
}

fn last_run(dir: &Path) -> LastRun {
    if let Some(line) = failure(dir) {
        return LastRun::Failed(line);
    }
    match std::fs::read_to_string(dir.join(LOG)) {
        Ok(_) => LastRun::Finished,
        Err(_) => LastRun::Never,
    }
}

/// The prompt for the person's AI agent, in the app's language.
pub fn prompt(dir: &Path, spanish: bool) -> String {
    let folder = dir.display();
    let video = dir.join("video.mp4").exists();
    let log = dir.join(LOG).exists();
    let run = last_run(dir);
    if spanish {
        let mut files = vec![
            "- script.mjs: el script (se ejecuta con node y usa desktop-script.mjs)".to_owned(),
            "- recording.jsonl: lo que hice, paso a paso".to_owned(),
        ];
        if video {
            files.push("- video.mp4: el video de la grabación".to_owned());
        }
        if log {
            files.push(format!(
                "- {LOG}: la salida completa de la última ejecución"
            ));
        }
        let ended = match run {
            LastRun::Failed(line) => format!("Última ejecución: {line}"),
            LastRun::Finished => {
                "Última ejecución: terminó, pero el script ya no hace bien la tarea.".to_owned()
            }
            LastRun::Never => "Todavía no se ha ejecutado desde Desktop Recorder.".to_owned(),
        };
        format!(
            "Repara un script de Desktop Recorder que no termina la tarea que se grabó.\n\n\
             Usa la skill script-repair. Si no la tienes, instálala con `npx desktop-recorder-mcp` \
             (viene con Agent Record Adapter).\n\n\
             Carpeta de la grabación: {folder}\n{files}\n\n{ended}\n\n\
             Cómo trabajar:\n\
             - Usa agent-record en segundo plano (apps de escritorio con sus comandos de escritorio, páginas web \
             con `agent-record browser`): no uses mi ratón ni mi teclado ni pongas ventanas al frente.\n\
             - Haz la tarea igual que en la grabación, en la misma app y por el mismo camino. Si la grabación \
             no guardó algo (por ejemplo, la opción elegida en un menú de clic derecho), búscalo en los menús de \
             la app o pregúntame; no inventes datos.\n\
             - Antes de cambiarlo, guarda una copia como script.original.mjs y deja el script reparado en script.mjs.\n\
             - Pruébalo con `node script.mjs --stop-before N` y pregúntame antes de un paso que no se puede deshacer \
             (enviar, borrar, pagar, publicar).\n\
             - Al terminar, dime qué paso fallaba, por qué y qué cambiaste.",
            files = files.join("\n"),
        )
    } else {
        let mut files = vec![
            "- script.mjs: the script (run with node; it uses desktop-script.mjs)".to_owned(),
            "- recording.jsonl: what I did, step by step".to_owned(),
        ];
        if video {
            files.push("- video.mp4: the video of the recording".to_owned());
        }
        if log {
            files.push(format!("- {LOG}: the whole output of the last run"));
        }
        let ended = match run {
            LastRun::Failed(line) => format!("Last run: {line}"),
            LastRun::Finished => {
                "Last run: it finished, but the script no longer does the task right.".to_owned()
            }
            LastRun::Never => "It has not been run from Desktop Recorder yet.".to_owned(),
        };
        format!(
            "Repair a Desktop Recorder script that no longer finishes the task that was recorded.\n\n\
             Use the script-repair skill. If you do not have it, install it with `npx desktop-recorder-mcp` \
             (it comes with Agent Record Adapter).\n\n\
             Recording folder: {folder}\n{files}\n\n{ended}\n\n\
             How to work:\n\
             - Use agent-record in the background (desktop apps through its desktop commands, web pages \
             through `agent-record browser`): never use my mouse or keyboard, or bring windows to the front.\n\
             - Do the task as the recording did, in the same app and the same way. If the recording missed \
             something (such as the item chosen in a right-click menu), look for it in the app's menus or ask me; \
             never make data up.\n\
             - Before changing it, keep a copy as script.original.mjs, and leave the repaired script in script.mjs.\n\
             - Try it with `node script.mjs --stop-before N`, and ask me before a step that cannot be undone \
             (send, delete, pay, publish).\n\
             - When you finish, tell me which step failed, why, and what you changed.",
            files = files.join("\n"),
        )
    }
}

/// `text` for a URL's query: everything but letters, digits and `-._~` percent-encoded (UTF-8).
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The links that open Claude with the prompt written and the recording's folder: Claude Code in
/// Claude Desktop, else Claude Code in a terminal. Neither sends anything by itself.
pub fn links(dir: &Path, prompt: &str) -> [String; 2] {
    let (q, folder) = (encode(prompt), encode(&dir.display().to_string()));
    [
        format!("claude://code/new?q={q}&folder={folder}"),
        format!("claude-cli://open?q={q}&cwd={folder}"),
    ]
}

/// The whole prompt, kept in the recording's folder for a short message to point to.
pub const PROMPT_FILE: &str = "repair-prompt.md";

/// The message a link carries: short, because a long link does not reach the app (about 2,000
/// characters is already too many); it points to the whole prompt, written next to the script.
pub fn short_prompt(dir: &Path, spanish: bool) -> String {
    let stopped = failure(dir)
        .map(|line| line.chars().take(300).collect::<String>())
        .map(|line| {
            if spanish {
                format!(" Última ejecución: {line}")
            } else {
                format!(" Last run: {line}")
            }
        })
        .unwrap_or_default();
    if spanish {
        format!(
            "Repara el script de Desktop Recorder de esta carpeta ({}) con la skill script-repair. \
             Lee primero {PROMPT_FILE}: tiene los archivos, dónde falló y cómo trabajar.{stopped}",
            dir.display()
        )
    } else {
        format!(
            "Repair the Desktop Recorder script in this folder ({}) with the script-repair skill. \
             Read {PROMPT_FILE} first: it lists the files, where it failed and how to work.{stopped}",
            dir.display()
        )
    }
}

/// Opens Claude with a message ready to send (the whole prompt goes to `repair-prompt.md`): the
/// first link the system can open. Returns whether one opened. For tests,
/// `DESKTOP_RECORDER_OPEN_LOG` names a file the links are written to instead.
pub fn open_in_claude(dir: &Path, prompt: &str, spanish: bool) -> bool {
    let _ = std::fs::write(dir.join(PROMPT_FILE), prompt);
    let links = links(dir, &short_prompt(dir, spanish));
    if let Some(log) = std::env::var_os("DESKTOP_RECORDER_OPEN_LOG") {
        return std::fs::write(log, &links[0]).is_ok();
    }
    links.iter().any(|link| open_link(link))
}

#[cfg(windows)]
fn open_link(link: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(link));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Above 32 means the system found a program for the link and started it.
    result as isize > 32
}

#[cfg(not(windows))]
fn open_link(link: &str) -> bool {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(link)
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_names_the_skill_the_files_and_where_it_stopped() {
        let dir = std::env::temp_dir().join(format!("dr-repair-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(LOG),
            "Step 1. Open db.exe\nStep 4 (Click \"New script\") failed: No window titled \"x\"\n",
        )
        .unwrap();
        let text = prompt(&dir, true);
        assert!(text.contains("script-repair"));
        assert!(text.contains(&dir.display().to_string()));
        assert!(text.contains("Step 4 (Click \"New script\") failed"));
        assert!(text.contains("last-run.log"));
        assert!(!text.contains("video.mp4"));
        assert!(
            text.len() < 5_000,
            "a link carries at most 5,000 characters"
        );
        let short = short_prompt(&dir, true);
        assert!(short.contains("repair-prompt.md") && short.contains("Step 4"));
        assert!(
            links(&dir, &short)[0].len() < 1_500,
            "a short link reaches the app"
        );
        let [desktop, cli] = links(&dir, &text);
        assert!(desktop.starts_with("claude://code/new?q=Repara%20un%20script"));
        assert!(desktop.contains("&folder="));
        assert!(cli.starts_with("claude-cli://open?q="));
        assert!(!desktop.contains(' ') && !desktop.contains('\n'));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn query_text_is_percent_encoded_as_utf8() {
        assert_eq!(encode("a b\nñ&"), "a%20b%0A%C3%B1%26");
    }
}
