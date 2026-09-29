use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ss_shading_adjust::config::Config;
use ss_shading_adjust::o2r::{self, Archive};
use ss_shading_adjust::water;

#[derive(Parser)]
#[command(name = "ss-shading-adjust", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the translucent materials (primitive alpha below 255) of every display list, as TSV.
    Audit {
        /// Game archive (e.g. oot.o2r); only read.
        game: PathBuf,
        /// Config (for the water textures and the planned surfaces).
        #[arg(long)]
        config: PathBuf,
        /// Also list translucent materials that draw no water texture.
        #[arg(long)]
        all: bool,
    },
    /// Build the mod: the configured surfaces' display lists with their water opacity raised.
    Build {
        /// Game archive (e.g. oot.o2r); only read.
        game: PathBuf,
        /// Output `.o2r`.
        output: PathBuf,
        #[arg(long)]
        config: PathBuf,
    },
    /// List the materials (render state per run of triangles) of the display lists matching a
    /// glob, as TSV.
    Materials {
        /// Game archive; only read.
        game: PathBuf,
        /// Display-list path glob (`*` within a segment, `**` across).
        glob: String,
    },
    /// Report which resources of a built mod other mods also replace (same path, or its `alt/`
    /// variant).
    Conflicts {
        /// The built mod.
        built: PathBuf,
        /// Other mod archives; only read.
        others: Vec<PathBuf>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Audit { game, config, all } => {
            let config = Config::load(&config)?.water;
            let mut archive = Archive::open(&game)?;
            let a_max = water::a_max(&mut archive, &config)?;
            let rows =
                water::translucent_materials(&mut archive, &config.water_textures, |_| true)?;
            println!(
                "# a_max {a_max:.3}, scale {:.3}, max_alpha {:.2}",
                config.scale(a_max),
                config.max_alpha
            );
            if let Some(m) = rows.iter().filter(|m| m.water).max_by_key(|m| m.alpha) {
                println!(
                    "# most opaque water: {} @{:#x}, alpha {:.3} ({}/255)",
                    m.dl,
                    m.offset,
                    m.alpha as f32 / 255.0,
                    m.alpha
                );
            }
            println!("dl\toffset\tcurrent\tmapped\tfinal\tstatus\twater\treason\ttextures");
            for m in rows.iter().filter(|m| all || m.water) {
                let a = m.alpha as f32 / 255.0;
                let surface = config.surfaces.iter().find(|s| s.dl == m.dl);
                let (mapped, fin, status, reason) = match surface {
                    Some(s) if m.water => {
                        let (mapped, fin) =
                            (config.mapped(a, a_max), config.target(s, m.alpha, a_max));
                        let status = if config.at_or_above_max(a) {
                            "unchanged: at or above max_alpha"
                        } else if (fin as f32 / 255.0) < mapped - 0.5 / 255.0 {
                            "limited (gameplay)"
                        } else {
                            "mapped"
                        };
                        (
                            format!("{mapped:.2}"),
                            format!("{:.2}", fin as f32 / 255.0),
                            status,
                            s.reason.as_str(),
                        )
                    }
                    _ => (
                        "-".to_string(),
                        format!("{a:.2}"),
                        "not patched",
                        if config.at_or_above_max(a) && m.water {
                            "at or above max_alpha"
                        } else {
                            "-"
                        },
                    ),
                };
                println!(
                    "{}\t{:#x}\t{a:.2}\t{mapped}\t{fin}\t{status}\t{}\t{reason}\t{}",
                    m.dl,
                    m.offset,
                    if m.water { "water" } else { "-" },
                    m.textures.join(" ")
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Build {
            game,
            output,
            config,
        } => {
            let full = Config::load(&config)?;
            let config = &full.water;
            let mut archive = Archive::open(&game)?;
            let (entries, changes, a_max) = water::build(&mut archive, config)?;
            println!("water: a_max {a_max:.3}, scale {:.3}", config.scale(a_max));
            for c in &changes {
                println!(
                    "{} @{:#x}: alpha {:.2} -> {:.2}",
                    c.dl,
                    c.offset,
                    c.from as f32 / 255.0,
                    c.to as f32 / 255.0
                );
            }
            println!(
                "water: {} display lists ({} materials)",
                entries.len(),
                changes.len()
            );
            let mut entries: std::collections::BTreeMap<String, Vec<u8>> =
                entries.into_iter().collect();
            if let Some(g) = full.ground.as_ref().filter(|g| g.enabled) {
                let names = archive.names();
                for scene in g.scenes.iter().filter(|s| s.enabled) {
                    let earth = ss_shading_adjust::ground::earth_set(scene);
                    let (mut patched, mut skipped) = (0, 0);
                    for name in names
                        .iter()
                        .filter(|n| ss_shading_adjust::glob::matches(&scene.dls, n))
                    {
                        let bytes = match entries.get(name) {
                            Some(b) => b.clone(),
                            None => archive.read(name)?,
                        };
                        if !ss_shading_adjust::dl::is_dl(&bytes) {
                            continue;
                        }
                        let (out, changes) =
                            ss_shading_adjust::ground::patch_dl(name, &bytes, &earth, |v| {
                                g.tint(scene, v)
                            })?;
                        for c in &changes {
                            println!(
                                "ground {}: {} @{:#x}: prim {:08x} -> {:08x} ({} tris){}",
                                scene.name,
                                c.dl,
                                c.offset,
                                c.from,
                                c.to,
                                c.tris,
                                if c.skipped.is_empty() {
                                    String::new()
                                } else {
                                    format!(" SKIPPED: {}", c.skipped)
                                }
                            );
                            if c.skipped.is_empty() {
                                patched += 1;
                            } else {
                                skipped += 1;
                            }
                        }
                        if out != bytes {
                            entries.insert(name.clone(), out);
                        }
                    }
                    println!(
                        "ground {}: {patched} primitive colors patched, {skipped} skipped",
                        scene.name
                    );
                }
            }
            let entries: Vec<(String, Vec<u8>)> = entries.into_iter().collect();
            o2r::write(&output, &entries)?;
            println!("{} resources -> {}", entries.len(), output.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Materials { game, glob } => {
            let mut archive = Archive::open(&game)?;
            let names = water::names_by_hash(&archive);
            let short = |h: &u64| {
                names
                    .get(h)
                    .map(|n| n.rsplit('/').next().unwrap_or(n).to_string())
                    .unwrap_or_else(|| format!("{h:016x}"))
            };
            println!("dl\ttris\tlit\t2cyc\ttextures\tcombine\tcycle1\tcycle2\tprim\tenv\tvtx");
            for name in archive.names() {
                if !ss_shading_adjust::glob::matches(&glob, &name) {
                    continue;
                }
                let bytes = archive.read(&name)?;
                if !ss_shading_adjust::dl::is_dl(&bytes) {
                    continue;
                }
                let cmds = ss_shading_adjust::dl::parse(&bytes)?;
                for m in ss_shading_adjust::material::materials(&cmds) {
                    let (comb, c1, c2) = match m.combine {
                        Some((o, w0, w1)) => {
                            let [x, y] = ss_shading_adjust::material::color_cycles(w0, w1);
                            (
                                format!("{o:#x}:{w0:06x}/{w1:08x}"),
                                x.to_string(),
                                y.to_string(),
                            )
                        }
                        None => ("-".into(), "-".into(), "-".into()),
                    };
                    let col = |c: Option<(usize, u32)>| {
                        c.map_or("-".to_string(), |(o, v)| format!("{o:#x}:{v:08x}"))
                    };
                    println!(
                        "{name}\t{}\t{:?}\t{:?}\t{}\t{comb}\t{c1}\t{c2}\t{}\t{}\t{}",
                        m.tris,
                        m.lit,
                        m.two_cycle,
                        m.textures.iter().map(short).collect::<Vec<_>>().join(" "),
                        col(m.prim),
                        col(m.env),
                        m.vtx
                            .iter()
                            .map(|(_, h)| short(h))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Conflicts { built, others } => {
            let ours = Archive::open(&built)?.names();
            let mut n = 0;
            for other in &others {
                let archive = Archive::open(other)?;
                for s in &ours {
                    for name in [s.clone(), format!("alt/{s}")] {
                        if archive.contains(&name) {
                            println!("conflict: {name} in {}", other.display());
                            n += 1;
                        }
                    }
                }
            }
            println!("{n} conflict(s) with {} other archive(s)", others.len());
            Ok(if n == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}
