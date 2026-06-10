use maxio::storage::hashing::EtagMd5;

#[test]
fn etag_md5_known_vector() {
    let mut hasher = EtagMd5::new();
    hasher.update(b"hello maxio");
    assert_eq!(
        hex::encode(hasher.finalize()),
        "c3dd79c5d3cff40236ff7108f804f3ef"
    );
}
