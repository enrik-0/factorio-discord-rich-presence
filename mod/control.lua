-- Discord Rich Presence para Factorio 2.1
--
-- El mod sólo produce datos: escribe un JSON en script-output/discord-rp/.
-- La aplicación acompañante lo lee y habla con Discord. El sandbox de Factorio
-- no permite sockets ni HTTP, así que el fichero es el único canal posible.

local collect = require("collect")

local SCHEMA = 2
local OUTPUT_FILE = "discord-rp/state.json"

-- Overhauls reconocidos, en orden de prioridad: si conviven varios gana el primero.
local OVERHAULS = {
  "pypostprocessing",
  "space-exploration",
  "Krastorio2",
  "bobplates",
  "angelsrefining",
  "space-age",
}

local CONTROLLER_NAMES = {}
for name, value in pairs(defines.controllers) do
  CONTROLLER_NAMES[value] = name
end

--------------------------------------------------------------------------------
-- Estado persistente
--------------------------------------------------------------------------------

-- Inicio de la sesión de cada jugador, en `game.ticks_played`.
--
-- No va en `storage`: la sesión es "desde que se cargó la partida", y lo que se
-- guarda sobrevive al cierre y acabaría contando horas de sesiones anteriores.
-- Al ser local, se vacía en cada carga. Sólo sirve para escribir el fichero de
-- estado, así que no influye en la simulación.
local session_start = {}

local function init_storage()
  storage.seq = storage.seq or 0
  storage.tech = storage.tech or {}
  storage.translations = storage.translations or {}
  storage.requested = storage.requested or {}
  storage.pending = storage.pending or {}
  storage.session_start = nil -- heredado de la 0.2.1; ya no se guarda
end

--------------------------------------------------------------------------------
-- Caché del conteo de tecnologías
--------------------------------------------------------------------------------

local function recount(force)
  local done, total = collect.count_technologies(force)
  storage.tech[force.index] = { done = done, total = total }
end

local function recount_all()
  storage.tech = {}
  for _, force in pairs(game.forces) do
    recount(force)
  end
end

--------------------------------------------------------------------------------
-- Traducción de los nombres de tecnología
--------------------------------------------------------------------------------

-- `localised_name` es un LocalisedString: Lua no puede convertirlo a texto por sí
-- mismo. `request_translation` lo resuelve en el idioma del cliente y devuelve el
-- resultado por evento. Así funciona también con tecnologías de cualquier mod.
local function ensure_translation(player, tech)
  local requested = storage.requested[player.index]
  if not requested then
    requested = {}
    storage.requested[player.index] = requested
  end
  if requested[tech.name] then
    return
  end

  local id = player.request_translation(tech.prototype.localised_name)
  if id then
    requested[tech.name] = true
    storage.pending[id] = { player_index = player.index, tech_name = tech.name }
  end
end

script.on_event(defines.events.on_string_translated, function(event)
  local info = storage.pending[event.id]
  if not info then
    return
  end
  storage.pending[event.id] = nil

  if not event.translated then
    -- Sin traducción disponible; se reintentará si el jugador vuelve a entrar.
    local requested = storage.requested[info.player_index]
    if requested then
      requested[info.tech_name] = nil
    end
    return
  end

  local cache = storage.translations[info.player_index]
  if not cache then
    cache = {}
    storage.translations[info.player_index] = cache
  end
  cache[info.tech_name] = event.result
end)

--------------------------------------------------------------------------------
-- Escritura periódica
--------------------------------------------------------------------------------

local function detect_overhaul()
  local active = script.active_mods
  for _, name in ipairs(OVERHAULS) do
    if active[name] then
      return name
    end
  end
  return nil
end

local function count_mods()
  local count = 0
  for _ in pairs(script.active_mods) do
    count = count + 1
  end
  return count
end

-- Preferencias de visualización: qué campos quiere ver este jugador en la
-- tarjeta. La aplicación las obedece; el hueco de cada uno es fijo.
local function build_display(settings)
  return {
    save = settings["drp-show-save"].value,
    planet = settings["drp-show-planet"].value,
    overhaul = settings["drp-show-overhaul"].value,
    research = settings["drp-show-research"].value,
    tech_count = settings["drp-show-tech-count"].value,
    evolution = settings["drp-show-evolution"].value,
    rockets = settings["drp-show-rockets"].value,
    mod_count = settings["drp-show-mod-count"].value,
    mode = settings["drp-show-mode"].value,
    player_name = settings["drp-show-player-name"].value,
    server = settings["drp-show-server"].value,
    timer = settings["drp-timer"].value,
  }
