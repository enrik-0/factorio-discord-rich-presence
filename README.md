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

## Estado del proyecto

| Fase | Qué | Estado |
|---|---|---|
| 0 | Andamiaje, CI, empaquetado | hecho |
| 1 | Canal IPC con Discord (`--selftest`) | **validado** contra Discord real |
| 2 | El mod escribe `state.json` | código listo, **falta probarlo dentro del juego** |
| 3 | Fusión de fuentes + plantillas | **validado** (45 tests) |
| 4 | Parseo del log, save y detección de proceso | **validado** contra un log real de 2.1.17 |
| 5 | Bandeja, autoarranque | pendiente |
| 6 | Publicación | pendiente |

Lo único sin verificar de extremo a extremo es el mod dentro de Factorio. La aplicación
ya funciona en modo degradado: detecta el juego, saca el nombre del save del log y lo
publica en Discord.

Pendiente de configuración, no de código:

- **Subir el logo de Factorio** al portal de Discord con la clave `factorio`. Sin él
  aparece un icono de marcador de posición.
- **Nombrar la aplicación `Factorio`** en el portal, si quieres que la tarjeta se lea como
  soporte nativo del juego en lugar de mostrar el nombre de la aplicación.

## Requisitos

- Factorio **2.1** (el mod declara `factorio_version: "2.1"`; Factorio no tiene
  compatibilidad hacia delante, así que no carga en 2.0)
- Rust estable, para compilar la aplicación
- Python 3, sólo para el script de empaquetado

## Puesta en marcha

### 1. Crear la aplicación de Discord

Este paso es manual y sólo se hace una vez:

1. Entra en <https://discord.com/developers/applications> y pulsa **New Application**.
2. Llámala exactamente `Factorio` — ese nombre es lo que Discord muestra como
   *"Jugando a ..."*.
3. Copia el **Application ID** de *General Information*.
4. En *Rich Presence → Art Assets* sube **una sola imagen**, el logo de Factorio,
   con la clave `factorio`. No hace falta un icono por planeta: el planeta se lee
   en el texto de la tarjeta.

### 2. Configurar la aplicación

```bash
cp app/config.example.toml config.toml
```

Pon el Application ID en `config.toml`. Para una prueba rápida sirve la variable
de entorno `FACTORIO_DRP_APP_ID`.

### 3. Comprobar que todo está en su sitio

```bash
cargo run -- --check
```

Verifica el Application ID y localiza la carpeta de datos de Factorio.

### 4. Validar la conexión con Discord

```bash
cargo run -- --selftest
```

Publica una actividad fija y la mantiene. Compruébalo **desde otra cuenta de Discord**:
el propio perfil no muestra la tarjeta completa.

### 5. Instalar el mod

```bash
python scripts/package_mod.py
```

Copia el zip resultante de `dist/` a tu carpeta `mods` de Factorio.

Dentro del juego, `/drp-debug` imprime los datos que está publicando el mod, para
contrastarlos con el árbol de tecnologías.

### 6. Abrirla junto con Factorio (opcional)

En Steam, clic derecho en Factorio → **Propiedades → Opciones de lanzamiento**, y
pon la ruta de la aplicación seguida de `%command%`:

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
- Para volver al arranque normal, borra la línea de las opciones de lanzamiento.

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

```bash
cargo test           # tests de la aplicación
cargo clippy --all-targets -- -D warnings
python scripts/package_mod.py
```

## Licencia

MIT
