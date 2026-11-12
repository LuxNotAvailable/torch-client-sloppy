# Torch Default pack

"Torch Default (built-in)" is the resource pack built into the client. It holds
Torch's own visual tweaks, each one off when the pack is unselected. This guide
says how a tweak joins it.

## What the pack is

- **It has no files.** It shows in the Resource Packs list (`resourcepacks::list`)
  like any pack, but selecting it only sets `resourcepacks::builtin_enabled()`.
  Each tweak reads that flag where it builds its data.
- **It starts on.** A fresh install selects it (`resourcepacks::selected`). A
  player's saved selection is never changed for them.
- **It sits in the priority order.** Packs above it win and packs below it lose,
  as with any pack. A tweak resolves its own data after the mounted packs', so a
  pack that does the same thing replaces it.
- **Changes need a restart,** like other packs. Tweaks build their data once at
  startup (atlas tiles, rule tables), and the screen already asks to restart.

## Rules for a tweak

1. **Ship no art.** Generate what the tweak needs at startup from the textures
   that are loaded, which are vanilla's or a pack's. Connected glass cuts its 47
   tiles from each glass texture (`ctm::connected_tile`). This keeps Mojang's and
   third-party art out of the repo, and a pack that retextures a block gets the
   tweak in its own style.
2. **Reuse the pack formats.** If OptiFine, Continuity or vanilla already has a
   format for the effect, build the tweak as data in that format. Connected
   glass produces ordinary CTM rules (`ctm::builtin`), and the same mesher path
   draws packs' rules and the built-in ones.
3. **Stay vanilla-shaped.** Change how a block looks, not its geometry,
   collision or gameplay. Where vanilla's model leaves a gap the tweak needs
   (pane end caps, stacked pane edges), fix it in the mesher for every pack and
   name the vanilla behaviour in a comment.
4. **Cost nothing when off.** An unselected pack must leave the mesher and atlas
   exactly as they would be without it. Check `tracks_kinds()`, or the tweak's
   own flag, before doing per-quad work.
5. **One switch.** Tweaks don't get their own menu toggles. If one later needs a
   setting, it goes in the pack's own options, not in Video Settings.

## Naming

- Atlas keys for generated tiles: `ctm!torch/<texture>/<index>`. Packs' tiles
  use `ctm!<path>`. The `torch/` prefix keeps the two apart.
- Tweak functions say what they make: `connected_tile`, `representative_masks`.
- Describe the pack to players as "Torch's own visual tweaks". Don't list the
  tweaks in its description, since the list would go stale.

## Adding a tweak

1. Generate the tweak's data where the matching pack data is built (for
   textures, `atlas::Textures::blocks`), gated on `builtin_enabled()`.
2. Add a unit test for the generator (see `ctm::tests`) and a mesher test with
   real block states if it changes geometry (see `mesh::pane_tests`).
3. Run the client with the pack on and off, with and without a pack that does
   the same thing. Check that the higher-priority one wins.
4. Add the tweak to this file's list below and to the `BUILTIN` doc comment.

## Tweaks so far

- **Connected glass** (`renderer/ctm.rs`): glass, tinted glass, the 16 stained
  glasses and their panes. Pane extras in `mesh.rs`: end strips sample the frame
  column, end caps take the pane's edge texture, and stacked panes hide the
  edges between them.
