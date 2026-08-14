// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::PathBuf};

use bao_image::{SignOptions, pack_elf, sign_image};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about, version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a Zephyr ELF to a Baochip presign image.
    Pack { input: PathBuf, output: PathBuf },
    /// Sign a presign image and emit UF2 using Xous libraries.
    Sign {
        input: PathBuf,
        output: PathBuf,
        #[arg(long)]
        key: PathBuf,
        #[arg(long, default_value = "v0.9.8-790")]
        min_xous_version: String,
        #[arg(long)]
        git_describe: String,
        #[arg(long, requires = "pq_key_cache")]
        pq_key: Option<PathBuf>,
        #[arg(long, requires = "pq_key")]
        pq_key_cache: Option<PathBuf>,
    },
}

fn main() -> Result<(), Box<dyn Error>> {
    match Cli::parse().command {
        Command::Pack { input, output } => {
            let size = pack_elf(&input, &output)?;
            println!("packed {size} ROM bytes into {}", output.display());
        }
        Command::Sign {
            input,
            output,
            key,
            min_xous_version,
            git_describe,
            pq_key,
            pq_key_cache,
        } => {
            let uf2 = sign_image(SignOptions {
                input: &input,
                output: &output,
                key: &key,
                min_xous_version: &min_xous_version,
                git_describe: &git_describe,
                pq_key: pq_key.as_deref(),
                pq_key_cache: pq_key_cache.as_deref(),
            })?;
            println!("signed {} and wrote {}", output.display(), uf2.display());
        }
    }
    Ok(())
}
