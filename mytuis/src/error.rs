//! # Manejo de errores
//!
//! Acá definimos el tipo `AppError` que se usa en todo el programa.
//! Rust obliga a manejar los errores explícitamente: si una función puede
//! fallar, su firma devuelve un `Result<T, AppError>` en vez de tirar
//! excepciones como en otros lenguajes.
//!
//! `thiserror` nos permite derivar automáticamente la implementación del
//! trait `std::error::Error` y la conversión a `String` para que se pueda
//! mostrar al usuario con `{}` o `{:?}`.

use std::path::{Path, PathBuf};

use crate::lang::Lang;

/// `AppError` es un enum donde cada variante representa una clase distinta
/// de fallo. Esto es más útil que un único "string error" porque permite
/// al call site decidir cómo reaccionar (por ejemplo, mostrar un mensaje
/// distinto para `NotFound` que para `Io`).
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Fallo de I/O al leer o escribir un archivo. Envolvemos el error
    /// original (`#[from]`) para que `?` lo convierta automáticamente.
    #[error("error de E/S en {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// El YAML está corrupto o tiene un formato inesperado.
    #[error("error al parsear YAML en {path:?}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    /// El usuario pidió una app / favorito por nombre y no existe.
    #[error("no se encontró una entrada llamada '{0}'")]
    NotFound(String),

    /// El usuario tipeó un nombre duplicado al hacer `add`.
    #[error("ya existe una entrada llamada '{0}'")]
    Duplicate(String),

    /// No se pudo resolver un comando a un archivo ejecutable.
    #[error("no se pudo resolver el comando '{0}'")]
    InvalidCommand(String),

    /// No se pudo resolver un path (por ejemplo, un directorio que no
    /// existe al guardarlo como favorito).
    #[error("no se pudo resolver el path '{0}'")]
    InvalidPath(String),

    /// No se pudo validar una URL de tool (vacía, esquema no soportado,
    /// etc.). El string es la URL original que pasó el usuario.
    #[error("URL inválida '{0}'")]
    InvalidUrl(String),

    /// No se detectó ningún emulador de terminal instalado en el sistema.
    #[error(
        "no se encontró un emulador de terminal. Probá definir la variable \
         $TERMINAL con la ruta a tu terminal favorito (ej. export TERMINAL=alacritty)"
    )]
    NoTerminal,

    /// Cualquier otro error que no encaja en las variantes anteriores.
    /// Útil para `from()` de errores de librerías externas que no queremos
    /// tipar uno por uno.
    #[error("{0}")]
    Other(String),
}

/// Alias de `Result` con nuestro tipo de error por defecto. Es una
/// convención muy común en Rust para no escribir `Result<T, AppError>`
/// una y otra vez.
pub type Result<T> = std::result::Result<T, AppError>;

/// Helper para construir errores "Other" rápidamente con `Into`.
/// Útil cuando `?` no aplica porque el tipo del error de origen no se
/// puede inferir solo.
impl AppError {
    pub fn other<S: Into<String>>(msg: S) -> Self {
        AppError::Other(msg.into())
    }

    /// Devuelve un mensaje **localizado** para mostrar al usuario.
    /// El `Display` que `thiserror` deriva queda en inglés (es útil
    /// para `Debug`, tests y logs); este método es para la UI final.
    pub fn localized(&self, lang: Lang) -> String {
        match self {
            AppError::Io { path, .. } => lang.err_io(path),
            AppError::Yaml { path, .. } => lang.err_yaml(path),
            AppError::NotFound(name) => lang.err_not_found(name),
            AppError::Duplicate(name) => lang.err_duplicate(name),
            AppError::InvalidCommand(cmd) => lang.err_invalid_command(cmd),
            AppError::InvalidPath(path) => lang.err_invalid_path(path),
            AppError::InvalidUrl(url) => lang.err_invalid_url(url),
            AppError::NoTerminal => lang.err_no_terminal(),
            AppError::Other(msg) => msg.clone(),
        }
    }
}