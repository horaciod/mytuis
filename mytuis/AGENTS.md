# AGENTS.md — mytuis (Rust)

## Qué es este directorio

Reimplementación en Rust del `mytuis.sh` que vive en el directorio
padre (`/datos/tui/`). Mismo concepto (catálogo de apps) más una nueva
entidad: **rutas favoritas**. La TUI usa `ratatui` + `crossterm` en
vez de `gum`.

## Estructura del proyecto

```
mytuis/
├── Cargo.toml
├── README.md          # user-facing
├── AGENTS.md          # este archivo
├── src/
│   ├── main.rs        # entrypoint + dispatchers CLI
│   ├── cli.rs         # clap definitions
│   ├── config.rs      # rutas (~/.mytuis/)
│   ├── model.rs       # structs App, FavoritePath
│   ├── resolve.rs     # resolve_command + resolve_favorite_dir
│   ├── storage.rs     # YAML load/save + migrate_from_bash_if_needed
│   ├── open.rs        # open_terminal_in + copy_to_clipboard
│   ├── error.rs       # AppError (thiserror)
│   └── tui/
│       ├── mod.rs     # state machine, event loop, ui()
│       ├── theme.rs   # paleta
│       ├── list_view.rs
│       └── form.rs
└── target/            # ignorado (build artifacts)
```

## Cómo validar cambios

```bash
cd /datos/tui/mytuis

# 1. Compilar.
cargo build --release

# 2. Tests unitarios.
cargo test

# 3. Smoke test CLI con HOME temporal.
export HOME=/tmp/mytuis_smoke
rm -rf "$HOME/.mytuis" "$HOME/.mytuis.yaml"
mkdir -p "$HOME"
./target/release/mytuis apps add firefox "Web browser" firefox
./target/release/mytuis apps add lsl "Listado largo" "ls -lad"
./target/release/mytuis apps list
./target/release/mytuis paths add pepe /tmp -d "tmp"
./target/release/mytuis paths list
./target/release/mytuis paths get pepe
./target/release/mytuis apps remove firefox -y

# 4. Migración desde bash:
cat > "$HOME/.mytuis.yaml" <<'YAML'
apps:
  - name: 'bash'
    path: '/usr/bin/bash'
    created: '2026-06-26 10:00:00'
YAML
./target/release/mytuis apps list   # debe migrar y listar
[ -f "$HOME/.mytuis.yaml.bak" ] && echo "✓ backup OK"

# 5. TUI: visual; el frame no se renderiza con `script -qc` pero
#    se puede confirmar que arranca sin panic:
TERM=xterm-256color timeout 1 script -qc "./target/release/mytuis" \
    /dev/null </dev/null 2>&1 | grep -q "1049h" && echo "✓ TUI arrancó"
```

## Convenciones internas que importan

- **Almacenamiento en directorio**: `~/.mytuis/{apps,favs}.yaml`. NO
  volver al archivo único — la migración desde bash ya está cableada.
- **Escritura atómica**: `storage::atomic_write` escribe a `.tmp` y
  hace rename. NO hacer `fs::write` directo a la ruta final.
- **`args` opcional**: se omite del YAML cuando está vacío (mismo
  criterio que el bash). `serde` lo maneja con `skip_serializing_if`.
- **Comentarios**: el código está exhaustivamente comentado en
  español porque el usuario es novato en Rust. Mantener ese nivel.
- **TUI struct vs model struct**: el struct de UI se llama `Tui`
  (no `App`) para no colisionar con `crate::model::App`. Si agregás
  nuevos entities, mantener la convención: `Tui` para el estado de UI,
  nombres específicos para los datos.
- **Aliases top-level**: `mytuis list`/`add`/`remove` se reescriben
  a `mytuis apps list`/etc. en `rewrite_legacy_argv` antes de pasar
  el argv a clap. Si modificás subcomandos, actualizá también esa
  función.

## TUI: detalles no triviales

- **Dos fases de setup/restauración**: en `tui::run` activamos raw
  mode + alternate screen. Pase lo que pase, los desactivamos. Es
  crítico porque una terminal cruda sin restaurar deja al usuario
  sin prompt ni echo.
- **`ListView<I>` es genérico**: las closures para render y filtro
  se guardan como `Box<dyn Fn>` porque las structs genéricas no
  pueden guardar genéricos. Trade-off conocido: una indirección
  por call, costo despreciable.
