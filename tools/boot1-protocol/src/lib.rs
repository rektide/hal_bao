// SPDX-License-Identifier: Apache-2.0

use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use thiserror::Error;

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
    number: u32,
    address: u32,
    payload_size: u32,
    bytes: [u8; BLOCK_SIZE],
}

impl Uf2Block {
    pub fn number(&self) -> u32 {
        self.number
    }

    pub fn address(&self) -> u32 {
        self.address
    }

    pub fn payload_size(&self) -> u32 {
        self.payload_size
    }

    pub fn bytes(&self) -> &[u8; BLOCK_SIZE] {
        &self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uf2Image {
    blocks: Vec<Uf2Block>,
}

impl Uf2Image {
    pub fn blocks(&self) -> &[Uf2Block] {
        &self.blocks
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Uf2Block> {
        self.blocks.iter()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PreflightError {
    #[error("UF2 size must be a non-zero multiple of {BLOCK_SIZE}, got {actual}")]
    InvalidSize { actual: usize },
    #[error("UF2 has too many blocks")]
    TooManyBlocks,
    #[error("block {block} has invalid UF2 magic")]
    InvalidMagic { block: usize },
    #[error("block {block} has non-canonical flags 0x{actual:08x}, expected 0x{expected:08x}")]
    NonCanonicalFlags {
        block: usize,
        actual: u32,
        expected: u32,
    },
    #[error("block {block} family is 0x{actual:08x}, expected Baochip 0x{expected:08x}")]
    WrongFamily {
        block: usize,
        actual: u32,
        expected: u32,
    },
    #[error("block {block} payload is {actual} bytes, expected canonical 256")]
    WrongPayloadSize { block: usize, actual: u32 },
    #[error("block {block} numbering is {number}/{total}, expected {block}/{expected_total}")]
    WrongNumbering {
        block: usize,
        number: u32,
        total: u32,
        expected_total: u32,
    },
    #[error(
        "block {block} address is 0x{actual:08x}, expected contiguous baremetal address 0x{expected:08x}"
    )]
    NonContiguousAddress {
        block: usize,
        actual: u32,
        expected: u32,
    },
    #[error("block {block} address overflows")]
    AddressOverflow { block: usize },
    #[error("block {block} range 0x{start:08x}..0x{end:08x} is outside the baremetal slot")]
    OutsideBaremetalSlot { block: usize, start: u32, end: u32 },
}

#[derive(Debug, Error)]
pub enum ImageFileError {
    #[error("could not read UF2 image: {0}")]
    Read(#[from] io::Error),
    #[error(transparent)]
    Invalid(#[from] PreflightError),
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four-byte field"),
    )
}

/// Parse and validate the canonical UF2 emitted by `bao-image` for the baremetal slot.
pub fn preflight_bytes(bytes: &[u8]) -> Result<Uf2Image, PreflightError> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(BLOCK_SIZE) {
        return Err(PreflightError::InvalidSize {
            actual: bytes.len(),
        });
    }

    let expected_total =
        u32::try_from(bytes.len() / BLOCK_SIZE).map_err(|_| PreflightError::TooManyBlocks)?;
    let mut blocks = Vec::with_capacity(expected_total as usize);
    let mut expected_address = BAREMETAL_START;

    for (index, bytes) in bytes.chunks_exact(BLOCK_SIZE).enumerate() {
        if word(bytes, 0) != MAGIC_START0
            || word(bytes, 4) != MAGIC_START1
            || word(bytes, 508) != MAGIC_END
        {
            return Err(PreflightError::InvalidMagic { block: index });
        }
        let flags = word(bytes, 8);
        if flags != FAMILY_ID_PRESENT {
            return Err(PreflightError::NonCanonicalFlags {
                block: index,
                actual: flags,
                expected: FAMILY_ID_PRESENT,
            });
        }
        let address = word(bytes, 12);
        let payload_size = word(bytes, 16);
        let number = word(bytes, 20);
        let total = word(bytes, 24);
        let family = word(bytes, 28);

        if family != BAOCHIP_FAMILY_ID {
            return Err(PreflightError::WrongFamily {
                block: index,
                actual: family,
                expected: BAOCHIP_FAMILY_ID,
            });
        }
        if payload_size != 256 {
            return Err(PreflightError::WrongPayloadSize {
                block: index,
                actual: payload_size,
            });
        }
        if number != index as u32 || total != expected_total {
            return Err(PreflightError::WrongNumbering {
                block: index,
                number,
                total,
                expected_total,
            });
        }
        if address != expected_address {
            return Err(PreflightError::NonContiguousAddress {
                block: index,
                actual: address,
                expected: expected_address,
            });
        }
        let end = address
            .checked_add(payload_size)
            .ok_or(PreflightError::AddressOverflow { block: index })?;
        if address < BAREMETAL_START || end > BAREMETAL_END {
            return Err(PreflightError::OutsideBaremetalSlot {
                block: index,
                start: address,
                end,
            });
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

pub fn preflight_file(path: &Path) -> Result<Uf2Image, ImageFileError> {
    Ok(preflight_bytes(&fs::read(path)?)?)
}

pub trait ReplTransport: Read + Write {
    fn clear_input(&mut self) -> io::Result<()>;
}

#[derive(Debug, Clone)]
pub struct SendOptions {
    pub response_timeout: Duration,
    pub retries: u32,
    pub retry_delay: Duration,
    /// Delay after duplicate byte-wise `localecho off` commands, before negotiation.
    pub settle_delay: Duration,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            response_timeout: Duration::from_millis(500),
            retries: 3,
            retry_delay: Duration::from_millis(100),
            settle_delay: Duration::from_millis(100),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Legacy,
    Crc,
}

/// Successful command parsing reported by boot1.
///
/// A `Wrote` acknowledgment does not report RRAM persistence. In particular, current stock boot1
/// can acknowledge a failed RRAM write because its write error is sent only to DUART. Perform a
/// post-write audit and boot validation before treating the image as installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferReport {
    pub protocol: Protocol,
    pub blocks: usize,
    pub retries: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckFailure {
    Mismatch,
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AckResponse {
    Accepted,
    Mismatch,
    DeviceWrite(String),
}

impl std::fmt::Display for AckFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Mismatch => "device returned a mismatched acknowledgment",
            Self::Timeout => "timed out waiting for an exact acknowledgment",
        })
    }
}

#[derive(Debug, Error)]
pub enum SendError {
    #[error("retries must be at least 1")]
    InvalidRetries,
    #[error("could not disable local echo: {0}")]
    DisableEcho(#[source] io::Error),
    #[error("CRC probe write failed: {0}")]
    ProbeWrite(#[source] io::Error),
    #[error("CRC probe read failed: {0}")]
    ProbeRead(#[source] io::Error),
    #[error("block {block} write failed: {source}")]
    BlockWrite {
        block: usize,
        #[source]
        source: io::Error,
    },
    #[error("block {block} read failed: {source}")]
    BlockRead {
        block: usize,
        #[source]
        source: io::Error,
    },
    /// A boot1 variant reported an in-band device write error.
    ///
    /// Current stock boot1 does not expose its RRAM write error on the REPL transport, so this
    /// error cannot detect stock boot1 persistence failures.
    #[error("block {block} device write failed: {report}")]
    DeviceWrite { block: usize, report: String },
    #[error("block {block} failed after {attempts} attempts: {reason}")]
    BlockFailed {
        block: usize,
        attempts: u32,
        reason: AckFailure,
    },
    #[error("could not restore local echo: {0}")]
    RestoreEcho(#[source] io::Error),
}

fn write_command(transport: &mut impl ReplTransport, command: &str) -> io::Result<()> {
    transport.write_all(command.as_bytes())?;
    transport.flush()
}

fn read_until<T>(
    transport: &mut impl ReplTransport,
    timeout: Duration,
    mut inspect: impl FnMut(&str) -> Option<T>,
) -> io::Result<Option<T>> {
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
        .map_err(SendError::ProbeWrite)?;
    let response = read_until(transport, timeout, |line| match line.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    })
    .map_err(SendError::ProbeRead)?;
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

/// Transfer a preflighted image and require an exact command acknowledgment for every block.
///
/// An in-band `Write error` from a boot1 variant fails the transfer even if followed by `Wrote`.
/// Current stock boot1 sends RRAM write errors only to DUART and unconditionally sends `Wrote` on
/// the REPL, so success confirms command parsing only. Perform a post-write audit and boot
/// validation before treating the image as installed.
pub fn send_image(
    transport: &mut impl ReplTransport,
    image: &Uf2Image,
    options: &SendOptions,
    mut progress: impl FnMut(usize, usize),
) -> Result<TransferReport, SendError> {
    if options.retries == 0 {
        return Err(SendError::InvalidRetries);
    }
    if let Err(error) = set_echo(transport, false) {
        let _ = set_echo(transport, true);
        return Err(SendError::DisableEcho(error));
    }
    thread::sleep(options.settle_delay);

    let transfer = (|| {
        let protocol = negotiate_protocol(transport, options.response_timeout)?;
        let mut retry_count = 0;
        for (index, block) in image.iter().enumerate() {
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
                    .map_err(|source| SendError::BlockWrite {
                        block: index,
                        source,
                    })?;
                let response = read_until(transport, options.response_timeout, |line| {
                    if line.starts_with("Write error") {
                        Some(AckResponse::DeviceWrite(line.to_owned()))
                    } else if line.starts_with("CRC error ") {
                        Some(AckResponse::Mismatch)
                    } else {
                        parse_ack(line, block, protocol, crc).map(|accepted| {
                            if accepted {
                                AckResponse::Accepted
                            } else {
                                AckResponse::Mismatch
                            }
                        })
                    }
                })
                .map_err(|source| SendError::BlockRead {
                    block: index,
                    source,
                })?;
                let reason = match response {
                    Some(AckResponse::Accepted) => {
                        retry_count += attempt - 1;
                        progress(index + 1, image.len());
                        break;
                    }
                    Some(AckResponse::DeviceWrite(report)) => {
                        return Err(SendError::DeviceWrite {
                            block: index,
                            report,
                        });
                    }
                    Some(AckResponse::Mismatch) => AckFailure::Mismatch,
                    None => AckFailure::Timeout,
                };
                if attempt < options.retries {
                    thread::sleep(options.retry_delay);
                } else {
                    return Err(SendError::BlockFailed {
                        block: index,
                        attempts: attempt,
                        reason,
                    });
                }
            }
        }
        Ok(TransferReport {
            protocol,
            blocks: image.len(),
            retries: retry_count,
        })
    })();

    let cleanup = set_echo(transport, true).map_err(SendError::RestoreEcho);
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
        in_band_write_error: bool,
        duart_only_write_error: bool,
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
                if self.in_band_write_error {
                    let parts: Vec<_> = command.split_whitespace().collect();
                    if self.crc {
                        format!(
                            "Write error\r\nWrote 256 to 0x60060000 crc {}\r\n",
                            parts[2]
                        )
                    } else {
                        "Write error\r\nWrote 256 to 0x60060000\r\n".to_owned()
                    }
                } else if self.fail_first_block && self.block_attempts == 1 {
                    "Wrote 255 to 0x60060000\r\n".to_owned()
                } else {
                    // Stock boot1's DUART-only write error is absent from the REPL response, so
                    // duart_only_write_error intentionally has the same in-band result as success.
                    let _ = self.duart_only_write_error;
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

    fn test_options() -> SendOptions {
        SendOptions {
            settle_delay: Duration::ZERO,
            retry_delay: Duration::ZERO,
            ..Default::default()
        }
    }

    #[test]
    fn preflight_rejects_wrong_family_and_noncontiguous_blocks() {
        let mut wrong_family = block(0, 1, BAREMETAL_START);
        wrong_family[28..32].copy_from_slice(&0_u32.to_le_bytes());
        assert!(matches!(
            preflight_bytes(&wrong_family),
            Err(PreflightError::WrongFamily { .. })
        ));

        let mut bytes = block(0, 2, BAREMETAL_START).to_vec();
        bytes.extend_from_slice(&block(1, 2, BAREMETAL_START + 512));
        assert!(matches!(
            preflight_bytes(&bytes),
            Err(PreflightError::NonContiguousAddress { .. })
        ));

        let blocks_in_slot = (BAREMETAL_END - BAREMETAL_START) / 256;
        let total = blocks_in_slot + 1;
        let mut outside_slot = Vec::with_capacity(total as usize * BLOCK_SIZE);
        for number in 0..total {
            outside_slot.extend_from_slice(&block(number, total, BAREMETAL_START + number * 256));
        }
        assert!(matches!(
            preflight_bytes(&outside_slot),
            Err(PreflightError::OutsideBaremetalSlot { .. })
        ));
    }

    #[test]
    fn negotiates_crc_and_sends_exact_crc_command() {
        let raw_block = block(0, 1, BAREMETAL_START);
        let image = preflight_bytes(&raw_block).unwrap();
        let mut mock = MockBoot1 {
            crc: true,
            ..Default::default()
        };
        let report = send_image(&mut mock, &image, &test_options(), |_, _| {}).unwrap();
        let encoded = STANDARD.encode(raw_block);
        let crc = crc32fast::hash(&raw_block);
        let expected = format!(
            "localecho off\rlocalecho off\rhas-crc\ruf2 {encoded} {crc:08x}\rlocalecho on\r"
        );

        assert_eq!(report.protocol, Protocol::Crc);
        assert_eq!(String::from_utf8(mock.writes).unwrap(), expected);
    }

    #[test]
    fn legacy_sender_retries_mismatch_and_restores_echo() {
        let mut mock = MockBoot1 {
            fail_first_block: true,
            ..Default::default()
        };
        let report = send_image(&mut mock, &image(), &test_options(), |_, _| {}).unwrap();
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
            ..test_options()
        };
        assert!(send_image(&mut mock, &image(), &options, |_, _| {}).is_err());
        assert!(mock.writes.ends_with(b"localecho on\r"));
    }

    #[test]
    fn stock_duart_only_write_error_is_indistinguishable_from_ack() {
        let mut mock = MockBoot1 {
            crc: true,
            duart_only_write_error: true,
            ..Default::default()
        };

        let report = send_image(&mut mock, &image(), &test_options(), |_, _| {}).unwrap();

        assert_eq!(report.blocks, 1);
        assert_eq!(mock.block_attempts, 1);
    }

    // Some boot1 variants may expose write failures on the REPL even though current stock boot1
    // emits them only on DUART. Preserve fail-fast handling for that stronger protocol.
    fn assert_in_band_device_write_error(crc: bool) {
        let mut mock = MockBoot1 {
            crc,
            in_band_write_error: true,
            ..Default::default()
        };
        let error = send_image(&mut mock, &image(), &test_options(), |_, _| {}).unwrap_err();

        assert!(matches!(
            error,
            SendError::DeviceWrite {
                block: 0,
                ref report
            } if report == "Write error"
        ));
        assert_eq!(mock.block_attempts, 1);
        assert!(mock.writes.ends_with(b"localecho on\r"));
    }

    #[test]
    fn legacy_in_band_write_error_precedes_valid_ack() {
        assert_in_band_device_write_error(false);
    }

    #[test]
    fn crc_in_band_write_error_precedes_valid_ack() {
        assert_in_band_device_write_error(true);
    }
}
