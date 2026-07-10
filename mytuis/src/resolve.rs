//! # Resolución de comandos y paths
//!
//! Acá vive la versión Rust de la función `resolve_path` del script bash.
//! La idea: dado un string que el usuario tipeó (algo como `firefox`,
//! `code /datos/pepe`, `ls -lad` o `~/bin/myscript.sh`), queremos:
//!
//! 1. Separar la primera palabra (el "ejecutable") del resto (los args).
//! 2. Resolver el ejecutable a un **path absoluto** que exista en disco.
//! 3. Devolver `(path_absoluto, args_combinados)` como tupla.
//!
//! A diferencia del bash (que pasaba los resultados por globales
//! `RESOLVED_PATH` y `RESOLVED_ARGS`), en Rust devolvemos una tupla. Esto
//! es mucho más prolijo: la función es pura y testeable.
//!
//! ## Reglas de resolución (en orden)
//!
//! 1. Expandir `~` o `~/` al principio → `$HOME`.
//! 2. Si el resultado es un path **absoluto** y existe → usarlo tal cual.
//! 3. Si empieza con `./` o `../` → resolver contra el directorio actual.
//! 4. Si no → buscar el binario en `$PATH` usando el crate `which`.

use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// Resultado de resolver un comando.
///
/// - `path`: path **absoluto** al ejecutable. Si no se pudo resolver,
///   va vacío (`""`).
/// - `args`: string con todos los argumentos (lo que estaba después del
///   ejecutable en el input, más cualquier argumento extra que el
///   usuario haya pasado por separado). String vacío si no hay args.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedCommand {
    pub path: String,
    pub args: String,
}

impl ResolvedCommand {
    /// `true` si la resolución fue exitosa (encontró un ejecutable).
    pub fn is_ok(&self) -> bool {
        !self.path.is_empty()
    }
}

/// Resuelve una línea de comando completa (`firefox`, `ls -lad`,
/// `code /datos/pepe`) en un struct `ResolvedCommand`.
///
/// Esta función **no falla**: si no puede resolver el ejecutable,
/// devuelve un `ResolvedCommand` con `path` vacío. El caller decide
/// qué hacer (mostrar error al usuario, abortar, etc.).
///
/// Es un port directo de `resolve_path` del bash, con la ventaja de que
/// no contamina el scope con globales.
pub fn resolve_command(input: &str) -> ResolvedCommand {
    let input = input.trim();
    if input.is_empty() {
        return ResolvedCommand::default();
    }

    // --- 1. Separar la primera palabra del resto ----------------------
    // Buscamos el primer whitespace. Si lo encontramos, todo lo de
    // antes es el "ejecutable" y lo de después son los "args".
    let (exec_part, args) = match input.find(char::is_whitespace) {
        Some(idx) => {
            let exec = &input[..idx];
            // Después del espacio, puede haber más espacios al principio
            // que queremos ignorar.
            let rest = input[idx..].trim_start().to_string();
            (exec.to_string(), rest)
        }
        None => (input.to_string(), String::new()),
    };

    // --- 2. Expandir tilde ---------------------------------------------
    // Solo expandimos `~` o `~/` al principio. No expandimos `~usuario`
    // (sería pedirle al SO que resuelva un usuario, lo cual no
    // necesitamos).
    let exec_part = expand_tilde(&exec_part);

    // --- 3. Reglas de resolución ---------------------------------------
    let resolved_path = resolve_executable(&exec_part);

    ResolvedCommand {
        path: resolved_path,
        args,
    }
}

