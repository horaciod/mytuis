//! # Formularios modales (Add / Edit)
//!
//! Cuando el usuario aprieta `a` (agregar) o `e` (editar), abrimos un
//! formulario modal en el centro de la pantalla. Cada formulario tiene
//! una serie de campos de texto:
//!
//! - El usuario navega entre campos con Tab / Shift+Tab.
//! - Escribe caracteres que se insertan en el campo actual.
//! - Backspace borra.
//! - Enter confirma y aplica.
//! - Esc cancela sin aplicar.
//!
//! Mantenemos los formularios como **estado mutable en `App`** (ver
//! `mod.rs`) en vez de devolver un struct de un método. Esto evita
//! problemas de borrowing dentro del event loop (Rust te obliga a
//! pensar bien quién es dueño de qué).
//!
//! ## Cómo se dibuja
//!
//! Cada formulario sabe:
//! 1. Sus campos (lista de tuplas `(label, valor)`).
//! 2. Qué campo está activo (índice).
//!
//! El renderer (`render_form`) los muestra en un bloque centrado con
//! el campo activo resaltado.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::theme;

/// Tipos de campos que puede tener un formulario. Para mantener
/// simple el ejemplo, todos son strings (la versión bash hacía lo
/// mismo). Si después agregamos checkboxes o selects, se extiende acá.
pub type Field = String;

/// Estado de un formulario en la TUI.
pub struct FormState {
    /// Título que se muestra en el borde superior.
    pub title: String,
    /// Hint en el borde inferior (atajos).
    pub hint: String,
    /// Labels de cada campo, en orden.
    pub labels: Vec<&'static str>,
    /// Valores actuales. Mismo largo que `labels`.
    pub values: Vec<Field>,
    /// Índice del campo activo.
    pub active: usize,
}

impl FormState {
    /// Crea un formulario nuevo a partir de los labels y valores
    /// iniciales.
    pub fn new(
        title: impl Into<String>,
        hint: impl Into<String>,
        labels: Vec<&'static str>,
        values: Vec<Field>,
    ) -> Self {
        assert_eq!(labels.len(), values.len(), "labels y values deben tener el mismo largo");
        Self {
            title: title.into(),
            hint: hint.into(),
            labels,
            values,
            active: 0,
        }
    }

    /// Cantidad de campos.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// `true` si no hay campos (raro, pero por completitud).
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Mueve el foco al siguiente campo (wrap-around).
    pub fn next_field(&mut self) {
        if !self.values.is_empty() {
            self.active = (self.active + 1) % self.values.len();
        }
    }

    /// Mueve el foco al campo anterior.
    pub fn previous_field(&mut self) {
        if !self.values.is_empty() {
            self.active = if self.active == 0 {
                self.values.len() - 1
            } else {
                self.active - 1
            };
        }
    }

    /// Inserta un caracter en el campo activo.
    pub fn insert_char(&mut self, c: char) {
        self.values[self.active].push(c);
    }

    /// Borra el último caracter del campo activo.
    pub fn backspace(&mut self) {
        self.values[self.active].pop();
    }

    /// Devuelve el valor del campo `i` (por referencia).
    pub fn get(&self, i: usize) -> &str {
        &self.values[i]
    }

    /// Devuelve una copia de los valores en orden.
    pub fn values(&self) -> Vec<String> {
        self.values.clone()
    }
}

/// Dibuja un formulario modal centrado en pantalla.
///
/// `error` (opcional) muestra un mensaje de error debajo del form.
pub fn render_form(
    frame: &mut Frame,
    form: &mut FormState,
    error: Option<&str>,
) {
    let area = centered_rect(70, 60, frame.size());

    // Limpiamos el área primero (sin esto se vería el contenido de
    // atrás a través del modal).
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::highlighted_border_style())
        .title(format!(" {} ", form.title))
        .title_bottom(format!(" {} ", form.hint));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Layout vertical: una línea por campo, más una línea opcional de
    // error al final.
    let rows: Vec<Constraint> = form
        .labels
        .iter()
        .map(|_| Constraint::Length(3))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(rows)
        .split(inner);

    for (i, label) in form.labels.iter().enumerate() {
        let active = i == form.active;
        let border_style = if active {
            theme::highlighted_border_style()
        } else {
            theme::muted_border_style()
        };
        // Mostramos el cursor con "_" al final del valor si está
        // activo. ratatui no tiene cursor nativo, así que lo simulamos.
        let display_value = if active {
            format!("{}_", form.values[i])
        } else {
            form.values[i].clone()
        };

        let p = Paragraph::new(display_value)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(format!(" {label} ")),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(p, chunks[i]);
    }

    if let Some(msg) = error {
        let p = Paragraph::new(msg)
            .style(theme::error_style())
            .wrap(Wrap { trim: false });
        frame.render_widget(p, chunks[form.len()]);
    }
}

/// Centra un rectángulo de `percent_x` por `percent_y` en el área
/// dada. Helper clásico de ratatui.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}