**English** | [Español](README.es.md)

# Factorio Discord Rich Presence

Shows what you're playing in Factorio on your Discord profile: the save, the planet,
the research in progress, how many technologies you have, and how long you've played.

## Why two pieces

Factorio's Lua sandbox has no sockets, no HTTP, and no filesystem access outside
`script-output`, and the developers have said it never will, because it would break
determinism in multiplayer. So:

- **The mod** collects the state and writes it to `script-output/discord-rp/state.json`.
- **The app** reads that file and talks to Discord's IPC.

## Requirements

- Windows or Linux, and Factorio **2.1** (the mod declares `factorio_version: "2.1"`;
  Factorio has no forward compatibility, so it won't load on 2.0)
- On Windows, if you play Factorio through Steam, nothing else is needed: the installer
  detects it on its own. Linux support is newer and more manual — see
  [Installation (Linux)](#installation-linux) below.

## Installation (Windows)

1. Download the installer (`FactorioDiscordRP-Setup-*.exe`) from
   [Releases](https://github.com/enrik-0/factorio-discord-rich-presence/releases) and
   run it. It's an unsigned installer, so Windows may warn with
   *"Windows protected your PC"*: click **More info → Run anyway**.
2. Choose how to set up Steam:
   - **Automatic** (recommended): the installer sets Factorio's launch options for you.
     It opens when you launch the game and closes with it.
   - **Manual**: the installer shows you the full line to paste yourself into
     Steam → Factorio → Properties → Launch Options.
3. Install the «Discord Rich Presence» mod from the
   [Mod Portal](https://mods.factorio.com/mod/discord-rich-presence) or from the game
   itself, under *Mods*. Without it, the app still works in degraded mode: it publishes
   the save name and playtime, but not the planet or research.
4. Play. Check the card **from another Discord account**: your own profile doesn't show
   it in full.

To uninstall, use *Add or remove programs*: it also removes whatever it put in Steam's
launch options, without touching the rest of your options.

In-game, `/drp-debug` prints the data the mod is publishing, to check it against the
technology tree.

## Installation (Linux)

Linux support is newer and more manual than Windows: there's no installer, no automatic
Steam configuration, and no system tray icon yet — the app just runs quietly in the
background and logs to a file.

1. Download `factorio-discord-rp-linux-x86_64-*.tar.gz` from
   [Releases](https://github.com/enrik-0/factorio-discord-rich-presence/releases) and
   extract it somewhere permanent. Make sure the binary is executable
   (`chmod +x factorio-discord-rp`).
2. In Steam → Factorio → Properties → Launch Options, add:
   ```
   "/path/to/factorio-discord-rp" %command%
   ```
   (with the real path to wherever you extracted it). The app launches Factorio,
   publishes while it stays open, and closes itself shortly after you close it.
3. Install the «Discord Rich Presence» mod, same as above — from the
   [Mod Portal](https://mods.factorio.com/mod/discord-rich-presence) or from the game
   itself.
4. Play. Check the card **from another Discord account**.

No Application ID setup is needed for normal use, same as on Windows. To stop using it,
just remove that line from the launch options and delete the binary — there's nothing
else installed anywhere.

`factorio-discord-rp --check` from a terminal reports what it can find (paths,
Application ID) without touching Discord; logs go to
`~/.local/share/factorio-discord-rp/factorio-discord-rp.log`.

## Mod settings

**You choose what's shown, from inside the game.** The state file never leaves your
computer, so choosing what to show is also the privacy control: the only thing anyone
else sees is the Discord card.

Every field has a fixed slot, stated in its setting's description, so no checkbox can be
turned on without anything showing up.

| Setting | Slot | Default |
|---|---|---|
| Save name | line 1 | yes |
| Planet | line 1 | yes |
| Main modpack | line 1 | no |
| Current research | line 2 | yes |
| Technology counter | on hover | yes |
| Evolution factor | on hover | no |
| Rockets launched | on hover | yes, hidden while zero |
| Mod count | on hover | no |
| Singleplayer / multiplayer | on hover | yes |
| My player name | on hover | no |
| Server address | on hover | **no** |
| Timer | — | save playtime |

Settings are **per player**, so in multiplayer everyone decides their own. The write
interval is global, because the timer is unique to the whole save.

If a slot goes over Discord's 128 characters, lower-priority fields get dropped instead
of cutting off mid-word.

### Advanced mode: templates

If you'd rather decide the layout yourself, define a `[templates]` section in
`config.toml`. Then the templates take over and the mod's checkboxes are ignored — the
two can't both be in charge. See `app/config.example.toml`.

## Privacy

The server address has a **double lock**: it needs to be turned on both in the mod's
settings *and* in `config.toml`. It's the only field that exposes anything from outside
the game, so the app keeps a veto over the mod.

Everything else is controlled from the mod's settings, in-game.

## Development

To build and run from source instead of using the installer:

- Rust stable, to build the app.
- Python 3, only for the mod packaging script.
- Inno Setup 6, only to build the installer (`installer/factorio-discord-rp.iss`).

### Create the Discord application

Only needed once, if you're not using the installer (which already includes one):

1. Go to <https://discord.com/developers/applications> and click **New Application**.
2. Name it exactly `Factorio` — that name is what Discord shows as *"Playing ..."*.
3. Copy the **Application ID** from *General Information*.
4. Under *Rich Presence → Art Assets*, upload **a single image**, the Factorio logo,
   with the key `factorio`. No per-planet icon is needed: the planet is read from the
   card's text.

### Configure and run

```bash
cp app/config.example.toml config.toml
```

Put the Application ID in `config.toml`. For a quick test, the `FACTORIO_DRP_APP_ID`
environment variable also works.

```bash
cargo run -- --check      # Application ID and paths, without connecting to Discord
cargo run -- --selftest   # publishes a fixed activity and keeps it up
```

Check it **from another Discord account**: your own profile doesn't show the full card.

### Package the mod

```bash
python scripts/package_mod.py
```

Copy the resulting zip from `dist/` to your Factorio `mods` folder.

### Build the installer

```bash
cargo build --release
ISCC.exe /DAppVersion=0.5.2 installer\factorio-discord-rp.iss
```

`FACTORIO_DRP_DEFAULT_APP_ID`, set before `cargo build`, bakes a default Application ID
into the binary (the CI does this with a repository variable); without it, every user
needs their own `config.toml`.

### Tests and lint

```bash
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

### Launching it alongside Factorio by hand

This is what the installer does for you. To do it without the installer, in
Steam → Factorio → Properties → Launch Options:

```
"C:\path\to\factorio-discord-rp.exe" %command%
```

The app launches Factorio, publishes while it stays open, and closes itself shortly
after you close it. Everything after the game's path belongs to the game, so your other
launch options keep working.

- If you already have it in the tray via **Start with Windows**, that copy is the one
  publishing, and this one just launches the game: there are never two copies at once.
- If the setup fails, Factorio still starts, without presence. The reason is in the log
  (`%APPDATA%\factorio-discord-rp\factorio-discord-rp.log`).
- `config.toml` is looked up next to the `.exe` and in
  `%APPDATA%\factorio-discord-rp\`.

## License

[MIT](LICENSE)
