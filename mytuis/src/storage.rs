//! # Persistencia: leer y escribir el YAML
//!
//! Toda la I/O de archivos vive acá. Las reglas que seguimos:
//!
//! 1. **Archivos separados**: `apps.yaml` y `favs.yaml`, ambos en
//!    `~/.mytuis/` (ver `config.rs`).
//! 2. **Escritura atómica**: cuando guardamos, escribimos primero a un
//!    archivo `.tmp` y después hacemos `rename` al definitivo. Así, si
//!    la energía se corta a mitad de escritura, el archivo viejo sigue
//!    intacto.
//! 3. **Migración automática**: si encontramos el `~/.mytuis.yaml` del
//!    bash y todavía no migramos, lo importamos a `apps.yaml` y
//!    renombramos el original a `~/.mytuis.yaml.bak`.
//! 4. **Auto-inicialización**: si el archivo no existe, devolvemos una
//!    lista vacía en vez de error (es lo que espera la TUI: "no hay
//!    apps registradas todavía").
//!
//! ## Formato de los archivos
//!
//! `apps.yaml`:
//! ```yaml
//! apps:
//!   - name: 'nvim'
//!     description: 'Editor'
//!     path: '/usr/bin/nvim'
//!     created: '2026-06-26 10:00:00'
//!     last_used: '2026-06-26 12:00:00'
//! ```
//!
//! `favs.yaml`:
//! ```yaml
//! favorites:
//!   - name: 'pepe'
//!     path: '/datos/pepe'
//!     created: '2026-06-26 11:00:00'
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{apps_file, ensure_data_dir, favs_file, legacy_bash_file};
use crate::error::{AppError, Result};
use crate::model::{App, AppsFile, FavoritePath, FavoritesFile};

// ============================================================================
//  APPS
// ============================================================================

/// Lee todas las apps del archivo. Si el archivo no existe (primera
/// corrida), devuelve un vector vacío. Si el archivo existe pero está
/// vacío o solo tiene `apps: []`, también devuelve `vec![]`.
pub fn load_apps() -> Result<Vec<App>> {
    let path = apps_file();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|source| AppError::Io {
        path: path.clone(),
        source,
    })?;

    // Si el archivo está vacío, serde_yaml se queja. Lo tratamos como
    // "sin apps", igual que si no existiera.
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let parsed: AppsFile = serde_yaml::from_str(&text).map_err(|source| AppError::Yaml {
        path: path.clone(),
        source,
    })?;
    Ok(parsed.apps)
}

/// Guarda todas las apps, sobrescribiendo el archivo. Escritura atómica.
pub fn save_apps(apps: &[App]) -> Result<()> {
    ensure_data_dir()?;
    let path = apps_file();
    let payload = AppsFile { apps: apps.to_vec() };
    let yaml = serde_yaml::to_string(&payload).map_err(|source| AppError::Yaml {
        path: path.clone(),
        source,
    })?;
    atomic_write(&path, &yaml)
}

// ============================================================================
//  FAVORITOS
// ============================================================================

/// Lee todos los favoritos del archivo. Mismas reglas que `load_apps`.
pub fn load_favs() -> Result<Vec<FavoritePath>> {
    let path = favs_file();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|source| AppError::Io {
        path: path.clone(),
        source,
    })?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed: FavoritesFile =
        serde_yaml::from_str(&text).map_err(|source| AppError::Yaml {
            path: path.clone(),
            source,
        })?;
    Ok(parsed.favorites)
}

/// Guarda todos los favoritos (escritura atómica).
pub fn save_favs(favs: &[FavoritePath]) -> Result<()> {
    ensure_data_dir()?;
    let path = favs_file();
    let payload = FavoritesFile {
        favorites: favs.to_vec(),
    };
    let yaml = serde_yaml::to_string(&payload).map_err(|source| AppError::Yaml {
        path: path.clone(),
        source,
    })?;
    atomic_write(&path, &yaml)
}

// ============================================================================
//  HELPERS PRIVADOS
// ============================================================================

/// Escribe `content` en `path` de forma atómica:
///   1. Crea `<path>.tmp` con todo el contenido.
///   2. Hace `rename` de `<path>.tmp` a `path` (atómico en Unix).
///
/// Si el rename falla, intenta borrar el `.tmp` para no dejar basura.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = with_extension(path, "tmp");
    fs::write(&tmp, content).map_err(|source| AppError::Io {
        path: tmp.clone(),
        source,
    })?;
    // `fs::rename` es atómico en el mismo filesystem en Linux/macOS.
    // Si quisiéramos ser muy prolijos podríamos hacer `sync_file_range`
    // antes del rename, pero para nuestro caso (archivos chiquitos) no
    // hace falta.
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(AppError::Io {
            path: path.to_path_buf(),
            source: e,
        });
    }
    Ok(())
}

