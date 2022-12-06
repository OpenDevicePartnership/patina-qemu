# Cargo Test Host Check

A CiBuildPlugin that finds all Rust packages (toml files) in a UEFI package. If
a toml file has a library, it will attempt to compile and run all host based
tests present for that library. Note that even something such as a DXE_DRIVER
could have a library; it only needs to meet one of these requirements:

1. A `[[lib]]` section in the cargo.toml file for the Rust package
2. A lib.rs file

## Compilation requirements

This check will attempt to compile and execute (on the host system) any tests
associated with the library. The developer must ensure that the library remains
target agnostic and can be compiled with the host machine as a target, rather then
i386-unknown-uefi or x86_64-unknown-uefi. Any architecture specific
implementations should either be moved out of the library, or conditionally
compiled out using `#[cfg(target_os="uefi")]` or `#[cfg_attr(target_os="uefi", DEC_TO_SET)]`.

There are two types of tests that can be executed on the host:

### Integration Tests

Integration tests are black box tests that do not have access to the private
members of any module inside the library. They are located in the tests folder
as seen below:

```cmd
    |-cargo.toml
    |-src/
    |-tests/
```

Integration tests only need to be decorated with `#[test]` to be executed. To
test the library, it must be imported as if you were importing any other crate
as seen in the full example below:

```rust
use fv_lib::*;

#[test]
fn trivial_integration_test() {
    assert_eq!(FfsFileType::EfiFvFileTypeAll FfsFileType::EfiFvFileTypeAll);
}
```

### Unit Tests

Unit tests are tests written within the library itself and have access to
private methods and data. While not necessary, any tests or test modules should
have the conditional compilation decorator for the test profile as seen in the
full example below:

```rust
#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn trivial_unit_test() {
        assert_eq!(EFI_FV_FILETYPE_ALL, 0x00);
    }
}
```
