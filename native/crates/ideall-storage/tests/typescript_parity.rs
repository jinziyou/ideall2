use ideall_storage::{ArchiveLimits, parse_workspace_archive};

#[test]
fn imports_archive_emitted_by_the_typescript_v2_exporter() {
    let raw = include_str!("fixtures/workspace-v2-typescript.json");
    let archive = parse_workspace_archive(raw, ArchiveLimits::default()).unwrap();

    assert_eq!(archive.manifest.checksum, "2c8a7d50");
    assert_eq!(archive.nodes[0].base().title, "TS fixture");
    assert_eq!(archive.blobs[0].data, b"abc");
}
