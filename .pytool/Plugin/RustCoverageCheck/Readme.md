# Rust Coverage Check

This CI plugin runs all unit tests with coverage enabled, calculating coverage results on a per package basis. It filters results to only calculate coverage on files within the package.

This CI plugin will also calculate coverage for the entire workspace.

## Plugin Customizations

As a default, this plugin requires 75% coverage, though this can be configured within a packages ci.yaml file by adding the entry `RustCoverageCheck`. The required coverage percent can also be customized on a per (rust) package bases.

### Example ci settings

``` yaml
"RustCoverageCheck": {
    "Default": 1,
    "PackageOverrides": {
        "DxeRust": 0.0,
        "UefiEventLib": 0.0,
    }
}
```
