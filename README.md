# SS Shading Adjust

Engine-side look adjustments for Ship of Harkinian (Ocarina of Time), in the direction of
Skyward Sword. It ships patched copies of game resources (display lists) in a small `.o2r`
mod; every patch changes only the bytes it names. Texture restyling is a separate tool.

The adjustments live in `ss-shading-adjust.toml`, one section each.

## `[water]`: deeper water

Ocarina of Time's translucent water surfaces become more opaque, so ponds and rivers read as
water rather than a light floor seen through a thin film. Only the alpha byte of the water
materials' primitive color changes.

- **Proportional mapping**: opacity `a` becomes `a * max_alpha / a_max`, at most `max_alpha`
  (0.70). Every patched surface gets more opaque and the ratios between surfaces stay; water
  already at or above `max_alpha` keeps its value, nothing is made less opaque.
- **Gameplay first**: surfaces where you dive or need to see below get a lower `max_alpha`
  (Lake Hylia, Zora's Domain and Fountain, the Deku Tree basement, the Kakariko well, the Lost
  Woods pool, grottos, the Ice Cavern). The Water Temple, Jabu-Jabu's Belly, Gerudo Training
  Ground's diving room, the Lakeside Laboratory tank and the fishing pond keep their vanilla
  water.

## Use

```sh
cargo build --release --locked
ss-shading-adjust audit     oot.o2r --config ss-shading-adjust.toml   # water materials, TSV
ss-shading-adjust build     oot.o2r SS-Shading-Adjust.o2r --config ss-shading-adjust.toml
ss-shading-adjust conflicts other-mod.o2r --config ss-shading-adjust.toml
```

The game archive is only read. Put `SS-Shading-Adjust.o2r` in SoH's `mods` folder. It replaces
display lists, not textures, so it combines with texture packs (check with `conflicts`).

Tests: `cargo test --release --locked`; set `OOT_O2R` to a game archive to also round-trip every
display list in it.

## License

MIT OR Apache-2.0.
