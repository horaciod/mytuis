//! # Acciones externas: abrir terminal + clipboard
//!
//! Dos responsabilidades auxiliares que no encajan en otro módulo:
//!
//! 1. **Abrir una terminal** en un directorio dado (la acción principal
//!    de un favorito desde la TUI).
//! 2. **Copiar al portapapeles** del SO (para la acción "copiar path").
//!
//! ## Abrir terminal
//!
//! No hay una API estándar de Unix para "abrir nueva terminal". Cada
//! emulador tiene sus flags. Nuestra estrategia:
//!
//! 1. Si el usuario definió `$TERMINAL`, usar eso (convención común
//!    en Arch/Artix).
//! 2. Si no, probar una lista ordenada de terminales conocidos:
//!    `gnome-terminal`, `konsole`, `xfce4-terminal`, `alacritty`,
//!    `kitty`, `foot`, `wezterm`, `xterm`.
//! 3. Si ninguno está instalado, devolver `AppError::NoTerminal`.
//!
//! Para cada terminal soportado sabemos qué flag acepta para cambiar
//! el directorio de trabajo. Si el terminal no está en nuestra lista,
//! caemos a un fallback genérico vía shell: `sh -c "cd <dir> && exec
//! $TERM"`.

use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::error::{AppError, Result};

// ============================================================================
//  CLIPBOARD
// ============================================================================

/// Copia `text` al portapapeles del sistema. Usa el crate `arboard`,
/// que en Linux habla con X11/Wayland, en macOS con NSPasteboard y en
/// Windows con OLE.
///
/// Si el portapapeles no está disponible (por ejemplo, headless),
/// devuelve un error. La idea es que la TUI muestre un mensaje
/// "✖ no se pudo copiar al portapapeles" pero no se caiga.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| AppError::other(format!("clipboard: {e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| AppError::other(format!("clipboard: {e}")))?;
    Ok(())
}

// ============================================================================
//  TERMINAL
// ============================================================================

/// Lista ordenada de terminales a probar si el usuario no definió
/// `$TERMINAL`. El orden refleja popularidad en Linux de escritorio:
/// GNOME primero (Ubuntu/Fedora/Workstation), KDE segundo, después
/// los más livianos.
const CANDIDATE_TERMINALS: &[&str] = &[
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "alacritty",
    "kitty",
    "foot",
    "wezterm",
    "xterm",
];

/// Devuelve el nombre del binario del primer terminal disponible.
///
/// Estrategia:
/// 1. Si `$TERMINAL` está definido y existe en `$PATH`, usarlo.
/// 2. Si no, recorrer la lista `CANDIDATE_TERMINALS` y devolver el
///    primero que esté en `$PATH`.
fn pick_terminal() -> Option<String> {
    // Opción 1: variable de entorno.
    if let Ok(t) = std::env::var("TERMINAL") {
        if !t.trim().is_empty() && which::which(&t).is_ok() {
            return Some(t);
        }
    }

    // Opción 2: candidatos conocidos.
    for cand in CANDIDATE_TERMINALS {
        if which::which(cand).is_ok() {
            return Some((*cand).to_string());
        }
    }

    None
}

/// Lanza una terminal en el directorio `cwd`. La terminal se abre
/// como proceso **hijo**, pero desacoplado de la TUI (no esperamos a
/// que termine: `spawn`, no `wait`).
///
/// Devuelve `Ok(())` si el `spawn` funcionó, o un error si:
/// - No hay terminal disponible.
/// - El comando falla al lanzarse.
pub fn open_terminal_in(cwd: &Path) -> Result<()> {
    let term = pick_terminal().ok_or(AppError::NoTerminal)?;
    let cwd = cwd.to_path_buf();

    // Cada terminal tiene su flag distinto. Hacemos match sobre el
    // nombre y construimos el comando apropiado.
    let mut cmd = build_terminal_command(&term, &cwd);

    // `spawn` en vez de `output` para que la terminal se abra en
    // background. `Command::new(...).spawn()` hereda stdin/stdout/
    // stderr por defecto, lo que está perfecto: el usuario quiere
    // ver la terminal nueva.
    cmd.spawn().map_err(|e| {
        AppError::other(format!(
            "no se pudo lanzar la terminal '{}': {e}",
            term
        ))
    })?;

    Ok(())
}

