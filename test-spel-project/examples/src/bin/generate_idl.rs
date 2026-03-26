/// Generate IDL JSON for the test-spel-project program.
///
/// Usage:
///   cargo run --bin generate_idl > test-spel-project-idl.json

lez_framework::generate_idl!("../methods/guest/src/bin/test_spel_project.rs");
