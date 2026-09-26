-- Globales que aporta el entorno de scripting de Factorio.
std = "lua54"
max_line_length = 120

read_globals = {
  "game", "script", "storage", "settings", "commands", "helpers",
  "defines", "data", "serpent", "prototypes", "rendering",
}

exclude_files = { "app/**", "target/**" }