- **Submenú de favoritos** tiene 5 acciones (Run, Edit, CopyPath,
  Delete, Back); el de apps tiene 4. El dispatch está en
  `Tui::open_submenu`.
- **Click-to-quit**: `q` siempre sale (excepto dentro de un form,
  donde cancela). `Ctrl+C` también.
- **Modal forms**: el form se renderiza encima de la lista de fondo
  con `Clear` widget para tapar lo de atrás.

## Ratatui gotchas

- **`use ListState::default().with_selected(...)`** — ListState
  requiere esto, no `Some(idx)` directo.
- **`render_stateful_widget`** vs `render_widget**: las listas
  usan la variante stateful porque necesitan trackear selección.
- **`Block::default().borders(...)`** + `border_style` controla el
  color del borde. El color del contenido va por `Span::styled` o
  `Paragraph::style`.
- **Tabs widget**: el highlight se aplica con `.highlight_style()`,
  no con `style()` del título.
- **`Paragraph::alignment(Alignment::Center)`** centra texto
  horizontalmente.
- **Frame size**: `frame.size()` puede ser `Rect { width: 0, ... }`
  en tests. Validar antes de hacer layouts complejos.

## Dependencias y cómo se usan

| Crate | Punto de uso | Notas |
|-------|--------------|-------|
| `ratatui` | `tui/mod.rs`, `tui/list_view.rs`, `tui/form.rs`, `tui/theme.rs` | Widgets |
| `crossterm` | `tui/mod.rs` (run/run_loop) | Backend + raw mode + events |
| `clap` | `cli.rs`, `main.rs` | Parser CLI con derive |
| `serde` | `model.rs`, `storage.rs` | Serialize/Deserialize |
| `serde_yaml` | `storage.rs` | (De)serialización YAML |
| `chrono` | `model.rs` | Timestamps |
| `arboard` | `open.rs` | Clipboard |
| `which` | `resolve.rs`, `open.rs` | Buscar binarios en $PATH |
| `dirs` | `config.rs`, `resolve.rs` | Localizar $HOME |
| `thiserror` | `error.rs` | Derivar `Error` |
| `anyhow` | `main.rs`, `storage.rs` | Errors dinámicos en main |

## Si agregás una nueva entidad (ej. "scripts", "aliases")

1. Nuevo struct en `model.rs` con `Serialize/Deserialize`.
2. Container raíz (ej. `ScriptsFile { scripts: Vec<Script> }`) igual
   que `AppsFile`/`FavoritesFile`.
3. Funciones `load_X`/`save_X` en `storage.rs`.
4. Nuevo `XCmd` enum en `cli.rs` + handler en `main.rs`.
5. Nuevo tab en `tui::Tab` + `Tui::on_key_list` con su match arm.
6. Submenú con sus acciones específicas en `tui::SubAction`.
7. Form en `tui::FormKind` con `open_add_form`/`open_edit_form`/
   `submit_form`.
8. Documentar en README.

## Si agregás un campo nuevo a un struct existente

1. Modificar el struct (con `#[serde(default)]` si es opcional).
2. Si el campo debe omitirse del YAML cuando vacío, agregar
   `skip_serializing_if = "String::is_empty"` (o similar).
3. Si es opcional y se lee desde el bash legacy, **NO** es problema:
   serde_yaml devuelve el `default` cuando el campo no está.
4. Actualizar los tests de `model.rs` (round-trip).

## Smoke test que vale la pena correr antes de commit

```bash
export HOME=/tmp/mytuis_smoke && rm -rf "$HOME/.mytuis" "$HOME/.mytuis.yaml" && mkdir -p "$HOME"
./target/release/mytuis --version
./target/release/mytuis --help
./target/release/mytuis apps list
./target/release/mytuis apps add firefox "Web browser" firefox
./target/release/mytuis apps add lsl "ls -lad" "ls -lad"
./target/release/mytuis apps list
./target/release/mytuis apps remove firefox -y
./target/release/mytuis paths add /tmp -d "tmp del sistema" tmpdir || \
    ./target/release/mytuis paths add tmpdir /tmp -d "tmp del sistema"
./target/release/mytuis paths list
./target/release/mytuis paths get tmpdir
./target/release/mytuis paths remove tmpdir -y
echo "✓ OK"
```

## Estado del proyecto

- 15 tests unitarios (todos en verde).
- CLI completo (apps list/add/remove, paths list/add/remove/get).
- TUI con 2 tabs + forms + submenús + clipboard.
- Migración desde bash implementada y probada.
- Sin CI ni formatter configurados.