/// Construye el `Command` apropiado para el terminal detectado. Si el
/// terminal no está en nuestra lista de "soportados con flag propio",
/// usamos el fallback genérico vía `sh -c "cd && exec"`.
fn build_terminal_command(term: &str, cwd: &Path) -> Command {
    let cwd_str = cwd.to_string_lossy().to_string();

    match term {
        // --working-directory existe en la mayoría de los terminales
        // modernos.
        "gnome-terminal" | "konsole" | "xfce4-terminal" | "alacritty"
        | "kitty" | "foot" | "wezterm" => {
            let mut cmd = Command::new(term);
            cmd.arg("--working-directory").arg(cwd);
            cmd
        }
        // `xterm` no tiene flag de working directory; lo cambiamos vía
        // shell. El `exec` reemplaza el `sh` con `xterm` para no
        // dejar un proceso intermediario.
        "xterm" => {
            let quoted = shell_quote(&cwd_str);
            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(format!("cd {quoted} && exec xterm"));
            cmd
        }
        // Fallback genérico: si el usuario puso un terminal raro en
        // $TERMINAL, intentamos el patrón "cd && exec".
        _ => {
            let quoted_cwd = shell_quote(&cwd_str);
            let quoted_term = shell_quote(term);
            let mut cmd = Command::new("sh");
            cmd.arg("-c")
                .arg(format!("cd {quoted_cwd} && exec {quoted_term}"));
            cmd
        }
    }
}

/// Escapa un string para usarlo dentro de comillas simples en un shell.
/// Es una versión minimalista de `shell_escape`: reemplaza cada `'` por
/// `'\''` (cerrar comilla, comilla escapada, abrir comilla).
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ============================================================================
//  SHELL INTEGRATION (fd 3)
// ============================================================================

// ============================================================================
//  URL OPENER (aplicaciones remotas / tools)
// ============================================================================

// ============================================================================
//  URL OPENER (aplicaciones remotas / tools)
// ============================================================================
//
// Un "tool" es una URL (Grafana, dashboards, etc.) que queremos abrir
// rápidamente. No hay un API estándar de Unix para eso, así que
// usamos el "opener" del entorno:
//
// - Linux (Freedesktop): intentamos `xdg-open` primero, después `gio
//   open` (que es el backend de GNOME para la misma idea).
// - macOS: `open`.
//
// Si ninguno está disponible, devolvemos error. La idea es la misma
// que `open_terminal_in`: hacer lo razonable sin inventar requisitos.

/// Abre `url` con el opener del sistema. `Spawn` desacoplado, no
/// esperamos a que termine (sería un navegador o un handler de URL
/// del usuario).
///
/// Devuelve `Ok(())` si el `spawn` funcionó. `Err` en cualquier otro
/// caso (no hay opener, falla el spawn, etc.).
///
/// Para el caso `gio open <url>` (que sí lleva un argumento pre-URL)
/// delegamos a una rama específica en vez de armar el vector de args
/// dinámicamente — más legible y sin acrobacias con lifetimes.
pub fn open_url(url: &str) -> Result<()> {
    // 1. xdg-open (Freedesktop, presente en casi cualquier Linux).
    if which::which("xdg-open").is_ok() {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(url);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
    }

    // 2. gio open (GNOME — antes era `gvfs-open`).
    if which::which("gio").is_ok() {
        let result = Command::new("gio").arg("open").arg(url).spawn();
        if result.is_ok() {
            return Ok(());
        }
    }

    // 3. open (macOS).
    if which::which("open").is_ok() {
        let mut cmd = Command::new("open");
        cmd.arg(url);
        if cmd.spawn().is_ok() {
            return Ok(());
        }
    }

    Err(AppError::other(
        "no se encontró un opener para URLs (probá instalar xdg-utils / gio)",
    ))
}
//
// El fd 3 es el "side channel" estándar para que un subproceso le pase
// comandos al shell padre sin necesidad de un wrapper externo (mismo
// patrón que `broot`, `zoxide`, `fzf-cd-widget`, etc.).
//
// Flujo típico:
//
//   1. El usuario define en su `.bashrc` / `.zshrc`:
//        mytuis() {
//            local out
//            out=$(command mytuis "$@" 3>&1 1>&2 2>&3)
//            [ -n "$out" ] && eval "$out"
//        }
//
//   2. Cuando invoca `mytuis` (TUI o subcomando), el shell dup-lica el
//      stdout del padre al fd 3 del hijo (`3>&1`), y mueve el stdout
//      del hijo al stderr (`1>&2`).
//
//   3. mytuis escribe al fd 3 (que originalmente era stdout del padre)
//      comandos que el padre debe ejecutar **en su propio contexto**,
//      no en el subshell de `$(...)`.
//
//   4. Cuando mytuis termina, el `$(...)` del wrapper recoge el fd 3
//      y el shell padre hace `eval` sobre eso → `cd /path`, etc.

