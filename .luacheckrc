-- Globales que aporta el entorno de scripting de Factorio.
std = "lua54"
max_line_length = 120

read_globals = {
  "game", "script", "settings", "commands", "helpers",
  "defines", "data", "serpent", "prototypes", "rendering",
}

-- `storage` es la tabla de estado persistente del mod: se escribe en control.lua.
globals = { "storage" }

exclude_files = { "app/**", "target/**" }
