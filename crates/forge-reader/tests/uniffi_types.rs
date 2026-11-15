//! `reader_types!()` compiles against the real types (the field lists must
//! match them exactly), and they cross the boundary.

uniffi::setup_scaffolding!("forge_reader_check");
forge_reader::reader_types!();

#[uniffi::export]
fn first_block(section: ReaderSection) -> Option<ReaderBlock> {
    section.blocks.into_iter().next()
}

#[uniffi::export]
fn anchor_of(section: ReaderSection, location: ReaderLocation) -> Option<ReaderAnchor> {
    ReaderAnchor::at(&section, location)
}

#[test]
fn the_types_cross_the_boundary() {
    let section = ReaderSection {
        id: "s".into(),
        title: "T".into(),
        blocks: forge_reader::blocks_from_html(
            "<p>Hi</p><img src='https://example.com/a.png'>",
            &mut forge_reader::WebResources::new(None),
        ),
    };
    assert_eq!(first_block(section.clone()).unwrap().text(), "Hi");
    let anchor = anchor_of(
        section,
        ReaderLocation {
            block: 1,
            offset: 0.0,
        },
    )
    .unwrap();
    assert_eq!(anchor.opening, "");
}
