CUSTOM THEMES FOR CYBER GIVEAWAY
================================

This folder holds custom themes for the OBS overlay and the timer display.
Every subfolder is one theme. The folder name is its ID
(only a–z, 0–9, - and _; folders starting with _ are ignored).

How to:
  1. Copy the "_vorlage" folder and rename it, e.g. "mygame".
  2. Change the name and texts in mygame/theme.json
     (and mygame/theme.en.json for English).
  3. Change colors and font in mygame/theme.css.
  4. In the program (Settings → Theme) or in the dock click
     "Reload themes" and pick the theme.

theme.json / theme.en.json
--------------------------
theme.json holds the German texts, theme.en.json the English ones (same
layout). With the language set to English the program reads both; anything
missing in theme.en.json is taken from theme.json.

  "name"        name shown in the list
  "ign_game"    which game name is shown for the winner: "eve" (default)
                or an id from Settings → Chat & games (e.g. "wow")
  "game"        what happens in the overlay during the draw:
                  dice      Matrix run with dice (Shadowrun, default)
                  datasite  hacking a data site like in EVE Online
                  treasure  treasure hunt with map and digging (Palia style)
                  riddle    riddles, rune lock, braziers, loot roll (WoW style)
                  battle    boss fight with special attacks (anime)
  "description" short description
  "font_url"    Google Fonts link (also set the font in theme.css under --font)
  "shape"       node shape: hex | ring | diamond | circle | star
  "ui"          labels in the overlay and the timer display
  "texts"       all texts of the run (phases, rolls, log lines, results)
  "lists"       random lists:
                  nodes      exactly 7 stations of the run (entry … exit)
                  hosts      targets, sculpts  descriptions of the targets
                  decks      gear,    folders  where the winner file is found
                  demo_names made-up names for demo mode (optional)
                  demo_prizes sample prizes for demo mode (optional)

Everything is optional: whatever is missing comes from the default theme
(Shadowrun) or from the default texts of the chosen game.

The "_vorlage" template is for the dice game. For the other games all
texts, labels and lists are in the "_ablaeufe" folder (e.g.
_ablaeufe/treasure.json, English: _ablaeufe/treasure.en.json). Set "game"
and copy only the entries you want to change into your own theme file.
Riddles are written as "question|right answer|wrong|wrong|wrong" in
"riddles", events as "kind|text" in "events" or "traps".
So you can also change just a few texts or only the colors.

Placeholders in curly braces are replaced during the run, e.g.
{decker} (streamer name), {host} (target), {ice} (opponent), {n} (number),
{os}/{max} (alert level), {kw} (keyword), {prize} (prize).
The template shows which placeholders a text knows in the same text.
{decker} and {host} work in every run text.

theme.css
---------
Overrides the color and shape variables (see the comments in the template).
Any further CSS rules may follow below.

Preview without a running giveaway:
  http://localhost:8766/test?theme=mygame
  http://localhost:8766/timer?demo=1&theme=mygame
