-- Discord Rich Presence for Factorio 2.0+
--
-- The mod only produces data: it writes a JSON file under script-output/discord-rp/.
-- The companion app reads it and talks to Discord. Factorio's sandbox doesn't
-- allow sockets or HTTP, so the file is the only channel available.

local collect = require("collect")

local SCHEMA = 2
local OUTPUT_FILE = "discord-rp/state.json"

-- Recognized overhauls, in priority order: if several are active, the first one wins.
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
-- Persistent state
--------------------------------------------------------------------------------

-- Each player's session start, in `game.ticks_played`.
--
-- Not stored in `storage`: the session is "since the save was loaded", and
-- what's saved survives a close and would end up counting hours from previous
-- sessions. Being local, it clears on every load. Only used to write the
-- state file, so it doesn't affect the simulation.
local session_start = {}

local function init_storage()
  storage.seq = storage.seq or 0
  storage.tech = storage.tech or {}
  storage.translations = storage.translations or {}
  storage.requested = storage.requested or {}
  storage.pending = storage.pending or {}
  storage.session_start = nil -- inherited from 0.2.1; no longer stored
end

--------------------------------------------------------------------------------
-- Technology count cache
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
-- Technology name translation
--------------------------------------------------------------------------------

-- `localised_name` is a LocalisedString: Lua can't turn it into text on its
-- own. `request_translation` resolves it in the client's language and
-- returns the result via an event. This is also how it works for
-- technologies from any mod.
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
    -- No translation available; it'll be retried if the player rejoins.
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
-- Periodic write
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

-- Display preferences: which fields this player wants to see on the card.
-- The app obeys them; each field's slot is fixed.
local function build_display(settings)
  return {
    save = settings["drp-show-save"].value,
    planet = settings["drp-show-planet"].value,
    overhaul = settings["drp-show-overhaul"].value,
    research = settings["drp-show-research"].value,
    tech_count = settings["drp-show-tech-count"].value,
    evolution = settings["drp-show-evolution"].value,
    rockets = settings["drp-show-rockets"].value,
    trees = settings["drp-show-trees"].value,
    enemies = settings["drp-show-enemies"].value,
    deaths = settings["drp-show-deaths"].value,
    pollution = settings["drp-show-pollution"].value,
    afk = settings["drp-show-afk"].value,
    mod_count = settings["drp-show-mod-count"].value,
    mode = settings["drp-show-mode"].value,
    player_name = settings["drp-show-player-name"].value,
    server = settings["drp-show-server"].value,
    timer = settings["drp-timer"].value,
  }
end

-- A force's kills and deaths, computed at most once per write: if several
-- players share a force, the second lookup reuses the first instead of
-- repeating the walk over every surface.
local function combat_stats_of(force, cache)
  local cached = cache[force.index]
  if cached then
    return cached
  end
  local enemies, trees, deaths = collect.combat_stats(force)
  local stats = { enemies = enemies, trees = trees, deaths = deaths }
  cache[force.index] = stats
  return stats
end

-- Same per-write caching as `combat_stats_of`.
local function spm_of(force, cache)
  local cached = cache[force.index]
  if not cached then
    cached = collect.science_per_minute(force)
    cache[force.index] = cached
  end
  return cached
end

local function write_state()
  local players = game.connected_players
  if #players == 0 then
    return
  end

  storage.seq = storage.seq + 1
  local mod_count = count_mods()
  local overhaul = detect_overhaul()
  -- Not per force (see collect.total_pollution): computed just once.
  local pollution = collect.total_pollution()
  local combat_cache = {}
  local spm_cache = {}

  for _, player in pairs(players) do
    local settings = player.mod_settings
    if settings["drp-enabled"].value then
      local force = player.force

      local current = force.current_research
      if current then
        ensure_translation(player, current)
      end

      -- `on_player_joined_game` doesn't fire when loading an already-started
      -- save, so the session starts the first time the mod sees the player.
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
        combat = combat_stats_of(force, combat_cache),
        pollution = pollution,
        spm = spm_of(force, spm_cache),
      })

      -- `for_player` makes each client write only its own file: without
      -- this, every peer in multiplayer would write the same thing.
      helpers.write_file(OUTPUT_FILE, helpers.table_to_json(payload), false, player.index)
    end
  end
end

--------------------------------------------------------------------------------
-- Timer registration
--------------------------------------------------------------------------------

local function register_timer()
  script.on_nth_tick(nil)
  local seconds = settings.global["drp-interval-seconds"].value
  script.on_nth_tick(seconds * 60, write_state)
end

--------------------------------------------------------------------------------
-- Lifecycle
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
  -- Adding or removing mods changes the whole technology tree.
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

-- `player.online_time` does NOT work for session time: it accumulates every
-- one of that player's sessions in the save. The real session is how much
-- the game clock has advanced since they joined.
script.on_event(defines.events.on_player_joined_game, function(event)
  session_start[event.player_index] = game.ticks_played
end)

script.on_event(defines.events.on_player_left_game, function(event)
  session_start[event.player_index] = nil
end)

--------------------------------------------------------------------------------
-- Diagnostics
--------------------------------------------------------------------------------

-- Useful to cross-check the count against the game's own technology tree,
-- the only way to confirm the infinite-technology filter is correct.
commands.add_command("drp-debug", { "drp.debug-help" }, function(event)
  local player = game.get_player(event.player_index)
  if not player then
    return
  end
  local counts = storage.tech[player.force.index] or { done = 0, total = 0 }
  local surface = player.physical_surface
  player.print(string.format(
    "[Discord RP] technologies %d/%d | surface %s | planet %s | platform %s | ticks_played %d | seq %d",
    counts.done,
    counts.total,
    surface.name,
    surface.planet and surface.planet.name or "-",
    surface.platform and "yes" or "no",
    game.ticks_played,
    storage.seq or 0
  ))
  player.print("[Discord RP] file: script-output/" .. OUTPUT_FILE)

  -- Uncached, recomputed on the spot: useful to cross-check against the game
  -- itself (F4 > kill_count_statistics, or the "Production" panel > Losses).
  local enemies, trees, deaths = collect.combat_stats(player.force)
  player.print(string.format(
    "[Discord RP] enemies %d | trees %d | deaths %d | pollution %.0f | afk %d ticks",
    enemies,
    trees,
    deaths,
    collect.total_pollution(),
    player.afk_time
  ))
  -- Cross-check against the Production panel (P), 1m window, science packs' consumption
  -- (normal-quality packs match it exactly; higher qualities are weighted by their units).
  player.print(string.format(
    "[Discord RP] SPM %.1f",
    collect.science_per_minute(player.force)
  ))
end)
