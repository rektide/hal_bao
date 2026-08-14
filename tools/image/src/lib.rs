// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fmt, fs,
    mem::size_of,
    path::{Path, PathBuf},
    str::FromStr,
};

use bao1x_api::{BAREMETAL_START, JUMP_INSTRUCTION, StaticsInRom, signatures::SIGBLOCK_LEN};
use goblin::elf::{Elf, header::EM_RISCV, program_header::PT_LOAD};
use xous_semver::SemVer;
use xous_tools::sign_image::{Version, convert_to_uf2, load_pem, sign_file};

pub const BAREMETAL_CODE_ORIGIN: u64 =
    (BAREMETAL_START + SIGBLOCK_LEN + size_of::<StaticsInRom>()) as u64;
pub const BAREMETAL_CODE_END: u64 = (BAREMETAL_START + 256 * 1024 - 1024) as u64;

#[derive(Debug)]
pub struct ImageError(String);

impl ImageError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ImageError {}

#[derive(Debug)]
struct LoadSegment {
    offset: usize,
    physical_address: u64,
    file_size: usize,
}

pub fn pack_elf(input: &Path, output: &Path) -> Result<usize, Box<dyn Error>> {
    let data = fs::read(input)?;
    let elf = Elf::parse(&data)?;

    if elf.header.e_machine != EM_RISCV {
        return Err(ImageError::new(format!(
            "ELF machine is {}, expected RISC-V ({EM_RISCV})",
            elf.header.e_machine
        ))
        .into());
    }
    if !elf.is_64 && !elf.little_endian {
        return Err(ImageError::new("only little-endian ELF32 images are supported").into());
    }
    if elf.is_64 {
        return Err(ImageError::new("only ELF32 images are supported").into());
    }
    if elf.entry != BAREMETAL_CODE_ORIGIN {
        return Err(ImageError::new(format!(
            "ELF entry is 0x{:08x}, expected 0x{BAREMETAL_CODE_ORIGIN:08x}",
            elf.entry
        ))
        .into());
    }

    let mut segments = Vec::new();
    for (index, header) in elf.program_headers.iter().enumerate() {
        if header.p_type != PT_LOAD || header.p_filesz == 0 {
            continue;
        }
        if header.p_filesz > header.p_memsz {
            return Err(ImageError::new(format!(
                "PT_LOAD {index} has p_filesz greater than p_memsz"
            ))
            .into());
        }

        let offset = usize::try_from(header.p_offset)?;
        let file_size = usize::try_from(header.p_filesz)?;
        let file_end = offset
            .checked_add(file_size)
            .ok_or_else(|| ImageError::new(format!("PT_LOAD {index} file range overflows")))?;
        if file_end > data.len() {
            return Err(ImageError::new(format!("PT_LOAD {index} extends beyond the file")).into());
        }
        segments.push(LoadSegment {
            offset,
            physical_address: header.p_paddr,
            file_size,
        });
    }

    if segments.is_empty() {
        return Err(ImageError::new("ELF has no file-backed PT_LOAD segments").into());
    }
    segments.sort_by_key(|segment| segment.physical_address);
    if segments[0].physical_address != BAREMETAL_CODE_ORIGIN {
        return Err(ImageError::new(format!(
            "first PT_LOAD LMA is 0x{:08x}, expected 0x{BAREMETAL_CODE_ORIGIN:08x}",
            segments[0].physical_address
        ))
        .into());
    }

    let mut previous_end = BAREMETAL_CODE_ORIGIN;
    for segment in &segments {
        let end = segment
            .physical_address
            .checked_add(segment.file_size as u64)
            .ok_or_else(|| ImageError::new("PT_LOAD address range overflows"))?;
        if segment.physical_address < BAREMETAL_CODE_ORIGIN || end > BAREMETAL_CODE_END {
            return Err(ImageError::new(format!(
                "PT_LOAD LMA range 0x{:08x}..0x{end:08x} is outside the baremetal code slot \
                 0x{BAREMETAL_CODE_ORIGIN:08x}..0x{BAREMETAL_CODE_END:08x}; initialized RAM \
                 sections must use a ROM load address",
                segment.physical_address
            ))
            .into());
        }
        if segment.physical_address < previous_end {
            return Err(ImageError::new(format!(
                "overlapping PT_LOAD segments at LMA 0x{:08x}",
                segment.physical_address
            ))
            .into());
        }
        previous_end = end;
    }

    let program_end = segments
        .iter()
        .map(|segment| segment.physical_address + segment.file_size as u64)
        .max()
        .expect("segments is not empty");
    let program_size = usize::try_from(program_end - BAREMETAL_CODE_ORIGIN)?;
    let mut program = vec![0; program_size];
    for segment in segments {
        let destination = usize::try_from(segment.physical_address - BAREMETAL_CODE_ORIGIN)?;
        program[destination..destination + segment.file_size]
            .copy_from_slice(&data[segment.offset..segment.offset + segment.file_size]);
    }

    let mut packed = Vec::with_capacity(size_of::<StaticsInRom>() + program.len());
    packed.extend_from_slice(&JUMP_INSTRUCTION.to_le_bytes());
    packed.resize(size_of::<StaticsInRom>(), 0);
    packed.extend_from_slice(&program);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, packed)?;
    Ok(program_size)
}

