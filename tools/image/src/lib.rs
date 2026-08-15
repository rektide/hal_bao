// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    fmt, fs,
    io::{self, Write},
    mem::size_of,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use bao_boot1_protocol::{Uf2Image, preflight_bytes};
use bao1x_api::{
    BAREMETAL_START, JUMP_INSTRUCTION, KERNEL_START, StaticsInRom,
    signatures::{
        FunctionCode, MAGIC_NUMBER, SIGBLOCK_LEN, SIGNATURE_PQ_LENGTH, SignatureInFlash,
        UNSIGNED_LEN,
    },
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use goblin::elf::{Elf, header::EM_RISCV, program_header::PT_LOAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use xous_semver::SemVer;
use xous_tools::sign_image::{Version, convert_to_uf2, load_pem, sign_file};

pub const BAREMETAL_CODE_ORIGIN: u64 =
    (BAREMETAL_START + SIGBLOCK_LEN + size_of::<StaticsInRom>()) as u64;
pub const BAREMETAL_CODE_END: u64 = (BAREMETAL_START + 256 * 1024 - 1024) as u64;
const PQ_SIGNATURE_SIZE: u64 = 3856;
const HEADER_JUMP: u32 = 0x3000_006f;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddedVerification {
    Verified { key_slot: usize, key_tag: String },
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmbeddedKey {
    pub slot: usize,
    pub tag: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectionReport {
    pub profile: &'static str,
    pub blocks: usize,
    pub image_bytes: usize,
    pub signed_len: u32,
    pub function: &'static str,
    pub signature_mode: &'static str,
    pub version: u32,
    pub corrected_version: u32,
    pub compatible_header: bool,
    pub anti_rollback: u32,
    pub minimum_version_hex: String,
    pub image_version_hex: String,
    pub pq_enabled: bool,
    pub embedded_keys: Vec<EmbeddedKey>,
    pub embedded_verification: EmbeddedVerification,
    pub device_acceptance: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountCandidate {
    pub path: PathBuf,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct CopyOptions<'a> {
    pub image: &'a Path,
    pub target: Option<&'a Path>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyReport {
    pub target: PathBuf,
    pub destination: PathBuf,
    pub dry_run: bool,
    pub bytes: usize,
}

fn tag_string(tag: &[u8; 4]) -> String {
    String::from_utf8_lossy(tag).trim_end().to_owned()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn inspect_image(image: &Uf2Image) -> Result<InspectionReport, Box<dyn Error>> {
    let mut bytes = Vec::with_capacity(image.len() * 256);
    for block in image.iter() {
        bytes.extend_from_slice(block.payload());
    }
    if bytes.len() < SIGBLOCK_LEN {
        return Err(
            ImageError::new("signed image is shorter than the 768-byte signature block").into(),
        );
    }

    let header =
        bytemuck::pod_read_unaligned::<SignatureInFlash>(&bytes[..size_of::<SignatureInFlash>()]);
    if header._jal_instruction != HEADER_JUMP {
        return Err(ImageError::new(format!(
            "signature-block trampoline is 0x{:08x}, expected 0x{HEADER_JUMP:08x}",
            header._jal_instruction
        ))
        .into());
    }
    if header.sealed_data.magic != MAGIC_NUMBER {
        return Err(ImageError::new("signed header has invalid Baochip magic").into());
    }
    if header.sealed_data.function_code != FunctionCode::Baremetal as u32 {
        return Err(ImageError::new(format!(
            "signed header function is {}, expected baremetal ({})",
            header.sealed_data.function_code,
            FunctionCode::Baremetal as u32
        ))
        .into());
    }
    if bytes[UNSIGNED_LEN + size_of::<bao1x_api::signatures::SealedFields>()..SIGBLOCK_LEN]
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(ImageError::new("signature block has non-zero reserved padding").into());
    }
    if word_at(&bytes, SIGBLOCK_LEN) != JUMP_INSTRUCTION {
        return Err(ImageError::new("presign image has an invalid code trampoline").into());
    }

    let signed_end = UNSIGNED_LEN
        .checked_add(header.sealed_data.signed_len as usize)
        .ok_or_else(|| ImageError::new("signed length overflows"))?;
    if signed_end < SIGBLOCK_LEN || signed_end > bytes.len() {
        return Err(ImageError::new(format!(
            "signed length ends at byte {signed_end}, outside the reconstructed image"
        ))
        .into());
    }
    let pq_size = if header.sealed_data.pq_enabled == 0 {
        0
    } else {
        SIGNATURE_PQ_LENGTH
    };
    let image_bytes = signed_end
        .checked_add(pq_size)
        .ok_or_else(|| ImageError::new("signed image length overflows"))?;
    if image_bytes > bytes.len() {
        return Err(ImageError::new("PQ signature extends beyond the reconstructed image").into());
    }
    if bytes[image_bytes..].iter().any(|byte| *byte != 0) {
        return Err(ImageError::new("UF2 contains non-zero data after the signed image").into());
    }

    let signed = &bytes[UNSIGNED_LEN..signed_end];
    let signature = Signature::from_bytes(&header.signature);
    let signed_hash = Sha512::digest(signed);
    let embedded_keys = header
        .sealed_data
        .pubkeys
        .iter()
        .enumerate()
        .filter(|(_, key)| key.pk != [0; 32])
        .map(|(slot, key)| EmbeddedKey {
            slot,
            tag: tag_string(&key.tag),
        })
        .collect();
    let verification = header
        .sealed_data
        .pubkeys
        .iter()
        .enumerate()
        .find_map(|(slot, key)| {
            if key.pk == [0; 32] {
                return None;
            }
            let verifying_key = VerifyingKey::from_bytes(&key.pk).ok()?;
            let valid = if header.aad_len == 0 {
                let mut digest = Sha512::new();
                digest.update(signed);
                verifying_key
                    .verify_prehashed(digest, None, &signature)
                    .is_ok()
            } else if header.aad_len as usize <= header.aad.len() {
                let mut message = header.aad[..header.aad_len as usize].to_vec();
                message.extend_from_slice(&Sha256::digest(signed_hash));
                verifying_key.verify(&message, &signature).is_ok()
            } else {
                false
            };
            valid.then(|| EmbeddedVerification::Verified {
                key_slot: slot,
                key_tag: tag_string(&key.tag),
            })
        })
        .unwrap_or(EmbeddedVerification::Failed);

    Ok(InspectionReport {
        profile: "canonical_baochip_baremetal_uf2",
        blocks: image.len(),
        image_bytes,
        signed_len: header.sealed_data.signed_len,
        function: "baremetal",
        signature_mode: if header.aad_len == 0 {
            "ed25519ph"
        } else {
            "fido2_ed25519"
        },
        version: header.sealed_data.version,
        corrected_version: header.sealed_data.corrected_version,
        compatible_header: header.is_compatible(),
        anti_rollback: header.sealed_data.anti_rollback,
        minimum_version_hex: hex(&header.sealed_data.min_semver),
        image_version_hex: hex(&header.sealed_data.semver),
        pq_enabled: header.sealed_data.pq_enabled != 0,
        embedded_keys,
        embedded_verification: verification,
        device_acceptance: "unknown: depends on installed keys, revocations, lifecycle, anti-rollback, and PQ policy",
    })
}

fn word_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("four-byte word"),
    )
}

/// Inspect a canonical signed Baochip baremetal UF2 without predicting device acceptance.
pub fn inspect_bytes(bytes: &[u8]) -> Result<InspectionReport, Box<dyn Error>> {
    inspect_image(&preflight_bytes(bytes)?)
}

pub fn inspect_file(path: &Path) -> Result<InspectionReport, Box<dyn Error>> {
    inspect_bytes(&fs::read(path)?)
}

#[derive(Deserialize)]
struct Lsblk {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
struct LsblkDevice {
    label: Option<String>,
    #[serde(default)]
    mountpoints: Vec<Option<PathBuf>>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

fn collect_mounts(device: LsblkDevice, mounts: &mut Vec<MountCandidate>) {
    if let Some(label) = device.label
        && (label == "BAOCHIP" || label == "ALTCHIP")
    {
        mounts.extend(
            device
                .mountpoints
                .into_iter()
                .flatten()
                .map(|path| MountCandidate {
                    path,
                    label: label.clone(),
                }),
        );
    }
    for child in device.children {
        collect_mounts(child, mounts);
    }
}

/// Discover mounted Baochip boot volumes using block-device labels.
pub fn discover_mounts() -> Result<Vec<MountCandidate>, Box<dyn Error>> {
    let output = Command::new("lsblk")
        .args(["--json", "--output", "LABEL,MOUNTPOINTS"])
        .output()?;
    if !output.status.success() {
        return Err(ImageError::new(format!("lsblk failed with {}", output.status)).into());
    }
    let listing: Lsblk = serde_json::from_slice(&output.stdout)?;
    let mut mounts = Vec::new();
    for device in listing.blockdevices {
        collect_mounts(device, &mut mounts);
    }
    Ok(mounts)
}

fn select_mount(
    candidates: &[MountCandidate],
    requested: Option<&Path>,
) -> Result<PathBuf, Box<dyn Error>> {
    let selected = if let Some(requested) = requested {
        let requested = requested.canonicalize()?;
        candidates
            .iter()
            .find(|candidate| {
                candidate
                    .path
                    .canonicalize()
                    .is_ok_and(|path| path == requested)
            })
            .ok_or_else(|| {
                ImageError::new("explicit target is not a mounted BAOCHIP/ALTCHIP volume")
            })?
            .clone()
    } else {
        let safe: Vec<_> = candidates
            .iter()
            .filter(|candidate| candidate.label == "BAOCHIP")
            .collect();
        match safe.as_slice() {
            [candidate] => (*candidate).clone(),
            [] => return Err(ImageError::new("no mounted BAOCHIP volume found").into()),
            _ => {
                return Err(ImageError::new("multiple BAOCHIP volumes found; use --target").into());
            }
        }
    };
    if selected.label != "BAOCHIP" {
        return Err(ImageError::new("refusing ALTCHIP target").into());
    }
    Ok(selected.path.canonicalize()?)
}

trait CopyOps {
    fn write_temp(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn rename(&mut self, source: &Path, destination: &Path) -> io::Result<()>;
    fn sync_directory(&mut self, path: &Path) -> io::Result<()>;
}

struct FsCopyOps;

impl CopyOps for FsCopyOps {
    fn write_temp(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()
    }

    fn rename(&mut self, source: &Path, destination: &Path) -> io::Result<()> {
        Ok(rustix::fs::renameat_with(
            rustix::fs::CWD,
            source,
            rustix::fs::CWD,
            destination,
            rustix::fs::RenameFlags::NOREPLACE,
        )?)
    }

    fn sync_directory(&mut self, path: &Path) -> io::Result<()> {
        fs::File::open(path)?.sync_all()
    }
}

fn copy_with_ops(
    image_path: &Path,
    bytes: &[u8],
    candidates: &[MountCandidate],
    requested: Option<&Path>,
    dry_run: bool,
    ops: &mut impl CopyOps,
) -> Result<CopyReport, Box<dyn Error>> {
    let target = select_mount(candidates, requested)?;
    let filename = image_path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| ImageError::new("image path has no filename"))?;
    let destination = target.join(filename);
    if destination.exists() {
        return Err(
            ImageError::new(format!("refusing to overwrite {}", destination.display())).into(),
        );
    }
    if !dry_run {
        let temporary = target.join(format!(".{}.bao-image.tmp", filename.to_string_lossy()));
        if temporary.exists() {
            return Err(ImageError::new(format!(
                "temporary file already exists: {}",
                temporary.display()
            ))
            .into());
        }
        ops.write_temp(&temporary, bytes)?;
        if let Err(error) = ops.rename(&temporary, &destination) {
            let _ = fs::remove_file(&temporary);
            return Err(error.into());
        }
        ops.sync_directory(&target)?;
    }
    Ok(CopyReport {
        target,
        destination,
        dry_run,
        bytes: bytes.len(),
    })
}

/// Preflight and optionally copy an image to a positively identified BAOCHIP mount.
pub fn copy_image(options: CopyOptions<'_>) -> Result<CopyReport, Box<dyn Error>> {
    let bytes = fs::read(options.image)?;
    let inspection = inspect_bytes(&bytes)?;
    if !inspection.compatible_header {
        return Err(ImageError::new("refusing to copy: signed header is not compatible").into());
    }
    if inspection.embedded_verification == EmbeddedVerification::Failed {
        return Err(
            ImageError::new("refusing to copy: no embedded key verifies the signature").into(),
        );
    }
    let candidates = discover_mounts()?;
    copy_with_ops(
        options.image,
        &bytes,
        &candidates,
        options.target,
        options.dry_run,
        &mut FsCopyOps,
    )
}

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
    if options.input == options.output {
        return Err(ImageError::new("signed output must differ from the presign input").into());
    }
    if !matches!(
        options.output.extension().and_then(|value| value.to_str()),
        Some("img" | "bin")
    ) {
        return Err(ImageError::new("signed output must end in .img or .bin").into());
    }
    if options.pq_key.is_some() != options.pq_key_cache.is_some() {
        return Err(ImageError::new("PQ key and key cache must be supplied together").into());
    }
    if options.pq_key.is_some() {
        let signed_size =
            SIGBLOCK_LEN as u64 + fs::metadata(options.input)?.len() + PQ_SIGNATURE_SIZE;
        let slot_size = (KERNEL_START - BAREMETAL_START) as u64;
        if signed_size > slot_size {
            return Err(ImageError::new(format!(
                "PQ-signed image is {signed_size} bytes, exceeding the {slot_size}-byte baremetal slot"
            ))
            .into());
        }
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

    use ed25519_dalek::{DigestSigner, SigningKey};
    use tempfile::{NamedTempFile, TempDir};

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

    fn signed_uf2() -> Vec<u8> {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let mut header = SignatureInFlash {
            _jal_instruction: HEADER_JUMP,
            ..SignatureInFlash::default()
        };
        header.sealed_data.function_code = FunctionCode::Baremetal as u32;
        header.sealed_data.anti_rollback = 1;
        header.sealed_data.pubkeys[3].pk = signing_key.verifying_key().to_bytes();
        header.sealed_data.pubkeys[3].tag = *b"dev ";

        let mut protected = header.sealed_data.as_ref().to_vec();
        protected.resize(SIGBLOCK_LEN - UNSIGNED_LEN, 0);
        protected.extend_from_slice(&JUMP_INSTRUCTION.to_le_bytes());
        protected.extend_from_slice(b"test payload");
        header.sealed_data.signed_len = protected.len() as u32;
        protected[..size_of::<bao1x_api::signatures::SealedFields>()]
            .copy_from_slice(header.sealed_data.as_ref());
        let signature: Signature = signing_key.sign_digest(Sha512::new().chain_update(&protected));
        header.signature.copy_from_slice(&signature.to_bytes());

        let mut image = header.as_ref()[..UNSIGNED_LEN].to_vec();
        image.extend_from_slice(&protected);
        let blocks = image.len().div_ceil(256) as u32;
        let mut uf2 = Vec::with_capacity(blocks as usize * 512);
        for number in 0..blocks {
            let mut block = [0; 512];
            block[0..4].copy_from_slice(&0x0a32_4655_u32.to_le_bytes());
            block[4..8].copy_from_slice(&0x9e5d_5157_u32.to_le_bytes());
            block[8..12].copy_from_slice(&0x2000_u32.to_le_bytes());
            block[12..16].copy_from_slice(
                &(bao_boot1_protocol::BAREMETAL_START + number * 256).to_le_bytes(),
            );
            block[16..20].copy_from_slice(&256_u32.to_le_bytes());
            block[20..24].copy_from_slice(&number.to_le_bytes());
            block[24..28].copy_from_slice(&blocks.to_le_bytes());
            block[28..32].copy_from_slice(&bao_boot1_protocol::BAOCHIP_FAMILY_ID.to_le_bytes());
            let start = number as usize * 256;
            let end = (start + 256).min(image.len());
            block[32..32 + end - start].copy_from_slice(&image[start..end]);
            block[508..512].copy_from_slice(&0x0ab1_6f30_u32.to_le_bytes());
            uf2.extend_from_slice(&block);
        }
        uf2
    }

    #[derive(Default)]
    struct RecordingOps {
        events: Vec<&'static str>,
    }

    impl CopyOps for RecordingOps {
        fn write_temp(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            self.events.push("write_sync");
            FsCopyOps.write_temp(path, bytes)
        }

        fn rename(&mut self, source: &Path, destination: &Path) -> io::Result<()> {
            self.events.push("rename");
            FsCopyOps.rename(source, destination)
        }

        fn sync_directory(&mut self, path: &Path) -> io::Result<()> {
            self.events.push("sync_directory");
            FsCopyOps.sync_directory(path)
        }
    }

    fn candidate(directory: &TempDir, label: &str) -> MountCandidate {
        MountCandidate {
            path: directory.path().to_path_buf(),
            label: label.to_owned(),
        }
    }

    #[test]
    fn inspects_canonical_signed_uf2() {
        let report = inspect_bytes(&signed_uf2()).unwrap();
        assert_eq!(report.profile, "canonical_baochip_baremetal_uf2");
        assert_eq!(
            report.embedded_verification,
            EmbeddedVerification::Verified {
                key_slot: 3,
                key_tag: "dev".to_owned()
            }
        );
        assert!(report.device_acceptance.starts_with("unknown"));
    }

    #[test]
    fn rejects_malformed_uf2_before_signed_header_parsing() {
        let mut uf2 = signed_uf2();
        uf2[0] ^= 1;
        assert!(
            inspect_bytes(&uf2)
                .unwrap_err()
                .to_string()
                .contains("magic")
        );
    }

    #[test]
    fn refuses_altchip_even_when_explicit() {
        let mount = TempDir::new().unwrap();
        let error = select_mount(&[candidate(&mount, "ALTCHIP")], Some(mount.path())).unwrap_err();
        assert!(error.to_string().contains("ALTCHIP"));
    }

    #[test]
    fn refuses_ambiguous_baochip_mounts() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let error = select_mount(
            &[candidate(&first, "BAOCHIP"), candidate(&second, "BAOCHIP")],
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("multiple BAOCHIP"));
    }

    #[test]
    fn dry_run_does_not_open_target_files() {
        let mount = TempDir::new().unwrap();
        let mut ops = RecordingOps::default();
        let report = copy_with_ops(
            Path::new("firmware.uf2"),
            b"validated",
            &[candidate(&mount, "BAOCHIP")],
            None,
            true,
            &mut ops,
        )
        .unwrap();
        assert!(report.dry_run);
        assert!(ops.events.is_empty());
        assert!(!report.destination.exists());
    }

    #[test]
    fn copy_writes_then_renames_then_syncs_directory() {
        let mount = TempDir::new().unwrap();
        let mut ops = RecordingOps::default();
        let report = copy_with_ops(
            Path::new("firmware.uf2"),
            b"validated",
            &[candidate(&mount, "BAOCHIP")],
            None,
            false,
            &mut ops,
        )
        .unwrap();
        assert_eq!(ops.events, ["write_sync", "rename", "sync_directory"]);
        assert_eq!(fs::read(report.destination).unwrap(), b"validated");
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

    #[test]
    fn rejects_signing_in_place() {
        let path = Path::new("image.bin");
        let error = sign_image(SignOptions {
            input: path,
            output: path,
            key: Path::new("key.pem"),
            min_xous_version: "v0.9.8-790",
            git_describe: "v0.10.2-0-g00000000",
            pq_key: None,
            pq_key_cache: None,
        })
        .unwrap_err();

        assert!(error.to_string().contains("must differ"));
    }

    #[test]
    fn rejects_pq_signature_that_exceeds_slot() {
        let input = NamedTempFile::new().unwrap();
        input
            .as_file()
            .set_len((KERNEL_START - BAREMETAL_START - SIGBLOCK_LEN) as u64)
            .unwrap();
        let output = input.path().with_extension("img");
        let error = sign_image(SignOptions {
            input: input.path(),
            output: &output,
            key: Path::new("key.pem"),
            min_xous_version: "v0.9.8-790",
            git_describe: "v0.10.2-0-g00000000",
            pq_key: Some(Path::new("pq.key")),
            pq_key_cache: Some(Path::new("pq.cache")),
        })
        .unwrap_err();

        assert!(error.to_string().contains("PQ-signed image"));
    }
}
