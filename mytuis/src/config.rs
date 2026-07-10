//! # Configuración: rutas a los archivos de datos
//!
//! Centralizamos acá todas las rutas de archivos que usa el programa.
//! La idea es que si mañana querés cambiar la ubicación, modificás un
//! solo lugar. También dejamos preparado el terreno para respetar XDG
//! (la convención Linux de tener configs en `$XDG_CONFIG_HOME` o, si no
//! está definida, en `~/.config`).
//!
//! Decisión de diseño: la versión bash guardaba todo en un único
//! archivo `~/.mytuis.yaml`. Acá pasamos a un **directorio**:
//!
//! ```text
//! ~/.mytuis/
//! ├── apps.yaml      ← apps (formato idéntico al del bash)
//! └── favs.yaml      ← favoritos (formato nuevo)
//! ```
//!
//! Esto facilita agregar más entidades en el futuro sin que el archivo
//! principal se vuelva un quilombo, y permite que el módulo de migración
//! toque solo `apps.yaml` y deje `favs.yaml` tranquilo.

use std::path::PathBuf;

/// Devuelve el directorio donde viven los datos de `mytuis`.
///
/// Por ahora es fijo: `$HOME/.mytuis/`. Si quisiéramos respetar XDG
/// completamente, podríamos chequear `$XDG_CONFIG_HOME/mytuis/` primero.
pub fn data_dir() -> PathBuf {
    // `dirs::home_dir()` resuelve `$HOME` (o el equivalente en cada SO).
    // Devuelve `Option<PathBuf>` porque teóricamente podría no estar
    // definido (raro, pero posible). En ese caso caemos a `.` que es
    // el directorio actual — feo, pero el programa va a fallar de
    // cualquier manera al intentar escribir ahí.
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mytuis")
}

/// Archivo de apps: `<data_dir>/apps.yaml`.
pub fn apps_file() -> PathBuf {
    data_dir().join("apps.yaml")
}

/// Archivo de favoritos: `<data_dir>/favs.yaml`.
pub fn favs_file() -> PathBuf {
    data_dir().join("favs.yaml")
}

/// Archivo de tools (aplicaciones remotas): `<data_dir>/tools.yaml`.
pub fn tools_file() -> PathBuf {
    data_dir().join("tools.yaml")
}

/// Archivo legacy de la versión bash: `~/.mytuis.yaml`. Lo usamos solo
/// durante la migración: si existe y todavía no migramos, lo leemos,
/// lo movemos a `apps.yaml` y renombramos este a `.bak`.
pub fn legacy_bash_file() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mytuis.yaml")
}

/// Asegura que el directorio de datos exista. Si no existe, lo crea
/// (incluyendo los padres, igual que `mkdir -p`). Llamamos a esto al
/// arrancar el programa y antes de cualquier escritura.
pub fn ensure_data_dir() -> crate::error::Result<()> {
    let dir = data_dir();
    if !dir.exists() {
        // `create_dir_all` es como `mkdir -p`: crea toda la cadena de
        // directorios padres que hagan falta. No falla si el directorio
        // ya existe.
        std::fs::create_dir_all(&dir).map_err(|source| crate::error::AppError::Io {
            path: dir,
            source,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_termina_en_mytuis() {
        let dir = data_dir();
        assert_eq!(dir.file_name().and_then(|s| s.to_str()), Some(".mytuis"));
    }

    #[test]
    fn apps_file_y_favs_file_viven_en_data_dir() {
        let a = apps_file();
        let f = favs_file();
        assert_eq!(a.parent(), f.parent());
        assert_eq!(a.parent().unwrap(), data_dir());
    }

    #[test]
    fn tools_file_vive_en_data_dir() {
        let t = tools_file();
        assert_eq!(t.parent().unwrap(), data_dir());
        assert_eq!(t.file_name().and_then(|s| s.to_str()), Some("tools.yaml"));
    }
}