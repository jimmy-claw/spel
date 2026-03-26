/// Generate IDL JSON for the test-spel-e2e program.
///
/// Usage:
///   cargo run --bin generate_idl > test-spel-e2e-idl.json

lez_framework::generate_idl!("../methods/guest/src/bin/test_spel_e2e.rs");
