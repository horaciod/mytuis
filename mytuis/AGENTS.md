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
│   ├── lang.rs        # Lang enum + todas las traducciones (EN/ES)
│   ├── error.rs       # AppError (thiserror) + localized()
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

# 4b. i18n: mensajes en EN y ES.
rm -rf "$HOME/.mytuis"
LANG=en_US.UTF-8 ./target/release/mytuis apps add firefox "Browser" firefox
LANG=en_US.UTF-8 ./target/release/mytuis apps list | grep -q "Name" && echo "✓ EN"
LANG=es_AR.UTF-8 ./target/release/mytuis apps list | grep -q "Nombre" && echo "✓ ES"
LANG=en_US.UTF-8 MYTUIS_LANG=es ./target/release/mytuis apps remove noexiste -y \
    | grep -q "no se encontró" && echo "✓ override"

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
  por call, costo despreciable. Las closures **deben usar `move`** si
  capturan variables, porque el trait bound es `Box<dyn Fn>` con
  `'static`.
- **Meta entries en listas**: `fav_list` no es `ListView<FavoritePath>`
  sino `ListView<FavListItem>` donde `FavListItem` es un enum
  (`MetaOpenHere | Fav(FavoritePath)`). Esto permite que la meta entry
  `[↵] Open here` se filtre junto con los favoritos reales y respete
  el ordenamiento. Si agregás una nueva meta entry:
  1. Agregar variante al enum `FavListItem`.
  2. Extender `build_fav_list_items` para prependerla.
  3. Actualizar las closures de render y search.
  4. Actualizar el dispatch de `Enter` y `g` en `on_key_list`.
  5. Agregar label y strings de búsqueda en `lang.rs`.
- **Submenú de favoritos** tiene 5 acciones (Run, Edit, CopyPath,
  Delete, Back); el de apps tiene 4. El dispatch está en
  `Tui::open_submenu`.
- **Click-to-quit**: `q` siempre sale (excepto dentro de un form,
  donde cancela). `Ctrl+C` también.
- **Modal forms**: el form se renderiza encima de la lista de fondo
  con `Clear` widget para tapar lo de atrás.
- **Atajos `g` vs `c` en tab Favoritos**: ambas son teclas rápidas,
  pero hacen cosas distintas:
  - `g` (y Enter sobre la meta entry): **abre una terminal NUEVA**
    en el directorio del favorito vía `open_terminal_in`.
  - `c`: emite `cd <path>` al fd 3 (side channel estándar,
    mismo patrón que `broot`/`zoxide`/`fzf-cd-widget`) y sale.
    El shell padre (con el wrapper configurado) lee el fd 3 y
    hace `eval`, así el usuario termina parado en el directorio
    sin procesos extra. Si fd 3 está cerrado (sin wrapper), la
    TUI muestra un flash con el snippet para configurarlo y NO
    sale — ver "Shell wrapper fd 3" más abajo.
- **Footer tab-aware**: el footer cambia según el tab activo porque
  `c` solo aplica en Favoritos. La función es `lang::footer_list(is_favs)`
  y se llama desde `draw_footer` con `matches!(tab, Tab::Favs)`.

## Shell wrapper fd 3 (integración con el shell)

Para que la tecla `c` (y el subcomando `paths cd`) puedan cambiar
el directorio del shell padre, mytuis emite el comando al fd 3.
El usuario debe tener una función `mytuis` en su `.bashrc`/`.zshrc`:

```bash
mytuis() {
    local out
    out=$(command mytuis "$@" 3>&1 1>&2 2>&3)
    [ -n "$out" ] && eval "$out"
}
```

### Cómo funciona

1. `3>&1` dup-lica stdout del shell al fd 3 del proceso hijo.
2. `1>&2` mueve stdout del hijo al stderr (así la TUI no rompe).
3. mytuis escribe `cd <path>\n` al fd 3.
4. Cuando mytuis termina, `$(...)` recoge lo escrito a fd 3.
5. `eval "$out"` ejecuta el `cd` **en el shell padre** (no en el
   subshell de `$()`).

### Portabilidad

`/dev/fd/3` (que es lo que usa `open::emit_cd_to_fd3`) está
disponible en Linux y macOS. En Windows no funciona, pero mytuis
ya es Unix-first.

### Sin wrapper

Si fd 3 no está abierto (no hay wrapper), `emit_cd_to_fd3` devuelve
error y la TUI muestra un flash multilínea con el snippet del
wrapper. NO sale de la TUI — el usuario puede seguir navegando
y configurar el wrapper cuando quiera.

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

## Internacionalización

Todos los strings user-facing viven en `src/lang.rs`. **Si vas a agregar
un mensaje nuevo que el usuario final vea, va en `lang.rs`, no inline
en el código.**

### Jerarquía de detección (en `Lang::detect()`)

1. `$MYTUIS_LANG` (override del usuario)
2. `$LC_ALL`
3. `$LC_MESSAGES`
4. `$LANG`
5. Default: `English`

