# mytuis (Rust)

Gestor de aplicaciones y rutas favoritas con TUI (basada en
[`ratatui`](https://ratatui.rs)) y CLI. Reimplementación en Rust de
[`mytuis.sh`](../README.md) con una nueva entidad: **rutas favoritas**.

## Qué hay en este directorio

```
mytuis/
├── Cargo.toml          ← dependencias (ratatui, clap, serde, arboard, ...)
├── src/
│   ├── main.rs         ← entrypoint + dispatchers CLI
│   ├── cli.rs          ← definición clap de subcomandos
│   ├── config.rs       ← rutas de los YAML (~/.mytuis/)
│   ├── model.rs        ← structs App y FavoritePath
│   ├── resolve.rs      ← resolución de comandos y directorios
│   ├── storage.rs      ← load/save YAML atómico + migración bash
│   ├── open.rs         ← detección de terminal + clipboard
│   ├── error.rs        ← AppError (thiserror)
│   └── tui/
│       ├── mod.rs      ← bootstrap + state machine + event loop
│       ├── theme.rs    ← paleta de colores (212/39/82/196/214/240/255)
│       ├── list_view.rs← lista filtrable reutilizable
│       └── form.rs     ← forms modales add/edit
└── AGENTS.md           ← notas internas del proyecto
```

## Compilar e instalar

```bash
# Build release (un solo binario, ~2.6 MB)
cargo build --release

# El binario queda en target/release/mytuis. Copialo a tu $PATH:
install -m 755 target/release/mytuis /usr/local/bin/

# O simplemente corrélo desde acá:
./target/release/mytuis
```

## Uso rápido

### TUI

```bash
mytuis                  # abre la TUI (tab Apps por defecto)
```

Una sola pantalla con dos tabs arriba: **Apps** y **Favoritos**. Tab /
`←→` cambia de tab, `1`/`2` van directo a uno, las flechas o `j`/`k`
navegan, typing filtra, Enter abre el submenú del item seleccionado,
`a`/`e`/`d`/`r` agregan/editan/borran/ejecutan, `q` sale.

En el tab Favoritos la acción **"abrir terminal aquí"** lanza una
terminal nueva con `cwd` = el directorio del favorito. El submenú de
favoritos además tiene **"Copiar path al portapapeles"**.

### CLI

```bash
# Apps
mytuis apps list
mytuis apps add nvim "Editor modal" nvim
mytuis apps add lsl "Listado largo" "ls -lad"
mytuis apps remove nvim           # confirma en TTY
mytuis apps remove nvim --yes     # sin confirmación

# Favoritos (la nueva feature)
mytuis paths list
mytuis paths add pepe /datos/pepe -d "Repo principal"
mytuis paths add docs ~/Documents -d "Documentos"
mytuis paths get pepe             # → /datos/pepe (para `cd` en shell)
mytuis paths remove pepe

# Aliases de compatibilidad con la versión bash
mytuis list                       # ≡ mytuis apps list
mytuis add nvim "Editor" nvim     # ≡ mytuis apps add ...
mytuis remove nvim                # ≡ mytuis apps remove ...
```

### Shell integration (recomendado)

Pegá esto en tu `.bashrc` / `.zshrc`:

```bash
# cd a un favorito por nombre
cdfav() {
    local dir
    dir=$(mytuis paths get "$1" 2>/dev/null) && cd "$dir" \
        || echo "mytuis: favorito '$1' no encontrado"
}

# Abrir un favorito en nueva terminal
favterm() {
    mytuis >/dev/null 2>&1 &  # lanzamos la TUI para elegir
}
```

## Storage

La versión Rust guarda los datos en un **directorio** en vez de un
solo archivo:

```
~/.mytuis/
├── apps.yaml      ← apps (mismo formato que la versión bash)
└── favs.yaml      ← favoritos
```

### Migración automática desde `mytuis.sh`

Si tenés un `~/.mytuis.yaml` de la versión bash, **al primer
arranque** se importa automáticamente a `~/.mytuis/apps.yaml` y el
archivo original se renombra a `~/.mytuis.yaml.bak` para no perder
datos. Vas a ver un mensaje al stderr tipo:

```
mytuis: migradas 3 app(s) desde ~/.mytuis.yaml (backup en ~/.mytuis.yaml.bak)
```

## Formato de los archivos

`apps.yaml`:

```yaml
apps:
  - name: 'nvim'
    description: 'Editor modal'
    path: '/usr/bin/nvim'
    args: '-p'                 # opcional
    created: '2026-06-26 10:00:00'
    last_used: '2026-06-26 12:00:00'   # opcional
```

`favs.yaml`:

```yaml
favorites:
  - name: 'pepe'
    description: 'Repo principal'
    path: '/datos/pepe'
    created: '2026-06-26 11:00:00'
    last_used: '2026-06-26 12:30:00'   # opcional
```

Los campos opcionales se omiten del YAML cuando están vacíos (igual
que la versión bash, para mantener el archivo compacto).

## Features

- **TUI con dos tabs** (Apps y Favoritos), filtrable.
- **Formularios modales** para add/edit de ambas entidades.
- **Migración transparente** desde `mytuis.sh`.
- **Atomic writes**: los YAML se escriben primero a `.tmp` y después
  se hace `rename`, así un corte de luz a mitad de guardado no rompe
  el archivo.
- **Resolución de paths**: acepta `firefox` (busca en `$PATH`),
  `/usr/bin/firefox`, `~/bin/foo`, `./scripts/myscript.sh`.
- **Clipboard** vía `arboard` (X11/Wayland en Linux, NSPasteboard en
  macOS, OLE en Windows).
- **Detección de terminal** inteligente: respeta `$TERMINAL` y, si no,
  prueba `gnome-terminal`, `konsole`, `xfce4-terminal`, `alacritty`,
  `kitty`, `foot`, `wezterm`, `xterm` en ese orden.
- **Tests unitarios** con `cargo test` (15 tests sobre model/storage/
  resolve/open/config).

## Dependencias clave

| Crate | Versión | Para qué |
|-------|---------|----------|
| `ratatui` | 0.29 | TUI widgets + rendering |
| `crossterm` | 0.28 | Backend de terminal (raw mode, input) |
| `clap` | 4 (derive) | Parsing de CLI |
| `serde` + `serde_yaml` | 1 / 0.9 | (De)serialización YAML |
| `chrono` | 0.4 | Timestamps `created` / `last_used` |
| `arboard` | 3 | Portapapeles del sistema |
| `which` | 6 | Buscar ejecutables en `$PATH` |
| `dirs` | 5 | Localizar `$HOME` |
| `thiserror` + `anyhow` | 1 | Manejo de errores |

## Licencia

MIT. Mismo proyecto que `mytuis.sh`.