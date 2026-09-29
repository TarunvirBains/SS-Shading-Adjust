use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use oot_deeper_water::config::Config;
use oot_deeper_water::o2r::{self, Archive};
use oot_deeper_water::water;

#[derive(Parser)]
#[command(name = "oot-deeper-water", version, about)]
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
    /// Report which of the configured display lists another mod also replaces.
    Conflicts {
        /// Other mod archive; only read.
        other: PathBuf,
        #[arg(long)]
        config: PathBuf,
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
            let config = Config::load(&config)?;
            let mut archive = Archive::open(&game)?;
            let a_max = water::a_max(&mut archive, &config)?;
            let rows =
                water::translucent_materials(&mut archive, &config.water_textures, |_| true)?;
            println!(
                "# a_max {a_max:.3}, scale {:.3}, max_alpha {:.2}",
                config.scale(a_max),
                config.max_alpha
            );
            println!("dl\toffset\tcurrent\tmapped\tfinal\twater\treason\ttextures");
            for m in rows.iter().filter(|m| all || m.water) {
                let a = m.alpha as f32 / 255.0;
                let surface = config.surfaces.iter().find(|s| s.dl == m.dl);
                let (mapped, fin, reason) = match surface {
                    Some(s) if m.water => (
                        format!("{:.2}", config.mapped(a, a_max)),
                        format!("{:.2}", config.target(s, m.alpha, a_max) as f32 / 255.0),
                        s.reason.as_str(),
                    ),
                    _ => ("-".to_string(), format!("{a:.2}"), "not patched"),
                };
                println!(
                    "{}\t{:#x}\t{a:.2}\t{mapped}\t{fin}\t{}\t{reason}\t{}",
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
            let config = Config::load(&config)?;
            let mut archive = Archive::open(&game)?;
            let (entries, changes, a_max) = water::build(&mut archive, &config)?;
            println!("a_max {a_max:.3}, scale {:.3}", config.scale(a_max));
            for c in &changes {
                println!(
                    "{} @{:#x}: alpha {:.2} -> {:.2}",
                    c.dl,
                    c.offset,
                    c.from as f32 / 255.0,
                    c.to as f32 / 255.0
                );
            }
            o2r::write(&output, &entries)?;
            println!(
                "{} display lists ({} materials) -> {}",
                entries.len(),
                changes.len(),
                output.display()
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Conflicts { other, config } => {
            let config = Config::load(&config)?;
            let archive = Archive::open(&other)?;
            let mut n = 0;
            for s in &config.surfaces {
                for name in [s.dl.clone(), format!("alt/{}", s.dl)] {
                    if archive.contains(&name) {
                        println!("conflict: {name}");
                        n += 1;
                    }
                }
            }
            println!("{n} conflict(s) in {}", other.display());
            Ok(if n == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}
