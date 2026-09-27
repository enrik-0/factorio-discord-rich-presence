# Factorio Discord Rich Presence

Muestra en tu perfil de Discord a qué estás jugando en Factorio: el save, el planeta,
la investigación en curso, cuántas tecnologías llevas y el tiempo jugado de la partida.

> **Aviso: el mod desactiva los logros de Steam.** No es cosa de este mod — Factorio
> desactiva los logros con *cualquier* mod activo. Si te importan, puedes usar sólo la
> aplicación sin instalar el mod: seguirás viendo el save y el tiempo de sesión, pero no
> el planeta ni la investigación.

## Por qué son dos piezas

El sandbox de Lua de Factorio no tiene sockets, ni HTTP, ni acceso al sistema de ficheros
fuera de `script-output`, y los desarrolladores han dicho que nunca lo tendrá porque
rompería el determinismo en multijugador. Así que:

- **El mod** recolecta el estado y lo escribe en `script-output/discord-rp/state.json`.
- **La aplicación** lee ese fichero y habla con el IPC de Discord.

## Requisitos

- Windows, y Factorio **2.1** (el mod declara `factorio_version: "2.1"`; Factorio no
  tiene compatibilidad hacia delante, así que no carga en 2.0)
- Si juegas a Factorio desde Steam, no hace falta nada más: el instalador lo detecta
  solo.

## Instalación