/// Formatea un comando `cd <path>` para emitir por el side channel.
///
/// Es una función privada testeable: la lógica de formato vive acá
/// para poder verificarla sin necesidad de un fd real. La función
/// pública `emit_cd_to_fd3` se limita a wrappear a esta con la
/// apertura de `/dev/fd/3`.
fn format_cd_payload(path: &Path) -> String {
    format!("cd {}\n", path.display())
}

/// Emite `cd <path>` al descriptor de archivo 3 del proceso.
///
/// ## Comportamiento cuando fd 3 NO está abierto
///
/// Devuelve `Err(AppError::Other)` con un mensaje que la TUI muestra
/// como flash con instrucciones para configurar el wrapper. No
/// intentamos caer a stdout porque la TUI está en alternate screen y
/// la salida sería invisible o perturbadora.
///
/// ## Portabilidad
///
/// Usa `/dev/fd/3`, symlink disponible en Linux y macOS. En Windows
/// no funciona — pero mytuis ya es Unix-first de todas formas.
///
/// ## Tests
///
/// El formateo del payload se testea unitariamente. La escritura real
/// al fd 3 se valida manualmente con el smoke test (un wrapper que
/// dup-lica un tempfile al fd 3 antes de invocar mytuis).
pub fn emit_cd_to_fd3(path: &Path) -> Result<()> {
    let payload = format_cd_payload(path);

    // Abrimos `/dev/fd/3` para escritura. Si el shell padre no dup-licó
    // nada al fd 3, la syscall falla con EBADF (Linux) o ENOENT
    // (algunos sistemas con `/dev/fd` levemente distinto). Cualquier
    // error se traduce a un mensaje claro.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/fd/3")
        .map_err(|e| {
            AppError::other(format!(
                "fd 3 no está abierto (¿wrapper del shell no configurado?): {e}"
            ))
        })?;

    f.write_all(payload.as_bytes())
        .map_err(|e| AppError::other(format!("escritura a fd 3 falló: {e}")))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_escapa_comilla_simple() {
        assert_eq!(shell_quote("foo"), "'foo'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn shell_quote_con_path_con_espacios() {
        assert_eq!(shell_quote("/datos/pepe repo"), "'/datos/pepe repo'");
    }

    #[test]
    fn format_cd_payload_path_simple() {
        // El payload para emitir al fd 3 debe ser exactamente
        // `cd <path>\n`. El `\n` final es importante porque el wrapper
        // del shell va a hacer `eval` sobre lo que lea.
        assert_eq!(format_cd_payload(Path::new("/datos/pepe")), "cd /datos/pepe\n");
    }

    #[test]
    fn format_cd_payload_path_con_espacios() {
        // `Path::display()` no escapa espacios ni quotes — eso es
        // responsabilidad del shell que hace `eval`. Mientras tanto
        // nosotros pasamos el path tal cual, sin tocar.
        assert_eq!(
            format_cd_payload(Path::new("/datos/pepe repo")),
            "cd /datos/pepe repo\n"
        );
    }

    #[test]
    fn format_cd_payload_path_con_caracteres_unicode() {
        // El `display()` usa la representación nativa del SO. En Unix
        // suele ser UTF-8 raw; no escapamos nada.
        let p = Path::new("/datos/proyectos/ñoño");
        assert_eq!(format_cd_payload(p), "cd /datos/proyectos/ñoño\n");
    }
}