pub struct SignOptions<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub key: &'a Path,
    pub min_xous_version: &'a str,
    pub git_describe: &'a str,
    pub pq_key: Option<&'a Path>,
    pub pq_key_cache: Option<&'a Path>,
}

pub fn sign_image(options: SignOptions<'_>) -> Result<PathBuf, Box<dyn Error>> {
    if !matches!(
        options.output.extension().and_then(|value| value.to_str()),
        Some("img" | "bin")
    ) {
        return Err(ImageError::new("signed output must end in .img or .bin").into());
    }
    if options.pq_key.is_some() != options.pq_key_cache.is_some() {
        return Err(ImageError::new("PQ key and key cache must be supplied together").into());
    }

    let key_path = options
        .key
        .to_str()
        .ok_or_else(|| ImageError::new("signing key path is not valid UTF-8"))?;
    let private_key = load_pem(key_path)?;
    let minimum_version = Some(
        SemVer::from_str(options.min_xous_version)
            .map_err(|error| ImageError::new(format!("invalid minimum Xous version: {error}")))?,
    );
    let image_version: [u8; 16] = SemVer::from_str(options.git_describe)
        .map_err(|error| ImageError::new(format!("invalid image version: {error}")))?
        .into();
    let pq_keys = options
        .pq_key
        .zip(options.pq_key_cache)
        .map(|(key, cache)| (key.to_path_buf(), Some(cache.to_path_buf())));

    sign_file(
        &options.input,
        &options.output,
        &private_key,
        false,
        &minimum_version,
        Some(image_version),
        Version::Bao1xV1,
        true,
        SIGBLOCK_LEN,
        Some("baremetal"),
        Some(1),
        false,
        pq_keys,
    )?;

    let uf2 = options.output.with_extension("uf2");
    convert_to_uf2(&options.output, &uf2, Some("baremetal"), None)?;
    Ok(uf2)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::NamedTempFile;

    use super::*;

    fn elf32(segments: &[(u32, u32, &[u8])], entry: u32) -> Vec<u8> {
        const HEADER_SIZE: usize = 52;
        const PROGRAM_HEADER_SIZE: usize = 32;

        let data_offset = HEADER_SIZE + PROGRAM_HEADER_SIZE * segments.len();
        let mut elf = vec![0; data_offset];
        let mut headers = Vec::new();

        for (virtual_address, physical_address, bytes) in segments {
            let offset = elf.len() as u32;
            elf.extend_from_slice(bytes);
            headers.extend_from_slice(&1_u32.to_le_bytes());
            headers.extend_from_slice(&offset.to_le_bytes());
            headers.extend_from_slice(&virtual_address.to_le_bytes());
            headers.extend_from_slice(&physical_address.to_le_bytes());
            headers.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            headers.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            headers.extend_from_slice(&5_u32.to_le_bytes());
            headers.extend_from_slice(&4_u32.to_le_bytes());
        }

        elf[0..4].copy_from_slice(b"\x7fELF");
        elf[4..7].copy_from_slice(&[1, 1, 1]);
        elf[16..18].copy_from_slice(&2_u16.to_le_bytes());
        elf[18..20].copy_from_slice(&EM_RISCV.to_le_bytes());
        elf[20..24].copy_from_slice(&1_u32.to_le_bytes());
        elf[24..28].copy_from_slice(&entry.to_le_bytes());
        elf[28..32].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        elf[40..42].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        elf[42..44].copy_from_slice(&(PROGRAM_HEADER_SIZE as u16).to_le_bytes());
        elf[44..46].copy_from_slice(&(segments.len() as u16).to_le_bytes());
        elf[HEADER_SIZE..data_offset].copy_from_slice(&headers);
        elf
    }

    fn pack(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut input = NamedTempFile::new()?;
        input.write_all(bytes)?;
        let output = NamedTempFile::new()?;
        pack_elf(input.path(), output.path())?;
        Ok(fs::read(output.path())?)
    }

    #[test]
    fn preserves_initialized_data_at_its_rom_load_address() {
        let elf = elf32(
            &[
                (
                    BAREMETAL_CODE_ORIGIN as u32,
                    BAREMETAL_CODE_ORIGIN as u32,
                    b"TEXT",
                ),
                (0x6100_0000, BAREMETAL_CODE_ORIGIN as u32 + 0x20, b"DATA"),
            ],
            BAREMETAL_CODE_ORIGIN as u32,
        );
        let packed = pack(&elf).unwrap();

        assert_eq!(&packed[..4], &JUMP_INSTRUCTION.to_le_bytes());
        assert_eq!(&packed[256..260], b"TEXT");
        assert_eq!(&packed[260..288], &[0; 28]);
        assert_eq!(&packed[288..292], b"DATA");
    }

    #[test]
    fn rejects_wrong_entry_address() {
        let elf = elf32(
            &[(
                BAREMETAL_CODE_ORIGIN as u32,
                BAREMETAL_CODE_ORIGIN as u32,
                b"TEXT",
            )],
            BAREMETAL_CODE_ORIGIN as u32 + 4,
        );
        assert!(pack(&elf).unwrap_err().to_string().contains("ELF entry"));
    }

    #[test]
    fn rejects_overlapping_load_images() {
        let elf = elf32(
            &[
                (
                    BAREMETAL_CODE_ORIGIN as u32,
                    BAREMETAL_CODE_ORIGIN as u32,
                    b"12345678",
                ),
                (0x6100_0000, BAREMETAL_CODE_ORIGIN as u32 + 4, b"DATA"),
            ],
            BAREMETAL_CODE_ORIGIN as u32,
        );
        assert!(pack(&elf).unwrap_err().to_string().contains("overlapping"));
    }

    #[test]
    fn rejects_file_backed_ram_without_rom_load_address() {
        let elf = elf32(
            &[
                (
                    BAREMETAL_CODE_ORIGIN as u32,
                    BAREMETAL_CODE_ORIGIN as u32,
                    b"TEXT",
                ),
                (0x6100_0000, 0x6100_0000, b"DATA"),
            ],
            BAREMETAL_CODE_ORIGIN as u32,
        );
        assert!(
            pack(&elf)
                .unwrap_err()
                .to_string()
                .contains("initialized RAM sections")
        );
    }

    #[test]
    fn rejects_slot_overflow() {
        let elf = elf32(
            &[
                (
                    BAREMETAL_CODE_ORIGIN as u32,
                    BAREMETAL_CODE_ORIGIN as u32,
                    b"TEXT",
                ),
                (0x6100_0000, BAREMETAL_CODE_END as u32 - 2, b"DATA"),
            ],
            BAREMETAL_CODE_ORIGIN as u32,
        );
        assert!(
            pack(&elf)
                .unwrap_err()
                .to_string()
                .contains("outside the baremetal code slot")
        );
    }
}
