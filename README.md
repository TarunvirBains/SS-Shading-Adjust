# OoT Deeper Water

A small mod for Ship of Harkinian: Ocarina of Time's translucent water surfaces become more
opaque, so ponds and rivers read as water rather than a light floor seen through a thin film.
It ships patched copies of the scene display lists; only the alpha byte of the water materials'
primitive color changes.

- **Proportional mapping** (`deeper-water.toml`): opacity `a` becomes `a * max_alpha / a_max`,
  capped at `max_alpha` (0.70). Every patched surface gets more opaque and the ratios between
  surfaces stay; nothing is made less opaque.
- **Gameplay first**: surfaces where you dive or need to see below get a lower `max_alpha`
  (Lake Hylia, Zora's Domain and Fountain, the Deku Tree basement, the Kakariko well, the Lost
  Woods pool, grottos, the Ice Cavern). The Water Temple, Jabu-Jabu's Belly, Gerudo Training
  Ground's diving room, the Lakeside Laboratory tank and the fishing pond keep their vanilla
  water.

## Use

```sh
cargo build --release --locked
oot-deeper-water audit  oot.o2r --config deeper-water.toml      # the water materials, TSV
oot-deeper-water build  oot.o2r OoT-Deeper-Water.o2r --config deeper-water.toml
oot-deeper-water conflicts other-mod.o2r --config deeper-water.toml
```

The game archive is only read. Put `OoT-Deeper-Water.o2r` in SoH's `mods` folder. It replaces
display lists, not textures, so it combines with texture packs (check with `conflicts`).

Tests: `cargo test --release --locked`; set `OOT_O2R` to a game archive to also round-trip every
display list in it.

## License

MIT OR Apache-2.0.