/// Resuelve un "directorio favorito". A diferencia de los comandos, acá
/// no buscamos en `$PATH`: solo expandimos `~` y aceptamos paths
/// absolutos o relativos (contra CWD). Validamos que el directorio
/// exista y sea un directorio.
///
/// Si la resolución tiene éxito, devuelve el path absoluto. Si falla,
/// devuelve un `AppError::InvalidPath` con el input original para que
/// el mensaje de error sea claro.
pub fn resolve_favorite_dir(input: &str) -> Result<PathBuf> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AppError::InvalidPath(String::new()));
    }

    // Expandir `~` al principio.
    let expanded = expand_tilde(input);

    let path = PathBuf::from(&expanded);

    // Si es relativo, resolver contra CWD. `std::fs::canonicalize`
    // hace esto y además normaliza symlinks, pero **falla si el path
    // no existe**. Queremos fallar igual, pero con un mensaje más
    // amable.
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|e| AppError::other(format!("no se pudo leer CWD: {e}")))?
            .join(path)
    };

    // Verificar que exista y sea directorio.
    let metadata = std::fs::metadata(&absolute).map_err(|_| {
        AppError::InvalidPath(absolute.to_string_lossy().to_string())
    })?;
    if !metadata.is_dir() {
        return Err(AppError::InvalidPath(format!(
            "{} (no es un directorio)",
            absolute.to_string_lossy()
        )));
    }

    // Intentamos canonicalizar para colapsar `..` y symlinks. Si
    // falla (raro si ya validamos que existe), devolvemos el path
    // absoluto a secas.
    Ok(std::fs::canonicalize(&absolute).unwrap_or(absolute))
}

/// Resuelve/valida la URL de un tool. Acepta solo URLs http(s) con una
/// parte de host no vacía. Devuelve la URL normalizada (trim) si está
/// OK, o un `AppError::InvalidUrl` con el input original.
///
/// Decisiones de validación:
/// - Rechaza vacío.
/// - Solo `http://` y `https://` (los demás esquemas — `file:`, `ssh:`
///   — son cosas distintas y no aplican a este caso).
/// - Debe haber una parte de "autoridad" no vacía (ej. `example.com`).
///
/// No parseamos con `url::Url` para no agregar una dependencia: hacemos
/// el chequeo a mano, suficiente para los casos típicos.
pub fn resolve_tool_url(input: &str) -> Result<String> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AppError::InvalidUrl(String::new()));
    }

    // Buscamos el esquema al principio: `[a-zA-Z][a-zA-Z0-9+.-]*:`.
    // Si no hay `:` en posición razonable, no es una URL absoluta.
    let (scheme, rest) = match input.find(':') {
        // Encontramos `:`. Lo que va antes es el esquema candidato.
        Some(idx) if idx > 0 => (&input[..idx], &input[idx + 1..]),
        // `:` en la posición 0 (raro) o no hay `:` → no es URL válida.
        _ => return Err(AppError::InvalidUrl(input.to_string())),
    };

    let scheme_lower = scheme.to_lowercase();
    if scheme_lower != "http" && scheme_lower != "https" {
        return Err(AppError::InvalidUrl(input.to_string()));
    }

    // Después de `http(s):` tiene que venir `//` (URLs absolutas con
    // autoridad). Si no, no es una URL que sepamos abrir.
    let rest = rest.trim_start();
    if !rest.starts_with("//") {
        return Err(AppError::InvalidUrl(input.to_string()));
    }

    // El host es lo que viene después de `//` hasta el próximo `/`,
    // `?`, `#` o fin de string. Tiene que ser no vacío.
    let after_slashes = &rest[2..];
    let host_end = after_slashes
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_slashes.len());
    let host = &after_slashes[..host_end];
    if host.is_empty() {
        return Err(AppError::InvalidUrl(input.to_string()));
    }

    Ok(input.to_string())
}

