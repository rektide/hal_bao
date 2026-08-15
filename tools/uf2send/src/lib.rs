// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fmt, fs,
    io::{self, Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};

pub const BLOCK_SIZE: usize = 512;
pub const BAOCHIP_FAMILY_ID: u32 = 0xa7d7_6373;
pub const BAREMETAL_START: u32 = 0x6006_0000;
pub const BAREMETAL_END: u32 = 0x6009_fd00;

const MAGIC_START0: u32 = 0x0a32_4655;
const MAGIC_START1: u32 = 0x9e5d_5157;
const MAGIC_END: u32 = 0x0ab1_6f30;
const FAMILY_ID_PRESENT: u32 = 0x0000_2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uf2Block {
    pub number: u32,
    pub address: u32,
    pub payload_size: u32,
    pub bytes: [u8; BLOCK_SIZE],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uf2Image {
    pub blocks: Vec<Uf2Block>,
}

#[derive(Debug)]
pub struct SendError(String);

impl SendError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for SendError {}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four-byte field"),
    )
}

/// Parse and validate the canonical UF2 emitted by `bao-image` for the baremetal slot.
pub fn preflight_bytes(bytes: &[u8]) -> Result<Uf2Image, SendError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(BLOCK_SIZE) {
        return Err(SendError::new(format!(
            "UF2 size must be a non-zero multiple of {BLOCK_SIZE}, got {}",
            bytes.len()
        )));
    }

    let expected_total = u32::try_from(bytes.len() / BLOCK_SIZE)
        .map_err(|_| SendError::new("UF2 has too many blocks"))?;
    let mut blocks = Vec::with_capacity(expected_total as usize);
    let mut expected_address = BAREMETAL_START;

    for (index, bytes) in bytes.chunks_exact(BLOCK_SIZE).enumerate() {
        if word(bytes, 0) != MAGIC_START0
            || word(bytes, 4) != MAGIC_START1
            || word(bytes, 508) != MAGIC_END
        {
            return Err(SendError::new(format!(
                "block {index} has invalid UF2 magic"
            )));
        }
        let flags = word(bytes, 8);
        if flags != FAMILY_ID_PRESENT {
            return Err(SendError::new(format!(
                "block {index} has non-canonical flags 0x{flags:08x}, expected 0x{FAMILY_ID_PRESENT:08x}"
            )));
        }
        let address = word(bytes, 12);
        let payload_size = word(bytes, 16);
        let number = word(bytes, 20);
        let total = word(bytes, 24);
        let family = word(bytes, 28);

        if family != BAOCHIP_FAMILY_ID {
            return Err(SendError::new(format!(
                "block {index} family is 0x{family:08x}, expected Baochip 0x{BAOCHIP_FAMILY_ID:08x}"
            )));
        }
        if payload_size != 256 {
            return Err(SendError::new(format!(
                "block {index} payload is {payload_size} bytes, expected canonical 256"
            )));
        }
        if number != index as u32 || total != expected_total {
            return Err(SendError::new(format!(
                "block {index} numbering is {number}/{total}, expected {index}/{expected_total}"
            )));
        }
        if address != expected_address {
            return Err(SendError::new(format!(
                "block {index} address is 0x{address:08x}, expected contiguous baremetal address 0x{expected_address:08x}"
            )));
        }
        let end = address
            .checked_add(payload_size)
            .ok_or_else(|| SendError::new(format!("block {index} address overflows")))?;
        if address < BAREMETAL_START || end > BAREMETAL_END {
            return Err(SendError::new(format!(
                "block {index} range 0x{address:08x}..0x{end:08x} is outside the baremetal slot"
            )));
        }

        expected_address = end;
        blocks.push(Uf2Block {
            number,
            address,
            payload_size,
            bytes: bytes.try_into().expect("exact UF2 block"),
        });
    }

    Ok(Uf2Image { blocks })
}

pub fn preflight_file(path: &Path) -> Result<Uf2Image, Box<dyn Error>> {
    Ok(preflight_bytes(&fs::read(path)?)?)
}

pub trait ReplTransport: Read + Write {
    fn clear_input(&mut self) -> io::Result<()>;
}

pub struct SerialTransport {
    port: Box<dyn serialport::SerialPort>,
}

