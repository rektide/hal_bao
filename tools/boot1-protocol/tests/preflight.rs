// SPDX-License-Identifier: Apache-2.0

use bao_boot1_protocol::{
    BAOCHIP_FAMILY_ID, BAREMETAL_END, BAREMETAL_START, BLOCK_SIZE, PreflightError, preflight_bytes,
};

const MAGIC_START0: u32 = 0x0a32_4655;
const MAGIC_START1: u32 = 0x9e5d_5157;
const MAGIC_END: u32 = 0x0ab1_6f30;
const FAMILY_ID_PRESENT: u32 = 0x0000_2000;
const PAYLOAD_SIZE: u32 = 256;

fn set_word(block: &mut [u8], offset: usize, value: u32) {
    block[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn canonical_block(number: u32, total: u32, address: u32, payload_byte: u8) -> [u8; BLOCK_SIZE] {
    let mut block = [0_u8; BLOCK_SIZE];
    block[32..32 + PAYLOAD_SIZE as usize].fill(payload_byte);
    for (offset, value) in [
        (0, MAGIC_START0),
        (4, MAGIC_START1),
        (8, FAMILY_ID_PRESENT),
        (12, address),
        (16, PAYLOAD_SIZE),
        (20, number),
        (24, total),
        (28, BAOCHIP_FAMILY_ID),
        (508, MAGIC_END),
    ] {
        set_word(&mut block, offset, value);
    }
    block
}

fn canonical_image(blocks: u32) -> Vec<u8> {
    let mut image = Vec::with_capacity(blocks as usize * BLOCK_SIZE);
    for number in 0..blocks {
        image.extend_from_slice(&canonical_block(
            number,
            blocks,
            BAREMETAL_START + number * PAYLOAD_SIZE,
            number as u8,
        ));
    }
    image
}

#[test]
fn rejects_empty_and_partial_input_with_exact_sizes() {
    assert_eq!(
        preflight_bytes(&[]),
        Err(PreflightError::InvalidSize { actual: 0 })
    );

    let partial = vec![0_u8; BLOCK_SIZE - 1];
    assert_eq!(
        preflight_bytes(&partial),
        Err(PreflightError::InvalidSize {
            actual: BLOCK_SIZE - 1,
        })
    );
}

#[test]
fn rejects_each_invalid_magic_field() {
    for offset in [0, 4, 508] {
        let mut block = canonical_block(0, 1, BAREMETAL_START, 0);
        set_word(&mut block, offset, 0);

        assert_eq!(
            preflight_bytes(&block),
            Err(PreflightError::InvalidMagic { block: 0 }),
            "magic field at offset {offset} was accepted"
        );
    }
}

#[test]
fn rejects_noncanonical_flags_with_exact_values() {
    let mut block = canonical_block(0, 1, BAREMETAL_START, 0);
    let actual = FAMILY_ID_PRESENT | 1;
    set_word(&mut block, 8, actual);

    assert_eq!(
        preflight_bytes(&block),
        Err(PreflightError::NonCanonicalFlags {
            block: 0,
            actual,
            expected: FAMILY_ID_PRESENT,
        })
    );
}

#[test]
fn rejects_wrong_family_with_exact_values() {
    let mut block = canonical_block(0, 1, BAREMETAL_START, 0);
    let actual = BAOCHIP_FAMILY_ID ^ 1;
    set_word(&mut block, 28, actual);

    assert_eq!(
        preflight_bytes(&block),
        Err(PreflightError::WrongFamily {
            block: 0,
            actual,
            expected: BAOCHIP_FAMILY_ID,
        })
    );
}

#[test]
fn rejects_wrong_payload_size_with_exact_value() {
    let mut block = canonical_block(0, 1, BAREMETAL_START, 0);
    set_word(&mut block, 16, PAYLOAD_SIZE - 1);

    assert_eq!(
        preflight_bytes(&block),
        Err(PreflightError::WrongPayloadSize {
            block: 0,
            actual: PAYLOAD_SIZE - 1,
        })
    );
}

#[test]
fn rejects_wrong_block_number_and_total_with_exact_values() {
    let mut wrong_number = canonical_block(1, 1, BAREMETAL_START, 0);
    assert_eq!(
        preflight_bytes(&wrong_number),
        Err(PreflightError::WrongNumbering {
            block: 0,
            number: 1,
            total: 1,
            expected_total: 1,
        })
    );

    set_word(&mut wrong_number, 20, 0);
    set_word(&mut wrong_number, 24, 2);
    assert_eq!(
        preflight_bytes(&wrong_number),
        Err(PreflightError::WrongNumbering {
            block: 0,
            number: 0,
            total: 2,
            expected_total: 1,
        })
    );
}

#[test]
fn rejects_noncontiguous_addresses_with_exact_values() {
    let mut image = canonical_image(2);
    let actual = BAREMETAL_START + 2 * PAYLOAD_SIZE;
    set_word(&mut image[BLOCK_SIZE..2 * BLOCK_SIZE], 12, actual);

    assert_eq!(
        preflight_bytes(&image),
        Err(PreflightError::NonContiguousAddress {
            block: 1,
            actual,
            expected: BAREMETAL_START + PAYLOAD_SIZE,
        })
    );
}

#[test]
fn overflow_candidate_is_rejected_before_address_arithmetic() {
    let mut block = canonical_block(0, 1, BAREMETAL_START, 0);
    set_word(&mut block, 12, u32::MAX);

    assert_eq!(
        preflight_bytes(&block),
        Err(PreflightError::NonContiguousAddress {
            block: 0,
            actual: u32::MAX,
            expected: BAREMETAL_START,
        })
    );
}

#[test]
fn rejects_first_block_overrunning_baremetal_slot() {
    let blocks_in_slot = (BAREMETAL_END - BAREMETAL_START) / PAYLOAD_SIZE;
    let total = blocks_in_slot + 1;
    let image = canonical_image(total);

    assert_eq!(
        preflight_bytes(&image),
        Err(PreflightError::OutsideBaremetalSlot {
            block: blocks_in_slot as usize,
            start: BAREMETAL_END,
            end: BAREMETAL_END + PAYLOAD_SIZE,
        })
    );
}

#[test]
fn accepts_multiblock_image_and_exposes_validated_blocks() {
    let raw_blocks = [
        canonical_block(0, 3, BAREMETAL_START, 0x11),
        canonical_block(1, 3, BAREMETAL_START + PAYLOAD_SIZE, 0x22),
        canonical_block(2, 3, BAREMETAL_START + 2 * PAYLOAD_SIZE, 0x33),
    ];
    let bytes: Vec<_> = raw_blocks.iter().flatten().copied().collect();

    let image = preflight_bytes(&bytes).expect("canonical image should pass preflight");
    assert_eq!(image.len(), raw_blocks.len());
    assert!(!image.is_empty());
    assert_eq!(image.blocks().len(), raw_blocks.len());
    assert_eq!(image.iter().len(), raw_blocks.len());

    for (index, block) in image.iter().enumerate() {
        assert_eq!(block.number(), index as u32);
        assert_eq!(
            block.address(),
            BAREMETAL_START + index as u32 * PAYLOAD_SIZE
        );
        assert_eq!(block.payload_size(), PAYLOAD_SIZE);
        assert_eq!(block.bytes(), &raw_blocks[index]);
        assert_eq!(block.payload(), &raw_blocks[index][32..32 + 256]);
    }
}
