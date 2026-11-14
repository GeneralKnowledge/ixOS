//! Only for the iOS size check: pulls the same stack into a static library.
#[no_mangle]
pub extern "C" fn relay_spike_start() -> u8 {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let ep = iroh::Endpoint::builder(iroh::endpoint::presets::Minimal)
            .relay_mode(iroh::RelayMode::Disabled)
            .bind()
            .await
            .unwrap();
        let store = iroh_blobs::store::mem::MemStore::new();
        let _b = iroh_blobs::BlobsProtocol::new(&store, None);
        let _ = spake2::Spake2::<spake2::Ed25519Group>::start_symmetric(
            &spake2::Password::new(b"x"),
            &spake2::Identity::new(b"y"),
        );
        ep.id().as_bytes()[0]
    })
}
