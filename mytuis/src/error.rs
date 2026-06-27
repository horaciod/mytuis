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

use std::path::PathBuf;

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
}