end

local function write_state()
  local players = game.connected_players
  if #players == 0 then
    return
  end

  storage.seq = storage.seq + 1
  local mod_count = count_mods()
  local overhaul = detect_overhaul()

  for _, player in pairs(players) do
    local settings = player.mod_settings
    if settings["drp-enabled"].value then
      local force = player.force

      local current = force.current_research
      if current then
        ensure_translation(player, current)
      end

      -- Al cargar una partida ya empezada no llega `on_player_joined_game`, así
      -- que la sesión arranca la primera vez que el mod ve al jugador.
      if not session_start[player.index] then
        session_start[player.index] = game.ticks_played
      end

      local payload = collect.build_payload(player, {
        schema = SCHEMA,
        seq = storage.seq,
        controller_name = CONTROLLER_NAMES[player.controller_type] or "unknown",
        mod_count = mod_count,
        overhaul = overhaul,
        tech_counts = storage.tech[force.index],
        translations = storage.translations[player.index],
        display = build_display(settings),
        session_start = session_start[player.index],
      })

      -- `for_player` hace que cada cliente escriba sólo su propio fichero:
      -- sin esto, en multijugador todos los peers escribirían lo mismo.
      helpers.write_file(OUTPUT_FILE, helpers.table_to_json(payload), false, player.index)
    end
  end
end

--------------------------------------------------------------------------------
-- Registro del temporizador
--------------------------------------------------------------------------------

local function register_timer()
  script.on_nth_tick(nil)
  local seconds = settings.global["drp-interval-seconds"].value
  script.on_nth_tick(seconds * 60, write_state)
end

--------------------------------------------------------------------------------
-- Ciclo de vida
--------------------------------------------------------------------------------

script.on_init(function()
  init_storage()
  recount_all()
  register_timer()
end)

script.on_load(function()
  register_timer()
end)

script.on_configuration_changed(function()
  init_storage()
  -- Añadir o quitar mods cambia el árbol de tecnologías entero.
  recount_all()
  register_timer()
end)

script.on_event(defines.events.on_runtime_mod_setting_changed, function(event)
  if event.setting == "drp-interval-seconds" then
    register_timer()
  end
end)

script.on_event(defines.events.on_research_finished, function(event)
  recount(event.research.force)
end)

script.on_event(defines.events.on_research_reversed, function(event)
  recount(event.research.force)
end)

script.on_event(defines.events.on_technology_effects_reset, function(event)
  recount(event.force)
end)

script.on_event(defines.events.on_forces_merged, function()
  recount_all()
end)

script.on_event(defines.events.on_force_created, function(event)
  recount(event.force)
end)

script.on_event(defines.events.on_player_removed, function(event)
  storage.translations[event.player_index] = nil
  storage.requested[event.player_index] = nil
  session_start[event.player_index] = nil
end)

-- `player.online_time` NO sirve para el tiempo de sesión: acumula todas las
-- sesiones de ese jugador en la partida. La sesión real es cuánto ha avanzado
-- el reloj de la partida desde que entró.
script.on_event(defines.events.on_player_joined_game, function(event)
  session_start[event.player_index] = game.ticks_played
end)

script.on_event(defines.events.on_player_left_game, function(event)
  session_start[event.player_index] = nil
end)

--------------------------------------------------------------------------------
-- Diagnóstico
--------------------------------------------------------------------------------

-- Sirve para contrastar el conteo contra el árbol de tecnologías del juego, que
-- es la única forma de confirmar que el criterio de tecnologías infinitas acierta.
commands.add_command("drp-debug", { "drp.debug-help" }, function(event)
  local player = game.get_player(event.player_index)
  if not player then
    return
  end
  local counts = storage.tech[player.force.index] or { done = 0, total = 0 }
  local surface = player.physical_surface
  player.print(string.format(
    "[Discord RP] tecnologías %d/%d | superficie %s | planeta %s | plataforma %s | ticks_played %d | seq %d",
    counts.done,
    counts.total,
    surface.name,
    surface.planet and surface.planet.name or "-",
    surface.platform and "sí" or "no",
    game.ticks_played,
    storage.seq or 0
  ))
  player.print("[Discord RP] fichero: script-output/" .. OUTPUT_FILE)
end)
