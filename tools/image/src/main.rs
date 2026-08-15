// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::PathBuf};

use bao_image::{
    ClassicalVerification, CopyOptions, PqVerification, SignOptions, copy_image, inspect_file,
    pack_elf, sign_image,
};
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
    /// Inspect a canonical signed Baochip baremetal UF2.
    Inspect {
        input: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Preflight and copy a signed UF2 to a BAOCHIP MSC mount.
    Copy {
        input: PathBuf,
        /// Mounted BAOCHIP volume. Required unless exactly one is discovered.
        #[arg(long)]
        target: Option<PathBuf>,
        /// Validate and select the target without writing.
        #[arg(long)]
        dry_run: bool,
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
        Command::Inspect { input, json } => {
            let report = inspect_file(&input)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("profile: {}", report.profile);
                println!("blocks: {}", report.blocks);
                println!("signed image bytes: {}", report.image_bytes);
                println!("function: {}", report.function);
                println!("signature mode: {}", report.signature_mode);
                println!("signed length: {}", report.signed_len);
                println!("header version: 0x{:08x}", report.version);
                println!("corrected version: 0x{:08x}", report.corrected_version);
                println!("header compatible: {}", report.compatible_header);
                println!("anti-rollback: {}", report.anti_rollback);
                println!("minimum version: {}", report.minimum_version_hex);
                println!("image version: {}", report.image_version_hex);
                match report.classical_verification {
                    ClassicalVerification::Verified { key_slot, key_tag } => {
                        println!(
                            "classical verification: passed (embedded slot {key_slot}, tag {key_tag:?})"
                        );
                    }
                    ClassicalVerification::Failed => println!("classical verification: failed"),
                }
                match report.pq_verification {
                    PqVerification::NotPresent => println!("PQ verification: not present"),
                    PqVerification::NotImplemented => {
                        println!("PQ verification: not implemented (copy will refuse this image)")
                    }
                }
                println!("device acceptance: {}", report.device_acceptance);
            }
        }
        Command::Copy {
            input,
            target,
            dry_run,
        } => {
            let report = copy_image(CopyOptions {
                image: &input,
                target: target.as_deref(),
                dry_run,
            })?;
            if report.dry_run {
                println!(
                    "dry run: validated {} bytes for {}",
                    report.bytes,
                    report.destination.display()
                );
            } else {
                println!(
                    "copied and synced {} bytes to {}",
                    report.bytes,
                    report.destination.display()
                );
            }
        }
    }
    Ok(())
}