1. Descarga el instalador (`FactorioDiscordRP-Setup-*.exe`) desde
   [Releases](https://github.com/enrik-0/factorio-discord-rich-presence/releases) y
   ejecútalo. Es un instalador sin firmar, así que Windows puede avisar con
   *"Windows protegió su PC"*: pulsa **Más información → Ejecutar de todas formas**.
2. Elige cómo quieres que se abra:
   - **Con Factorio, desde Steam** (recomendado): se abre al lanzar el juego y se
     cierra con él. El instalador configura las opciones de lanzamiento por ti.
   - **Al iniciar Windows**: queda en la bandeja del sistema.
   - **Manual**: el instalador te enseña la línea completa para pegarla tú mismo en
     Steam → Factorio → Propiedades → Opciones de lanzamiento.
3. Instala el mod «Discord Rich Presence» desde el
   [Mod Portal](https://mods.factorio.com/mod/discord-rich-presence) o desde el
   propio juego, en *Mods*. Sin él, la aplicación sigue funcionando en modo
   degradado: publica el save y el tiempo jugado, pero no el planeta ni la
   investigación.
4. Juega. Comprueba la tarjeta **desde otra cuenta de Discord**: tu propio perfil no
   la muestra completa.

Para desinstalarla, usa *Agregar o quitar programas*: también retira lo que haya
puesto en las opciones de lanzamiento de Steam, sin tocar el resto de tus opciones.

Dentro del juego, `/drp-debug` imprime los datos que está publicando el mod, para
contrastarlos con el árbol de tecnologías.

## Ajustes del mod

**Tú eliges qué se ve, desde dentro del juego.** El fichero de estado nunca sale de
tu equipo, así que elegir qué se muestra es también el control de privacidad: lo
único que ven los demás es la tarjeta de Discord.

Cada campo tiene un hueco fijo, indicado en la descripción de su ajuste. Así ninguna
casilla puede activarse sin que aparezca nada.

| Ajuste | Hueco | Por defecto |
|---|---|---|
| Nombre de la partida | línea 1 | sí |
| Planeta | línea 1 | sí |
| Modpack principal | línea 1 | no |
| Investigación en curso | línea 2 | sí |
| Contador de tecnologías | al pasar el ratón | sí |
| Factor de evolución | al pasar el ratón | no |
| Cohetes lanzados | al pasar el ratón | sí, oculto mientras sean 0 |
| Número de mods | al pasar el ratón | no |
| Un jugador / multijugador | al pasar el ratón | sí |
| Mi nombre de jugador | al pasar el ratón | no |
| Dirección del servidor | al pasar el ratón | **no** |
| Cronómetro | — | tiempo de la partida |

Los ajustes son **por jugador**, así que en multijugador cada uno decide lo suyo.
El intervalo de escritura es global, porque el temporizador es único para toda la
partida.

Si un hueco se pasa de los 128 caracteres de Discord, se caen los campos de menor
prioridad en vez de cortar a mitad de palabra.

### Modo avanzado: plantillas

Si prefieres decidir tú el reparto, define una sección `[templates]` en
`config.toml`. Entonces mandan las plantillas y se ignoran las casillas del mod
— no pueden ser autoridad las dos a la vez. Ver `app/config.example.toml`.

## Privacidad

La dirección del servidor lleva **doble llave**: hay que activarla en los ajustes
del mod *y* en `config.toml`. Es el único campo que expone algo de fuera de la
partida, así que la aplicación mantiene un veto por encima del mod.

El resto se controla desde los ajustes del mod, dentro del juego.

## Desarrollo

Para compilar y ejecutar desde el código en vez de usar el instalador:

- Rust estable, para compilar la aplicación.
- Python 3, sólo para el script de empaquetado del mod.
- Inno Setup 6, sólo para compilar el instalador (`installer/factorio-discord-rp.iss`).

### Crear la aplicación de Discord

Sólo hace falta una vez, si no vas a usar el instalador (que ya trae una incluida):

1. Entra en <https://discord.com/developers/applications> y pulsa **New Application**.
2. Llámala exactamente `Factorio` — ese nombre es lo que Discord muestra como
   *"Jugando a ..."*.
3. Copia el **Application ID** de *General Information*.
4. En *Rich Presence → Art Assets* sube **una sola imagen**, el logo de Factorio,
   con la clave `factorio`. No hace falta un icono por planeta: el planeta se lee
   en el texto de la tarjeta.

### Configurar y ejecutar

```bash
cp app/config.example.toml config.toml
```

Pon el Application ID en `config.toml`. Para una prueba rápida sirve la variable
de entorno `FACTORIO_DRP_APP_ID`.

```bash
cargo run -- --check      # Application ID y rutas, sin conectar con Discord
cargo run -- --selftest   # publica una actividad fija y la mantiene
```

Compruébalo **desde otra cuenta de Discord**: el propio perfil no muestra la
tarjeta completa.

### Empaquetar el mod

```bash
python scripts/package_mod.py
```

Copia el zip resultante de `dist/` a tu carpeta `mods` de Factorio.

### Compilar el instalador

```bash
cargo build --release
ISCC.exe /DAppVersion=0.1.0 installer\factorio-discord-rp.iss
```

`FACTORIO_DRP_DEFAULT_APP_ID`, puesto antes de `cargo build`, incluye un Application
ID por defecto en el binario (lo hace la CI con una variable del repositorio); sin
él, cada usuario necesita su propio `config.toml`.

### Tests y lint

```bash
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

### Abrirla junto con Factorio a mano

Es lo que hace el instalador por ti. Para hacerlo sin él, en Steam → Factorio →
Propiedades → Opciones de lanzamiento:

```
"C:\ruta\a\factorio-discord-rp.exe" %command%
```

La aplicación arranca Factorio, publica mientras siga abierto y se cierra sola
poco después de que lo cierres. Todo lo que sigue a la ruta del juego es del juego,
así que tus otras opciones de lanzamiento siguen funcionando.

- Si ya la tienes en la bandeja con **Arrancar con Windows**, esa copia es la que
  publica y esta sólo lanza el juego: nunca hay dos copias a la vez.
- Si la configuración falla, Factorio arranca igualmente, sin presencia. El motivo
  queda en el registro (`%APPDATA%\factorio-discord-rp\factorio-discord-rp.log`).
- El `config.toml` se busca junto al `.exe` y en `%APPDATA%\factorio-discord-rp\`.

## Licencia

MIT
