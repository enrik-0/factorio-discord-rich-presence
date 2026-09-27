# Discord Rich Presence

Show what you're doing in Factorio on your Discord profile: save name, planet, the
research you're chasing, how many technologies you've finished, and how long
you've been playing — live, while you play.

## What shows up

- **Line 1** — save name, planet (or "Space platform"), and, if you turn it on,
  the overhaul the mod detected.
- **Line 2** — the technology you're researching, with its progress, translated
  into your own language automatically. Falls back to your technology counter
  when nothing is queued.
- **On hover** — technologies done out of the total (infinite research like
  mining productivity is excluded, so the count stays meaningful), evolution
  factor, rockets launched, how many mods you have enabled, singleplayer or
  multiplayer (and how many players are online), your player name, and — off
  by default — your server address.
- **Timer** — total playtime of the save, just this session, or none.

Detects popular overhauls when active: Krastorio2, Space Age, pyanodons, bob's
& angel's, Space Exploration.

## You choose what's shown

Every field above is a per-player setting, under *Settings → Mods*. Nothing you
don't enable ever leaves your computer: the mod only writes a local file, and
only the fields you pick reach your Discord card. Multiplayer? Each player
decides their own.

## Requires the companion app

This mod only collects the data — Factorio's sandbox has no sockets or HTTP, so
it can't talk to Discord by itself. Showing it on your profile needs the free,
open-source companion app for Windows, which reads the file this mod writes and
handles the Discord side for you. It installs in a couple of clicks and can
launch automatically with Factorio through Steam.

👉 **[github.com/enrik-0/factorio-discord-rich-presence](https://github.com/enrik-0/factorio-discord-rich-presence)**

You'll find the installer, the full source of both the mod and the app, and a
detailed README there.

## Requirements

- Factorio **2.1**
- Space Age, if you have it — detected automatically, not required

## One thing to know

Like any mod, this one disables Steam achievements while it's active. That's a
Factorio limitation that applies to every mod, not something specific to this
one. If achievements matter to you, you can use the companion app on its own,
without installing this mod: you'll still get the save name and playtime, just
not the planet or research.