/// Helper interno: expande `~` o `~/foo` al principio de un path.
///
/// Si `$HOME` no está definido, devuelve el input sin tocar (mejor
/// mostrar un error después que crashear acá).
fn expand_tilde(input: &str) -> String {
    if input == "~" {
        // Caso especial: solo `~` → todo `$HOME`.
        dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| input.to_string())
    } else if let Some(stripped) = input.strip_prefix("~/") {
        // `~/algo` → `$HOME/algo`.
        dirs::home_dir()
            .map(|p| p.join(stripped).to_string_lossy().to_string())
            .unwrap_or_else(|| input.to_string())
    } else {
        // No empieza con `~`, lo dejamos igual.
        input.to_string()
    }
}

/// Helper interno: dado un ejecutable ya con tilde expandido, intenta
/// encontrar el archivo en disco. Devuelve `""` si no lo encuentra.
fn resolve_executable(exec: &str) -> String {
    // Regla 2: absoluto y existe.
    let p = Path::new(exec);
    if p.is_absolute() {
        if p.exists() {
            return exec.to_string();
        }
        return String::new();
    }

    // Regla 3: relativo (`./foo`, `../foo`).
    if exec.starts_with("./") || exec.starts_with("../") {
        if let Ok(cwd) = std::env::current_dir() {
            let candidate = cwd.join(exec);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
        return String::new();
    }

    // Regla 4: buscar en $PATH.
    if let Ok(found) = which::which(exec) {
        return found.to_string_lossy().to_string();
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_bare_command_en_path() {
        // `ls` debería estar disponible en cualquier Unix.
        let r = resolve_command("ls -lad");
        assert!(r.is_ok(), "debería poder resolver `ls`: {:?}", r);
        assert!(r.path.ends_with("ls"));
        assert_eq!(r.args, "-lad");
    }

    #[test]
    fn resolve_sin_args() {
        let r = resolve_command("ls");
        assert!(r.is_ok());
        assert_eq!(r.args, "");
    }

    #[test]
    fn resolve_comando_inexistente_devuelve_path_vacio() {
        let r = resolve_command("mytuis_comando_inexistente_xyz_123");
        assert!(!r.is_ok());
        assert_eq!(r.path, "");
    }

    #[test]
    fn expand_tilde_basico() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~"), home.to_string_lossy());
        assert_eq!(expand_tilde("~/foo"), home.join("foo").to_string_lossy());
        assert_eq!(expand_tilde("/abs/path"), "/abs/path");
        assert_eq!(expand_tilde("relativo"), "relativo");
    }

    #[test]
    fn resolve_path_absoluto_existente() {
        // `/bin/sh` debería existir en cualquier Linux.
        let r = resolve_command("/bin/sh");
        assert!(r.is_ok(), "r = {:?}", r);
        assert_eq!(r.path, "/bin/sh");
    }

    #[test]
    fn resolve_tool_url_acepta_http_y_https() {
        let r = resolve_tool_url("https://grafana.example.com/d/abc").unwrap();
        assert_eq!(r, "https://grafana.example.com/d/abc");
        let r = resolve_tool_url("http://example.com").unwrap();
        assert_eq!(r, "http://example.com");
    }

    #[test]
    fn resolve_tool_url_normaliza_espacios_extremos() {
        let r = resolve_tool_url("  https://example.com  ").unwrap();
        assert_eq!(r, "https://example.com");
    }

    #[test]
    fn resolve_tool_url_rechaza_vacia() {
        assert!(resolve_tool_url("").is_err());
        assert!(resolve_tool_url("   ").is_err());
    }

    #[test]
    fn resolve_tool_url_rechaza_esquema_invalido() {
        // file://, ssh://, sin esquema, etc.
        assert!(resolve_tool_url("file:///etc/passwd").is_err());
        assert!(resolve_tool_url("ssh://user@host").is_err());
        assert!(resolve_tool_url("example.com").is_err());
        assert!(resolve_tool_url("ftp://example.com").is_err());
    }

    #[test]
    fn resolve_tool_url_rechaza_host_vacio() {
        // `https://` solo, sin host.
        assert!(resolve_tool_url("https://").is_err());
        assert!(resolve_tool_url("https:///path").is_err());
    }
}