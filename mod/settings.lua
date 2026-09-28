-- The write period is global: `on_nth_tick` is a single timer for the whole
-- save, so it can't vary per player.
--
-- Everything else is per player and decides **what shows up on the Discord
-- card**. The state file never leaves the machine, so choosing what's shown
-- is also the privacy control: the card is the only thing anyone else sees.
--
-- Each field's slot is fixed and noted in its description, so no toggle can
-- be turned on without anything actually appearing.

local function toggle(name, order, default)
  return {
    type = "bool-setting",
    name = name,
    setting_type = "runtime-per-user",
    default_value = default,
    order = order,
  }
end

data:extend({
  {
    type = "int-setting",
    name = "drp-interval-seconds",
    setting_type = "runtime-global",
    default_value = 5,
    minimum_value = 3,
    maximum_value = 30,
    order = "aa",
  },
  toggle("drp-enabled", "ab", true),

  -- Line 1: game identity.
  toggle("drp-show-save", "ba", true),
  toggle("drp-show-planet", "bb", true),
  toggle("drp-show-overhaul", "bc", false),

  -- Line 2: what you're doing.
  toggle("drp-show-research", "ca", true),

  -- Tooltip: counters.
  toggle("drp-show-tech-count", "da", true),
  toggle("drp-show-evolution", "db", false),
  toggle("drp-show-rockets", "dc", true),
  toggle("drp-show-trees", "dd", false),
  toggle("drp-show-enemies", "de", false),
  toggle("drp-show-deaths", "df", false),
  toggle("drp-show-pollution", "dg", false),
  toggle("drp-show-afk", "dh", false),
  toggle("drp-show-mod-count", "di", false),
  toggle("drp-show-mode", "dj", true),
  toggle("drp-show-player-name", "dk", false),
  toggle("drp-show-server", "dl", false),

  {
    type = "string-setting",
    name = "drp-timer",
    setting_type = "runtime-per-user",
    default_value = "save",
    allowed_values = { "save", "session", "none" },
    order = "ea",
  },
})
