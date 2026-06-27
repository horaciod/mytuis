//! # Modelos de datos
//!
//! Acá definimos las structs que representan las dos entidades que maneja
//! `mytuis`:
//!
//! * `App`         — un ejecutable guardado en el catálogo de apps.
//! * `FavoritePath` — un directorio favorito (atajo a una carpeta).
//!
//! Cada struct tiene el atributo `#[derive(Serialize, Deserialize)]` de
//! `serde`, lo que permite convertirla a/desde YAML con muy poco código.
//! Los atributos `#[serde(...)]` controlan exactamente cómo se serializa:
//! por ejemplo, `skip_serializing_if = "String::is_empty"` hace que el
//! campo no aparezca en el YAML cuando está vacío, manteniendo los
//! archivos prolijos.

use serde::{Deserialize, Serialize};

/// `App` representa una aplicación guardada en el catálogo.
///
/// Notá el formato del YAML final:
///
/// ```yaml
/// apps:
///   - name: 'nvim'
///     description: 'Editor modal'
///     path: '/usr/bin/nvim'
///     args: '-p'                # opcional, se omite si está vacío
///     created: '2026-06-26 10:00:00'
///     last_used: '2026-06-26 12:00:00'   # opcional, se omite si está vacío
/// ```
///
/// Es exactamente el mismo formato que escribía la versión bash
/// (`mytuis.sh`), lo que nos permite migrar de un lado al otro sin
/// transformaciones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct App {
    /// Nombre corto y único. Es la "key" con la que identificamos a la
    /// app en la CLI (`mytuis apps remove nvim`).
    pub name: String,

    /// Descripción libre. Se muestra en la lista y en la tarjeta de
    /// lanzamiento.
    pub description: String,

    /// Path **absoluto** al ejecutable. Se resuelve al guardar la app
    /// (acepta `firefox`, `/usr/bin/firefox`, `~/bin/foo`, etc.).
    pub path: String,

    /// Argumentos opcionales que se le pasan al ejecutable al lanzar.
    /// String vacía si no hay argumentos (y se omite del YAML).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub args: String,

    /// Timestamp de creación, en formato `"YYYY-MM-DD HH:MM:SS"`. Lo
    /// seteamos al guardar la app por primera vez.
    pub created: String,

    /// Timestamp de la última vez que se ejecutó. Se actualiza cada vez
    /// que lanzamos la app desde la TUI o la CLI.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_used: String,
}

impl App {
    /// Construye una app nueva con `created = now` y sin `last_used`.
    /// Útil tanto desde la CLI como desde el form de la TUI.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        path: impl Into<String>,
        args: impl Into<String>,
        created: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            path: path.into(),
            args: args.into(),
            created: created.into(),
            last_used: String::new(),
        }
    }
}

/// `FavoritePath` representa un directorio favorito. A diferencia de
/// `App`, no tiene un ejecutable asociado: solo guarda un path que el
/// usuario quiere recordar/atacar rápidamente desde la shell.
///
/// ```yaml
/// favorites:
///   - name: 'pepe'
///     description: 'Repo principal'      # opcional, se omite si está vacío
///     path: '/datos/pepe'
///     created: '2026-06-26 11:00:00'
///     last_used: '2026-06-26 12:30:00'   # opcional
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FavoritePath {
    /// Nombre corto y único.
    pub name: String,

    /// Descripción libre. Sirve para recordar para qué es la carpeta.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,

    /// Path **absoluto** al directorio. Se resuelve al guardar (acepta
    /// `~/proyectos`, `./local`, etc.).
    pub path: String,

    /// Timestamp de creación.
    pub created: String,

    /// Timestamp del último uso (se actualiza cuando "abrimos la
    /// terminal acá" desde la TUI).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_used: String,
}

impl FavoritePath {
    /// Constructor conveniente.
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        path: impl Into<String>,
        created: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            path: path.into(),
            created: created.into(),
            last_used: String::new(),
        }
    }
}

/// Formato del contenedor raíz del YAML de favoritos. Lo definimos como
/// struct para que serde pueda (des)serializarlo con un campo
/// `favorites: Vec<FavoritePath>`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct FavoritesFile {
    #[serde(default)]
    pub favorites: Vec<FavoritePath>,
}

/// Ídem para apps: el archivo raíz tiene un campo `apps: Vec<App>`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppsFile {
    #[serde(default)]
    pub apps: Vec<App>,
}

/// Helper para producir el timestamp actual en el formato
/// `"YYYY-MM-DD HH:MM:SS"`. Lo centralizamos acá porque la versión bash
/// lo hacía con `date "+%Y-%m-%d %H:%M:%S"`.
pub fn now_string() -> String {
    // `chrono::Local::now()` devuelve la hora local del sistema. Le
    // pedimos el formato exacto que usaba la versión bash.
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_roundtrip_yaml() {
        let app = App::new("nvim", "Editor modal", "/usr/bin/nvim", "-p", "2026-06-26 10:00:00");
        let yaml = serde_yaml::to_string(&AppsFile { apps: vec![app.clone()] }).unwrap();
        let parsed: AppsFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.apps.len(), 1);
        assert_eq!(parsed.apps[0], app);
    }

    #[test]
    fn favorite_roundtrip_yaml() {
        let fav = FavoritePath::new("pepe", "Repo", "/datos/pepe", "2026-06-26 11:00:00");
        let yaml = serde_yaml::to_string(&FavoritesFile { favorites: vec![fav.clone()] }).unwrap();
        let parsed: FavoritesFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.favorites.len(), 1);
        assert_eq!(parsed.favorites[0], fav);
    }

    #[test]
    fn empty_args_omitidos_del_yaml() {
        let app = App::new("ls", "listar", "/usr/bin/ls", "", "2026-06-26 10:00:00");
        let yaml = serde_yaml::to_string(&AppsFile { apps: vec![app] }).unwrap();
        // El campo `args:` no debería aparecer.
        assert!(!yaml.contains("args:"), "yaml debería omitir args vacío: {yaml}");
    }
}