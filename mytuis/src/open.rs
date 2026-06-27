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
}