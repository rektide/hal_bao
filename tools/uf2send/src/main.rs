// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    io::{self, Read, Write},
    path::PathBuf,
    time::Duration,
};

use bao_boot1_protocol::{ReplTransport, SendOptions, preflight_file, send_image};
use clap::Parser;

struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    fn open(path: &str, baud: u32, io_timeout: Duration) -> Result<Self, serialport::Error> {
        Ok(Self {
            port: serialport::new(path, baud).timeout(io_timeout).open()?,
        })
    }
}

impl Read for SerialTransport {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.port.read(buffer)
    }
}

impl Write for SerialTransport {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.port.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.port.flush()
    }
}

impl ReplTransport for SerialTransport {
    fn clear_input(&mut self) -> io::Result<()> {
        self.port
            .clear(serialport::ClearBuffer::Input)
            .map_err(io::Error::other)
    }
}

#[derive(Parser)]
#[command(about, version)]
struct Cli {
    /// Canonical Baochip baremetal UF2 to transfer.
    uf2: Option<PathBuf>,
    /// Serial port path, for example /dev/ttyACM0.
    #[arg(short, long)]
    port: Option<String>,
    /// List available serial ports and exit.
    #[arg(short, long)]
    list_ports: bool,
    /// Serial baud rate (physical Dabao UART2 uses 1,000,000).
    #[arg(short, long, default_value_t = 1_000_000)]
    baud: u32,
    /// Response timeout in milliseconds.
    #[arg(short, long, default_value_t = 500)]
    timeout_ms: u64,
    /// Delay after disabling local echo, before protocol negotiation.
    #[arg(long, default_value_t = 100)]
    settle_ms: u64,
    /// Maximum attempts for each block.
    #[arg(short, long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    retries: u32,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    if cli.list_ports {
        let ports = serialport::available_ports()?;
        if ports.is_empty() {
            println!("No serial ports found");
        } else {
            for port in ports {
                println!("{}", port.port_name);
            }
        }
        return Ok(());
    }

    let path = cli
        .uf2
        .ok_or("UF2 path is required unless --list-ports is used")?;
    let port = cli.port.ok_or("--port is required for transfer")?;
    // Validate the complete image before opening a device capable of persistent writes.
    let image = preflight_file(&path)?;
    println!("validated {} Baochip UF2 blocks", image.len());

    let timeout = Duration::from_millis(cli.timeout_ms);
    let mut transport =
        SerialTransport::open(&port, cli.baud, timeout.min(Duration::from_millis(20)))?;
    let options = SendOptions {
        response_timeout: timeout,
        retries: cli.retries,
        settle_delay: Duration::from_millis(cli.settle_ms),
        ..Default::default()
    };
    let report = send_image(&mut transport, &image, &options, |done, total| {
        eprint!("\rtransferred {done}/{total} blocks");
    })?;
    eprintln!();
    println!(
        "transferred {} blocks using {:?} acknowledgments ({} retries)",
        report.blocks, report.protocol, report.retries
    );
    Ok(())
}
