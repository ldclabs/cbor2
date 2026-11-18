//! Real consumer crates catch expansion failures hidden by this crate's
//! dev-dependencies and test-only imports.

#[test]
fn derive_consumer_fixtures() {
    use std::{path::Path, process::Command};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = root.join("tests/derive-fixtures/Cargo.toml");
    // Cargo omits nested packages from the published crate archive.
    if !manifest.is_file() && root.join("Cargo.toml.orig").is_file() {
        return;
    }
    let target = root.join("target/derive-fixtures");
    for (binary, expected_error) in [
        ("pass", None),
        ("bounds", None),
        ("renamed", None),
        ("array_skip", Some("array field positions")),
        ("tuple_skip", Some("array field positions")),
        (
            "enum_key_conflict",
            Some("consistently across enum variants"),
        ),
        ("enum_untagged", Some("untagged variants are not supported")),
        ("struct_tag", Some("on a struct conflicts with")),
        ("array_tag", Some("on a struct conflicts with")),
    ] {
        let consumer_manifest = if binary == "renamed" {
            root.join("tests/derive-fixtures/renamed/Cargo.toml")
        } else {
            manifest.clone()
        };
        let output = Command::new(env!("CARGO"))
            .args([
                if expected_error.is_none() {
                    "run"
                } else {
                    "check"
                },
                "--offline",
                "--manifest-path",
            ])
            .arg(&consumer_manifest)
            .arg("--target-dir")
            .arg(&target)
            .args(["--bin", binary])
            .output()
            .expect("cargo is available to run consumer fixtures");
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        if let Some(expected) = expected_error {
            assert!(
                !output.status.success() && diagnostic.contains(expected),
                "{binary}: {diagnostic}"
            );
        } else {
            assert!(output.status.success(), "{binary}: {diagnostic}");
        }
    }

    let output = Command::new(env!("CARGO"))
        .args([
            "check",
            "--offline",
            "--lib",
            "--no-default-features",
            "--manifest-path",
        ])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target)
        .output()
        .expect("cargo is available to check no_std consumers");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