El parser `from_env_value` acepta formatos POSIX: `en`, `en_US`,
`en_US.UTF-8`, `es_ES@euro`. Solo el prefijo de dos letras importa.

### Cómo agregar un idioma (e.g. Français)

1. Agregar variante al enum `Lang` en `lang.rs`:

   ```rust
   pub enum Lang {
       En,
       Es,
       Fr,  // ← nuevo
   }
   ```

2. Agregar brazo a TODOS los `match self` que devuelven strings. El
   compilador te va a quejar en cada uno que te falte — aprovéchalo.
   Patrón:

   ```rust
   pub fn submenu_back(self) -> &'static str {
       match self {
           Lang::En => "Back",
           Lang::Es => "Volver",
           Lang::Fr => "Retour",
       }
   }
   ```

3. Agregar reconocimiento en `from_env_value`:

   ```rust
   "fr" | "fre" | "french" | "français" => Some(Lang::Fr),
   ```

4. Actualizar los tests en `lang.rs` (los que matchean `from_env_value`
   agregan un caso para `fr`).

5. Documentar en este AGENTS.md y en README.md.

### Convenciones en el código

- **TUI**: el struct `Tui` tiene un campo `pub lang: Lang`. Toda
  función de la TUI accede a `self.lang.xxx()` para obtener strings.
- **CLI**: `main()` detecta el idioma con `Lang::detect()` y lo pasa
  a cada handler. Todos los handlers reciben `lang: Lang` como primer
  argumento.
- **Errores**: cada `AppError` tiene un método `localized(lang)` que
  devuelve el mensaje traducido. `main()` lo usa al imprimir errores
  al stderr.
- **Tests**: NO asumas el idioma en tests. Si testeás un mensaje que
  depende del idioma, usá `lang.msg_xxx()` explícitamente. Los tests
  de `lang.rs` sí asumen ambos idiomas (es su trabajo).

### Lo que NO se traduce (a propósito)

- `clap --help`: clap no soporta i18n nativo.
- Nombres de subcomandos (`apps`, `paths`, `list`): son API pública.
- Nombres de campos YAML: son storage interno.
- Comentarios del código: son para devs.
- Strings de debugging / `eprintln!("raw mode: ...")` en TUI setup.

### Tests de detección

Los tests de `lang.rs` testean `from_env_value` directamente, no
`detect()` (porque `detect()` lee variables de entorno del proceso, lo
que es frágil en tests). Si necesitás testear el comportamiento end-to-end,
setteá las variables explícitamente antes de llamar a `Lang::detect()`.

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

# Feature nuevo: 'paths cd' emite `cd <path>` al fd 3.
./target/release/mytuis paths add pepe /tmp -d "tmp dir"
./target/release/mytuis paths cd pepe  # debe fallar: fd 3 no está abierto
# End-to-end con wrapper Python (verifica fd 3):
python3 -c "
import os, subprocess, time
r, w = os.pipe()
p = subprocess.Popen(
    ['./target/release/mytuis', 'paths', 'cd', 'pepe'],
    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
    preexec_fn=lambda: (os.dup2(w, 3), os.close(w)),
    close_fds=False,
)
os.close(w)
out = b''
os.set_blocking(r, False); time.sleep(0.1)
while True:
    try: chunk = os.read(r, 4096)
    except BlockingIOError: break
    if not chunk: break
    out += chunk
err, _ = p.communicate()
assert out == b'cd /tmp\n', f'got {out!r}'
print('✓ fd 3 OK')
"
./target/release/mytuis paths remove pepe -y
echo "✓ OK"
```

## Estado del proyecto

- 61 tests unitarios (todos en verde). Agregados en la iteración de
  Tools: 4 model (roundtrip + last_used == created + description
  omit), 2 storage (tools roundtrip vacío + con datos), 5 resolve
  (`resolve_tool_url` http/https, espacios, vacía, esquema inválido,
  host vacío), 2 lang (strings presentes en ambos idiomas +
  `tab_section_hint` incluye `3`), 6 tui (selección con tecla `3`,
  `tab_toggle` cicla por tres tabs, frame con tools, frame tools
  vacío, filtro por URL, submenu de 4 acciones).
- CLI completo: apps list/add/remove, paths list/add/remove/get/go/cd,
  **tools list/add/remove/run**.
- TUI con **3 tabs** (Apps, Favoritos, Tools) + forms + submenús +
  clipboard + meta entry `[↵] Open here` + atajos `g` (terminal
  nueva) y `c` (cd + salir vía fd 3) + atajo `3` para ir directo al
  tab Tools.
- **Tools** son URLs (http/https) que se abren con el opener del
  sistema (`xdg-open` → `gio open` → `open`). Al crear un tool,
  `last_used = created` (decisión explícita del plan: el tool
  "nació" en ese momento).
- Migración desde bash implementada y probada.
- Integración con shell vía fd 3 (patrón `broot`/`zoxide`):
  verificada end-to-end con wrapper Python.
- Internacionalización EN/ES con detección automática de locale.
- Sin CI ni formatter configurados.