/// Devuelve `<path>.<ext>` reemplazando cualquier extensión previa.
/// Helper para construir el path del `.tmp`.
fn with_extension(path: &Path, ext: &str) -> PathBuf {
    let mut p = path.to_path_buf();
    p.set_extension(ext);
    p
}

// ============================================================================
//  MIGRACIÓN DESDE LA VERSIÓN BASH
// ============================================================================

/// Si existe `~/.mytuis.yaml` (versión bash) y todavía no tenemos
/// `~/.mytuis/apps.yaml`, lo migramos:
///   1. Leemos el archivo viejo con serde_yaml.
///   2. Lo guardamos como `apps.yaml`.
///   3. Renombramos el viejo a `~/.mytuis.yaml.bak` (para no perder
///      datos por si algo sale mal).
///
/// Devuelve `Some(n)` con la cantidad de apps migradas si hubo
/// migración, o `None` si no había nada que migrar.
pub fn migrate_from_bash_if_needed() -> Result<Option<usize>> {
    let legacy = legacy_bash_file();
    let new = apps_file();

    if !legacy.exists() || new.exists() {
        // Nada que hacer: o no hay archivo bash, o ya migramos.
        return Ok(None);
    }

    // Aseguramos que el directorio destino exista antes de leer.
    ensure_data_dir()?;

    let text = fs::read_to_string(&legacy).map_err(|source| AppError::Io {
        path: legacy.clone(),
        source,
    })?;

    // El formato bash es idéntico al nuestro: un campo `apps:` con
    // una lista de structs `App`. Si por algún motivo cambió, este
    // parseo va a fallar con un error claro.
    let parsed: AppsFile = serde_yaml::from_str(&text).map_err(|source| AppError::Yaml {
        path: legacy.clone(),
        source,
    })?;

    let count = parsed.apps.len();
    save_apps(&parsed.apps)?;

    // Renombramos el viejo a `.bak`. Si el rename falla, no es grave:
    // el usuario ya tiene sus datos en el nuevo archivo, simplemente
    // le avisamos por stderr.
    let bak = legacy.with_extension("yaml.bak");
    if let Err(e) = fs::rename(&legacy, &bak) {
        eprintln!(
            "mytuis: no se pudo renombrar {:?} a {:?}: {e}",
            legacy, bak
        );
    }

    Ok(Some(count))
}

// ============================================================================
//  TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::now_string;

    /// Helper: crea un directorio temporal y devuelve su path. `tempfile`
    /// no está en las deps, así que usamos el tmp del sistema. Usamos
    /// un contador atómico para evitar colisiones cuando los tests
    /// corren en paralelo dentro del mismo segundo.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn tmp_dir() -> PathBuf {
        let base = std::env::temp_dir();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let unique = format!(
            "mytuis_test_{}_{}_{}",
            std::process::id(),
            now_string().replace([' ', ':', '-'], "_"),
            n
        );
        let dir = base.join(unique);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn apps_roundtrip_vacio() {
        let dir = tmp_dir();
        let file = dir.join("apps.yaml");
        save_apps_at(&file, &[]).unwrap();
        let loaded = load_apps_at(&file).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn apps_roundtrip_con_datos() {
        let dir = tmp_dir();
        let file = dir.join("apps.yaml");
        let apps = vec![
            App::new("nvim", "Editor", "/usr/bin/nvim", "-p", "2026-06-26 10:00:00"),
            App::new("ls", "listar", "/usr/bin/ls", "", "2026-06-26 10:00:01"),
        ];
        save_apps_at(&file, &apps).unwrap();
        let loaded = load_apps_at(&file).unwrap();
        assert_eq!(loaded, apps);
    }

    #[test]
    fn atomic_write_no_deja_archivos_tmp() {
        let dir = tmp_dir();
        let file = dir.join("apps.yaml");
        save_apps_at(&file, &[]).unwrap();
        // No debería quedar ningún `.tmp` colgando.
        assert!(!file.with_extension("tmp").exists());
    }

    /// Versiones de load/save parametrizadas para tests (no usan las
    /// rutas globales). Mantienen la misma lógica que las públicas
    /// pero reciben el path como argumento.
    fn save_apps_at(path: &Path, apps: &[App]) -> Result<()> {
        // En tests queremos poder escribir en paths arbitrarios, así
        // que creamos el directorio padre si no existe.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let payload = AppsFile { apps: apps.to_vec() };
        let yaml = serde_yaml::to_string(&payload)
            .map_err(|source| AppError::Yaml { path: path.to_path_buf(), source })?;
        atomic_write(path, &yaml)
    }

    fn load_apps_at(path: &Path) -> Result<Vec<App>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = fs::read_to_string(path).map_err(|source| AppError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let parsed: AppsFile =
            serde_yaml::from_str(&text).map_err(|source| AppError::Yaml {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(parsed.apps)
    }
}