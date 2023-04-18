use fv_lib::*;
// Integration tests are black box tests.
// No access to private data / functions

#[test]
fn trivial_integration_test() {
    assert_eq!(FfsFileType::EfiFvFileTypeAll, FfsFileType::EfiFvFileTypeAll);
}
