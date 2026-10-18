use std::fs;
use std::path::Path;

#[test]
fn test_all_13_dist_packs_load_successfully() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dist_packs_dir = manifest_dir.join("../../dist/packs");
    if !dist_packs_dir.exists() {
        return;
    }

    let temp_dir = tempfile::tempdir().expect("creating temp dir");

    let entries = fs::read_dir(&dist_packs_dir).expect("reading dist/packs");
    let mut count = 0;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("gz") {
            let file_name = path.file_name().unwrap().to_string_lossy();
            let pack_id = file_name.trim_end_matches(".tar.gz");
            let unpack_dir = temp_dir.path().join(pack_id);
            fs::create_dir_all(&unpack_dir).expect("creating unpack dir");

            let file = fs::File::open(&path).expect("opening pack archive");
            let gz = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(gz);
            archive.unpack(&unpack_dir).expect("unpacking tar.gz");

            let bank = clack_core::decoder::load_pack(&unpack_dir, 48_000)
                .unwrap_or_else(|e| panic!("failed to load pack {}: {:?}", pack_id, e));

            // Verify the pack has valid clips
            assert!(
                !bank.press_default.is_empty() || bank.press.iter().any(|p| p.is_some()),
                "pack {} has no playable sounds",
                pack_id
            );
            count += 1;
        }
    }

    assert_eq!(
        count, 13,
        "expected all 13 packs to be present and verified"
    );
}
