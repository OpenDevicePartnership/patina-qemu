# MSFT RUST

## General

Any of the components we use such as `rust-src` are installed by default when building with `msrustup`
(automatically used on ADO) if we ever decide we need to use a different component and it fails on ADO due to a missing
component we will need to ask the msrustup team to bring in that component

---

## Environment Variable(S) Breakdown

In order to comply with the **ES** requirement, "that all Microsoft teams will *eventually* be required to use the
ES-provided Rust toolchain and crates.io mirror in their products and services", the following environment variables
 were introduced.

Explicitly, `stuart_build` will build with the environment variable(s):

* `RUSTC_BOOTSTRAP`
  * When set to `1` breaks the stability guarantees of the accompanied branch, which is currently required to build
UefiRust on a stable branch.
  * Eventually the goal is to only rely on the stable branch
