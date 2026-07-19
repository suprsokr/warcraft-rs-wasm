//! Integration test: parse vanilla M2 files with particle emitters.
//!
//! fountainparticles.m2 is a version-256 (vanilla 1.12) M2 model with 3
//! particle emitters.  It currently fails with "failed to fill whole buffer"
//! because the vanilla particle emitter struct layout (~220 bytes each) is
//! completely different from the TBC+ layout (~476 bytes) that the parser
//! expects.

use std::io::Cursor;

const TEST_FILE: &[u8] = include_bytes!("data/fountainparticles.m2");

#[test]
fn test_vanilla_particle_model_parses() {
    assert_eq!(&TEST_FILE[0..4], b"MD20");
    let version = u32::from_le_bytes(TEST_FILE[4..8].try_into().unwrap());
    assert_eq!(version, 256, "expected vanilla version 256");

    let mut cursor = Cursor::new(TEST_FILE);
    let result = wow_m2::parse_m2(&mut cursor);

    match &result {
        Ok(format) => {
            let model = match format {
                wow_m2::M2Format::Legacy(m) | wow_m2::M2Format::Chunked(m) => m,
            };
            eprintln!(
                "Parsed: {} bones, {} particles, {} texture_anims, {} transparency",
                model.bones.len(),
                model.particle_emitters.len(),
                model.texture_animations.len(),
                model.transparency_animations.len(),
            );
            assert!(model.particle_emitters.len() > 0, "should have particle emitters");
        }
        Err(e) => {
            panic!("Failed to parse vanilla particle model: {}", e);
        }
    }
}
