-- El periodo de escritura es global: `on_nth_tick` es único para toda la partida,
-- así que no puede variar por jugador.
--
-- Todo lo demás es por jugador y decide **qué se ve en la tarjeta de Discord**.
-- El fichero de estado nunca sale del equipo, así que elegir qué se muestra es
-- también el control de privacidad: lo único que ve otra gente es la tarjeta.
--
-- El hueco de cada campo es fijo y se indica en su descripción, para que ninguna
-- casilla pueda activarse sin que aparezca nada.

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

  -- Línea 1: identidad de la partida.
  toggle("drp-show-save", "ba", true),
  toggle("drp-show-planet", "bb", true),
  toggle("drp-show-overhaul", "bc", false),

  -- Línea 2: qué estás haciendo.
  toggle("drp-show-research", "ca", true),

  -- Tooltip: contadores.
  toggle("drp-show-tech-count", "da", true),
  toggle("drp-show-evolution", "db", false),
  toggle("drp-show-rockets", "dc", true),
  toggle("drp-show-mod-count", "dd", false),
  toggle("drp-show-mode", "de", true),
  toggle("drp-show-player-name", "df", false),
  toggle("drp-show-server", "dg", false),

  {
    type = "string-setting",
    name = "drp-timer",
    setting_type = "runtime-per-user",
    default_value = "save",
    allowed_values = { "save", "session", "none" },
    order = "ea",
  },
})
