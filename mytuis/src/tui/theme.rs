//! # Paleta de colores (theme)
//!
//! Centralizamos acá todos los `Color` de la TUI. La idea es que si
//! mañana queremos cambiar el esquema, tocamos un solo archivo.
//!
//! Los números vienen de la versión bash original: usaba códigos de
//! color ANSI 256 de `gum`, que es lo mismo que `Color::Indexed(n)` en
//! ratatui. Mantenemos los mismos números para que la transición
//! visual sea familiar.

use ratatui::style::{Color, Modifier, Style};

/// Color primario: rosa/magenta fuerte. Lo usamos para títulos,
/// items seleccionados, borders destacados.
pub const PINK: Color = Color::Indexed(212);

/// Cyan brillante. Subtítulos, bordes de tarjetas.
pub const CYAN: Color = Color::Indexed(39);

/// Verde. Mensajes de éxito.
pub const GREEN: Color = Color::Indexed(82);

/// Rojo brillante. Errores.
pub const RED: Color = Color::Indexed(196);

/// Naranja. Advertencias.
pub const ORANGE: Color = Color::Indexed(214);

/// Gris medio. Texto secundario, hints, labels.
pub const GRAY: Color = Color::Indexed(240);

/// Blanco. Texto normal.
pub const WHITE: Color = Color::Indexed(255);

/// Estilo del título principal (header).
pub fn title_style() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

/// Estilo de un subtítulo / header de sección.
pub fn subtitle_style() -> Style {
    Style::default().fg(CYAN)
}

/// Estilo del item seleccionado en una lista.
pub fn selected_style() -> Style {
    Style::default().fg(PINK).add_modifier(Modifier::BOLD)
}

/// Estilo del item normal (no seleccionado) en una lista.
pub fn normal_style() -> Style {
    Style::default().fg(WHITE)
}

/// Estilo de texto secundario (descripciones, hints).
pub fn muted_style() -> Style {
    Style::default().fg(GRAY)
}

/// Estilo de mensaje de error.
pub fn error_style() -> Style {
    Style::default().fg(RED).add_modifier(Modifier::BOLD)
}

/// Estilo de mensaje de éxito.
pub fn success_style() -> Style {
    Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
}

/// Estilo de advertencia.
pub fn warning_style() -> Style {
    Style::default().fg(ORANGE)
}

/// Borde "resaltado" (tabs activos, popups).
pub fn highlighted_border_style() -> Style {
    Style::default().fg(PINK)
}

/// Borde "apagado" (tabs inactivos, popups secundarios).
pub fn muted_border_style() -> Style {
    Style::default().fg(GRAY)
}