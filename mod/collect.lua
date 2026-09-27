-- Recolección del estado que se publica en Discord.
--
-- Todo lo caro (recorrer `force.technologies`) vive en caché en `storage` y se
-- recalcula sólo por eventos; aquí no se itera nada de coste variable.
--
-- Se envía siempre toda la información disponible: el fichero no sale del
-- equipo. Lo que el jugador elige es qué se *muestra*, y eso viaja en el bloque
-- `display` para que la aplicación lo obedezca.

local collect = {}

-- Las tecnologías infinitas (productividad de minería y compañía) declaran
-- `max_level` como el máximo de un uint32. Nunca pasan a `researched`, sólo
-- suben de nivel, así que contarlas falsearía el "41/1510".
--
-- No sirve filtrar por `prototype.upgrade`: en vanilla, tecnologías finitas como
-- physical-projectile-damage-3 también son de tipo upgrade.
local INFINITE_MAX_LEVEL = 4294967295

--- Cuenta tecnologías finitas investigadas y totales de una fuerza.
--- Caro: recorre `force.technologies`, que es un LuaCustomTable (cada acceso
--- cruza la frontera Lua/C++). Llamar sólo desde los eventos que lo justifican.
--- @return number done, number total
function collect.count_technologies(force)
  local done, total = 0, 0
  for _, tech in pairs(force.technologies) do
    if tech.prototype.max_level < INFINITE_MAX_LEVEL then
      -- Una tecnología deshabilitada por un mod pero ya investigada sigue
      -- contando: el jugador la investigó.
      if tech.enabled or tech.researched then
        total = total + 1
        if tech.researched then
          done = done + 1
        end
      end
    end
  end
  return done, total
end

--- Clasifica una superficie para que la aplicación sepa qué es.
--- @return string kind, string|nil planet
local function describe_surface(surface)
  if surface.platform then
    return "platform", nil
  end
  local planet = surface.planet
  if planet then
    return "planet", planet.name
  end
  -- Superficies de mods (Factorissimo, Space Exploration, fábricas...).
  return "other", nil
end

--- Factor de evolución de la superficie donde está el jugador.
--- Una sola llamada, sin iterar. En superficies sin enemigos devuelve 0.
local function evolution_of(force, surface)
  local ok, value = pcall(force.get_evolution_factor, surface)
  if ok and type(value) == "number" then
    return value
  end
  return nil
end

--------------------------------------------------------------------------------
-- Estadísticas "meme": árboles arrasados, enemigos abatidos, muertes y
-- contaminación. `get_kill_count_statistics` es por fuerza y superficie, así
-- que hay que sumar todas las superficies (Space Age tiene varias).
--------------------------------------------------------------------------------

-- Tipos de prototipo que cuentan como enemigo. `input_counts` ya sólo trae lo
-- que la fuerza ha matado (no lo propio), así que no hace falta excluir nada
-- del jugador: sólo hay que separar enemigos de árboles y de otras bajas
-- neutrales (rocas, peces...) que no interesan para este contador.
local ENEMY_TYPES = {
  ["unit"] = true,
  ["unit-spawner"] = true,
  ["turret"] = true,
  ["ammo-turret"] = true,
  ["electric-turret"] = true,
  ["fluid-turret"] = true,
  ["spider-unit"] = true,
  ["segment"] = true, -- segmentos del demolisher (Gleba, Space Age)
  ["segmented-unit"] = true, -- el demolisher en sí
}

--- Bajas y muertes de una fuerza, sumadas en todas las superficies.
--- Caro: recorre `input_counts`/`output_counts` de cada superficie, que son
--- LuaCustomTable. Llamar como mucho una vez por fuerza y escritura (ver
--- `combat_stats_of` en control.lua, que cachea el resultado dentro del tick).
--- @return number enemies, number trees, number deaths
function collect.combat_stats(force)
  local enemies, trees, deaths = 0, 0, 0

  for _, surface in pairs(game.surfaces) do
    local ok, stats = pcall(force.get_kill_count_statistics, surface)
    if ok and stats then
      for name, count in pairs(stats.input_counts) do
        local proto = prototypes.entity[name]
        if proto then
          if proto.type == "tree" then
            trees = trees + count
          elseif ENEMY_TYPES[proto.type] then
            enemies = enemies + count
          end
        end
      end
      -- El propio personaje siempre se llama "character"; con otros mods de
      -- cuerpo el jugador podría morir con otro nombre y no contaría aquí.
      deaths = deaths + (stats.output_counts["character"] or 0)
    end
  end

  return enemies, trees, deaths
end

--- Contaminación total emitida, sumada en todas las superficies del juego.
---
--- No es por fuerza: `LuaFlowStatistics.force` es `nil` para las estadísticas
--- de contaminación, así que la API no permite aislar sólo la tuya. En
--- multijugador con más de una fuerza, este número incluye la contaminación
--- de todo el mundo, no sólo la propia. Aun así vale para el propósito "meme"
--- de este campo.
--- @return number
function collect.total_pollution()
  local total = 0
  for _, surface in pairs(game.surfaces) do
    local ok, stats = pcall(function()
      return surface.pollution_statistics
    end)
    if ok and stats then
      for _, count in pairs(stats.input_counts) do
        total = total + count
      end
    end
  end
  return total
end

--- Construye la tabla que se serializa a JSON para un jugador.
function collect.build_payload(player, opts)
  local force = player.force
  local surface = player.physical_surface
  local kind, planet = describe_surface(surface)
  local counts = opts.tech_counts or { done = 0, total = 0 }
  local current = force.current_research

  local research = {
    done = counts.done,
    total = counts.total,
  }
  if current then
    research.current = current.name
    research.current_label = opts.translations and opts.translations[current.name]
    research.progress = force.research_progress
  end

  return {
    schema = opts.schema,
    seq = opts.seq,
    display = opts.display,
    player = {
      name = player.name,
      index = player.index,
      controller = opts.controller_name,
    },
    surface = {
      name = surface.name,
      kind = kind,
      planet = planet,
    },
    research = research,
    game = {
      multiplayer = game.is_multiplayer(),
      players_online = #game.connected_players,
      ticks_played = game.ticks_played,
      session_ticks = game.ticks_played - (opts.session_start or game.ticks_played),
      speed = game.speed,
    },
    mods = {
      count = opts.mod_count,
      overhaul = opts.overhaul,
    },
    rockets_launched = force.rockets_launched,
    evolution = evolution_of(force, surface),
    enemies_killed = opts.combat and opts.combat.enemies,
    trees_razed = opts.combat and opts.combat.trees,
    player_deaths = opts.combat and opts.combat.deaths,
    pollution_emitted = opts.pollution,
    -- Ticks desde la última acción del jugador. Es un valor "en vivo": no se
    -- cachea como las tecnologías, se lee tal cual en cada escritura.
    afk_ticks = player.afk_time,
  }
end

return collect
