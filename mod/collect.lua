-- Collects the state that gets published to Discord.
--
-- Anything expensive (walking `force.technologies`) is cached in `storage` and
-- only recomputed on events; nothing of variable cost is iterated here.
--
-- All available information is always sent: the file never leaves the
-- machine. What the player chooses is what gets *shown*, and that travels in
-- the `display` block for the app to obey.

local collect = {}

-- Infinite technologies (mining and rocket-launch productivity) declare
-- `max_level` as the max of a uint32. They never move to `researched`, they
-- only level up, so counting them would fake the "41/1510".
--
-- Filtering by `prototype.upgrade` doesn't work: in vanilla, finite techs like
-- physical-projectile-damage-3 are also of type upgrade.
local INFINITE_MAX_LEVEL = 4294967295

--- Counts a force's finished and total finite technologies.
--- Expensive: walks `force.technologies`, a LuaCustomTable (each access
--- crosses the Lua/C++ boundary). Only call from the events that justify it.
--- @return number done, number total
function collect.count_technologies(force)
  local done, total = 0, 0
  for _, tech in pairs(force.technologies) do
    if tech.prototype.max_level < INFINITE_MAX_LEVEL then
      -- A tech disabled by a mod but already researched still counts: the
      -- player did research it.
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

--- Classifies a surface so the app knows what it is.
--- @return string kind, string|nil planet
local function describe_surface(surface)
  if surface.platform then
    return "platform", nil
  end
  local planet = surface.planet
  if planet then
    return "planet", planet.name
  end
  -- Surfaces from mods (Factorissimo, Space Exploration, factories...).
  return "other", nil
end

--- Evolution factor of the surface the player is on.
--- A single call, no iteration. Returns 0 on surfaces without enemies.
local function evolution_of(force, surface)
  local ok, value = pcall(force.get_evolution_factor, surface)
  if ok and type(value) == "number" then
    return value
  end
  return nil
end

--------------------------------------------------------------------------------
-- "Meme" stats: trees razed, enemies killed, deaths and pollution.
-- `get_kill_count_statistics` is per force and surface, so every surface has
-- to be summed (Space Age has several).
--------------------------------------------------------------------------------

-- Prototype types that count as an enemy. `input_counts` already only holds
-- what the force has killed (never its own), so nothing of the player's needs
-- excluding: this just separates enemies from trees and other neutral
-- casualties (rocks, fish...) that don't matter for this counter.
local ENEMY_TYPES = {
  ["unit"] = true,
  ["unit-spawner"] = true,
  ["turret"] = true,
  ["ammo-turret"] = true,
  ["electric-turret"] = true,
  ["fluid-turret"] = true,
  ["spider-unit"] = true,
  ["segment"] = true, -- demolisher segments (Gleba, Space Age)
  ["segmented-unit"] = true, -- the demolisher itself
}

--- A force's kills and deaths, summed across every surface.
--- Expensive: walks each surface's `input_counts`/`output_counts`, which are
--- LuaCustomTable. Call at most once per force and write (see
--- `combat_stats_of` in control.lua, which caches the result within the tick).
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
      -- The player's own character is always named "character"; with other
      -- body mods the player might die under a different name and not count.
      deaths = deaths + (stats.output_counts["character"] or 0)
    end
  end

  return enemies, trees, deaths
end

--- Total pollution emitted, summed across every surface in the game.
---
--- Not per force: `LuaFlowStatistics.force` is `nil` for pollution
--- statistics, so the API doesn't allow isolating just yours. In multiplayer
--- with more than one force, this number includes everyone's pollution, not
--- just your own. Still good enough for this field's "meme" purpose.
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

--------------------------------------------------------------------------------
-- Science per minute: read from the same production statistics the game's
-- Production panel (P) shows, so the number matches what the player sees.
--------------------------------------------------------------------------------

-- Every item any lab accepts, from vanilla or from mods. Prototypes can't
-- change without a reload, which resets this local, so it never goes stale.
local science_packs

local function get_science_packs()
  if science_packs then
    return science_packs
  end
  local packs, seen = {}, {}
  for _, lab in pairs(prototypes.get_entity_filtered({ { filter = "type", type = "lab" } })) do
    for _, name in pairs(lab.lab_inputs or {}) do
      if not seen[name] then
        seen[name] = true
        packs[#packs + 1] = name
      end
    end
  end
  science_packs = packs
  return packs
end

-- Qualities with how many research units one pack of that quality is worth.
-- A higher-quality pack lasts several units (uncommon 2, rare 3, ...), so
-- counting packs alone would undercount a base that uses them.
local qualities

local function get_qualities()
  if qualities then
    return qualities
  end
  qualities = {}
  for name, quality in pairs(prototypes.quality) do
    qualities[#qualities + 1] = { name = name, units = quality.science_capacity_multiplier }
  end
  return qualities
end

--- Research units per minute: normal-quality packs the force consumed during
--- the last minute, counting each higher-quality pack as the units it holds.
---
--- Each research unit takes one of every ingredient at once, so every pack in
--- use drains at the same pace: the highest per-pack rate is the SPM, and
--- summing them would multiply it by the number of pack types.
--- Walks every surface (Space Age has labs on several planets).
--- @return number
function collect.science_per_minute(force)
  local per_pack = {}
  for _, surface in pairs(game.surfaces) do
    local ok, stats = pcall(force.get_item_production_statistics, surface)
    if ok and stats then
      for _, name in ipairs(get_science_packs()) do
        for _, quality in ipairs(get_qualities()) do
          -- "output" is consumption; `count = true` gives the total over the
          -- window, which for the one-minute window is the per-minute rate.
          -- A bare name only reads normal quality, so ask for each quality.
          local consumed = stats.get_flow_count({
            name = { name = name, quality = quality.name },
            category = "output",
            precision_index = defines.flow_precision_index.one_minute,
            count = true,
          })
          per_pack[name] = (per_pack[name] or 0) + consumed * quality.units
        end
      end
    end
  end

  local spm = 0
  for _, consumed in pairs(per_pack) do
    if consumed > spm then
      spm = consumed
    end
  end
  return spm
end

--- Builds the table that gets serialized to JSON for a player.
function collect.build_payload(player, opts)
  local force = player.force
  local surface = player.physical_surface
  local kind, planet = describe_surface(surface)
  local counts = opts.tech_counts or { done = 0, total = 0 }
  local current = force.current_research

  local research = {
    done = counts.done,
    total = counts.total,
    spm = opts.spm,
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
    -- Ticks since the player's last action. A "live" value: not cached like
    -- the technologies, read as-is on every write.
    afk_ticks = player.afk_time,
  }
end

return collect