impl SerialTransport {
    pub fn open(path: &str, baud: u32, io_timeout: Duration) -> Result<Self, serialport::Error> {
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

#[derive(Debug, Clone)]
pub struct SendOptions {
    pub response_timeout: Duration,
    pub retries: u32,
    pub retry_delay: Duration,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            response_timeout: Duration::from_millis(500),
            retries: 3,
            retry_delay: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Legacy,
    Crc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferReport {
    pub protocol: Protocol,
    pub blocks: usize,
    pub retries: u32,
}

fn write_command(transport: &mut impl ReplTransport, command: &str) -> io::Result<()> {
    transport.write_all(command.as_bytes())?;
    transport.flush()
}

fn read_until(
    transport: &mut impl ReplTransport,
    timeout: Duration,
    mut inspect: impl FnMut(&str) -> Option<bool>,
) -> io::Result<Option<bool>> {
    let deadline = Instant::now() + timeout;
    let mut pending = Vec::new();
    let mut buffer = [0_u8; 256];
    while Instant::now() < deadline {
        match transport.read(&mut buffer) {
            Ok(0) => thread::yield_now(),
            Ok(count) => {
                pending.extend_from_slice(&buffer[..count]);
                while let Some(end) = pending
                    .iter()
                    .position(|byte| *byte == b'\n' || *byte == b'\r')
                {
                    let line = String::from_utf8_lossy(&pending[..end]).trim().to_owned();
                    pending.drain(..=end);
                    if !line.is_empty()
                        && let Some(result) = inspect(&line)
                    {
                        return Ok(Some(result));
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

pub fn negotiate_protocol(
    transport: &mut impl ReplTransport,
    timeout: Duration,
) -> Result<Protocol, SendError> {
    transport
        .clear_input()
        .and_then(|()| write_command(transport, "has-crc\r"))
        .map_err(|error| SendError::new(format!("CRC probe failed: {error}")))?;
    let response = read_until(transport, timeout, |line| match line.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
    .map_err(|error| SendError::new(format!("CRC probe read failed: {error}")))?;
    Ok(if response == Some(true) {
        Protocol::Crc
    } else {
        Protocol::Legacy
    })
}

fn parse_ack(line: &str, block: &Uf2Block, protocol: Protocol, crc: u32) -> Option<bool> {
    let fields: Vec<_> = line.split_whitespace().collect();
    let expected_len = if protocol == Protocol::Crc { 6 } else { 4 };
    if fields.len() != expected_len || fields[0] != "Wrote" || fields[2] != "to" {
        return None;
    }
    let size = fields[1].parse::<u32>().ok()?;
    let address = u32::from_str_radix(fields[3].strip_prefix("0x")?, 16).ok()?;
    let crc_matches = protocol == Protocol::Legacy
        || (fields[4] == "crc" && u32::from_str_radix(fields[5], 16).ok() == Some(crc));
    Some(size == block.payload_size && address == block.address && crc_matches)
}

fn set_echo(transport: &mut impl ReplTransport, enabled: bool) -> io::Result<()> {
    let command = if enabled {
        "localecho on\r"
    } else {
        "localecho off\r"
    };
    if enabled {
        return write_command(transport, command);
    }
    // Boot1 may corrupt the first command while USB serial settles. Sending slowly twice
    // mirrors the upstream sender and ensures local echo is off before large commands.
    for _ in 0..2 {
        for byte in command.bytes() {
            transport.write_all(&[byte])?;
            transport.flush()?;
        }
    }
    Ok(())
}

pub fn send_image(
    transport: &mut impl ReplTransport,
    image: &Uf2Image,
    options: &SendOptions,
    mut progress: impl FnMut(usize, usize),
) -> Result<TransferReport, SendError> {
    if options.retries == 0 {
        return Err(SendError::new("retries must be at least 1"));
    }
    let protocol = negotiate_protocol(transport, options.response_timeout)?;
    if let Err(error) = set_echo(transport, false) {
        let _ = set_echo(transport, true);
        return Err(SendError::new(format!(
            "could not disable local echo: {error}"
        )));
    }

    let transfer = (|| {
        let mut retry_count = 0;
        for (index, block) in image.blocks.iter().enumerate() {
            let encoded = STANDARD.encode(block.bytes);
            let crc = crc32fast::hash(&block.bytes);
            let command = match protocol {
                Protocol::Legacy => format!("uf2 {encoded}\r"),
                Protocol::Crc => format!("uf2 {encoded} {crc:08x}\r"),
            };
            for attempt in 1..=options.retries {
                transport
                    .clear_input()
                    .and_then(|()| write_command(transport, &command))
                    .map_err(|error| {
                        SendError::new(format!("block {index} write failed: {error}"))
                    })?;
                let response = read_until(transport, options.response_timeout, |line| {
                    if line.starts_with("CRC error ") {
                        Some(false)
                    } else {
                        parse_ack(line, block, protocol, crc)
                    }
                })
                .map_err(|error| SendError::new(format!("block {index} read failed: {error}")))?;
                if response == Some(true) {
                    retry_count += attempt - 1;
                    progress(index + 1, image.blocks.len());
                    break;
                }
                let last_error = match response {
                    Some(false) => "device returned a mismatched acknowledgment".to_owned(),
                    None => "timed out waiting for an exact acknowledgment".to_owned(),
                    Some(true) => unreachable!(),
                };
                if attempt < options.retries {
                    thread::sleep(options.retry_delay);
                } else {
                    return Err(SendError::new(format!(
                        "block {index} failed after {attempt} attempts: {last_error}"
                    )));
                }
            }
        }
        Ok(TransferReport {
            protocol,
            blocks: image.blocks.len(),
            retries: retry_count,
        })
    })();

    let cleanup = set_echo(transport, true)
        .map_err(|error| SendError::new(format!("could not restore local echo: {error}")));
    match (transfer, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(report), Ok(())) => Ok(report),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    fn block(number: u32, total: u32, address: u32) -> [u8; BLOCK_SIZE] {
        let mut bytes = [0_u8; BLOCK_SIZE];
        for (offset, value) in [
            (0, MAGIC_START0),
            (4, MAGIC_START1),
            (8, FAMILY_ID_PRESENT),
            (12, address),
            (16, 256),
            (20, number),
            (24, total),
            (28, BAOCHIP_FAMILY_ID),
            (508, MAGIC_END),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[derive(Default)]
    struct MockBoot1 {
        crc: bool,
        fail_first_block: bool,
        block_attempts: usize,
        reads: VecDeque<u8>,
        writes: Vec<u8>,
        command: Vec<u8>,
    }

    impl MockBoot1 {
        fn respond(&mut self, command: &str) {
            let response = if command == "has-crc" {
                if self.crc {
                    "true\r\n".to_owned()
                } else {
                    "unknown command\r\n".to_owned()
                }
            } else if command.starts_with("uf2 ") {
                self.block_attempts += 1;
                if self.fail_first_block && self.block_attempts == 1 {
                    "Wrote 255 to 0x60060000\r\n".to_owned()
                } else {
                    let parts: Vec<_> = command.split_whitespace().collect();
                    if self.crc {
                        format!("Wrote 256 to 0x60060000 crc {}\r\n", parts[2])
                    } else {
                        "Wrote 256 to 0x60060000\r\n".to_owned()
                    }
                }
            } else {
                String::new()
            };
            self.reads.extend(response.bytes());
        }
    }

    impl Read for MockBoot1 {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.reads.is_empty() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "mock timeout"));
            }
            let count = output.len().min(self.reads.len());
            for byte in &mut output[..count] {
                *byte = self.reads.pop_front().unwrap();
            }
            Ok(count)
        }
    }

    impl Write for MockBoot1 {
        fn write(&mut self, input: &[u8]) -> io::Result<usize> {
            self.writes.extend_from_slice(input);
            for byte in input {
                if *byte == b'\r' {
                    let command = String::from_utf8(std::mem::take(&mut self.command)).unwrap();
                    self.respond(&command);
                } else {
                    self.command.push(*byte);
                }
            }
            Ok(input.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl ReplTransport for MockBoot1 {
        fn clear_input(&mut self) -> io::Result<()> {
            self.reads.clear();
            Ok(())
        }
    }

    fn image() -> Uf2Image {
        preflight_bytes(&block(0, 1, BAREMETAL_START)).unwrap()
    }

    #[test]
    fn preflight_rejects_wrong_family_and_noncontiguous_blocks() {
        let mut wrong_family = block(0, 1, BAREMETAL_START);
        wrong_family[28..32].copy_from_slice(&0_u32.to_le_bytes());
        assert!(
            preflight_bytes(&wrong_family)
                .unwrap_err()
                .to_string()
                .contains("family")
        );

        let mut bytes = block(0, 2, BAREMETAL_START).to_vec();
        bytes.extend_from_slice(&block(1, 2, BAREMETAL_START + 512));
        assert!(
            preflight_bytes(&bytes)
                .unwrap_err()
                .to_string()
                .contains("contiguous")
        );

        let blocks_in_slot = (BAREMETAL_END - BAREMETAL_START) / 256;
        let total = blocks_in_slot + 1;
        let mut outside_slot = Vec::with_capacity(total as usize * BLOCK_SIZE);
        for number in 0..total {
            outside_slot.extend_from_slice(&block(number, total, BAREMETAL_START + number * 256));
        }
        assert!(
            preflight_bytes(&outside_slot)
                .unwrap_err()
                .to_string()
                .contains("baremetal slot")
        );
    }

    #[test]
    fn negotiates_crc_and_sends_exact_crc_command() {
        let mut mock = MockBoot1 {
            crc: true,
            ..Default::default()
        };
        let report = send_image(&mut mock, &image(), &SendOptions::default(), |_, _| {}).unwrap();
        let writes = String::from_utf8(mock.writes).unwrap();
        assert_eq!(report.protocol, Protocol::Crc);
        assert!(writes.contains("uf2 "));
        assert!(writes.ends_with("localecho on\r"));
    }

    #[test]
    fn legacy_sender_retries_mismatch_and_restores_echo() {
        let mut mock = MockBoot1 {
            fail_first_block: true,
            ..Default::default()
        };
        let options = SendOptions {
            retry_delay: Duration::ZERO,
            ..Default::default()
        };
        let report = send_image(&mut mock, &image(), &options, |_, _| {}).unwrap();
        assert_eq!(report.protocol, Protocol::Legacy);
        assert_eq!(report.retries, 1);
        assert!(mock.writes.ends_with(b"localecho on\r"));
    }

    #[test]
    fn restores_echo_after_bounded_failure() {
        let mut mock = MockBoot1 {
            fail_first_block: true,
            ..Default::default()
        };
        let options = SendOptions {
            retries: 1,
            response_timeout: Duration::from_millis(1),
            ..Default::default()
        };
        assert!(send_image(&mut mock, &image(), &options, |_, _| {}).is_err());
        assert!(mock.writes.ends_with(b"localecho on\r"));
    }
}
