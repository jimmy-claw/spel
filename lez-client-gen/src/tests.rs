//! Tests for lez-client-gen.

use crate::generate_from_idl_json;

/// Sample IDL similar to what the lez-framework macro generates.
const SAMPLE_IDL: &str = r#"{
    "version": "0.1.0",
    "name": "my_multisig",
    "instructions": [
        {
            "name": "create",
            "accounts": [
                {
                    "name": "multisig_state",
                    "writable": true,
                    "signer": false,
                    "init": true,
                    "pda": {
                        "seeds": [
                            {"kind": "const", "value": "multisig_state__"},
                            {"kind": "arg", "path": "create_key"}
                        ]
                    }
                },
                {
                    "name": "creator",
                    "writable": false,
                    "signer": true,
                    "init": false
                }
            ],
            "args": [
                {"name": "create_key", "type": "[u8; 32]"},
                {"name": "threshold", "type": "u64"},
                {"name": "members", "type": {"vec": "[u8; 32]"}}
            ]
        },
        {
            "name": "approve",
            "accounts": [
                {
                    "name": "multisig_state",
                    "writable": false,
                    "signer": false,
                    "init": false,
                    "pda": {
                        "seeds": [
                            {"kind": "const", "value": "multisig_state__"}
                        ]
                    }
                },
                {
                    "name": "proposal",
                    "writable": true,
                    "signer": false,
                    "init": false
                },
                {
                    "name": "member",
                    "writable": false,
                    "signer": true,
                    "init": false
                }
            ],
            "args": [
                {"name": "proposal_id", "type": "u64"}
            ]
        }
    ],
    "accounts": [],
    "types": [],
    "errors": []
}"#;

#[test]
fn test_parse_and_generate() {
    let output = generate_from_idl_json(SAMPLE_IDL).expect("codegen should succeed");

    // Client code checks
    assert!(output.client_code.contains("pub enum MyMultisigInstruction"));
    assert!(output.client_code.contains("Create {"));
    assert!(output.client_code.contains("Approve {"));
    assert!(output.client_code.contains("pub struct CreateAccounts"));
    assert!(output.client_code.contains("pub struct ApproveAccounts"));
    assert!(output.client_code.contains("pub struct MyMultisigClient"));
    assert!(output.client_code.contains("async fn create("));
    assert!(output.client_code.contains("async fn approve("));

    // PDA computation lives in the client
    assert!(output.client_code.contains("compute_multisig_state_pda"));

    // Correct endianness — in client's parse_program_id_hex
    assert!(output.client_code.contains("from_le_bytes"));
}

#[test]
fn test_ffi_generation() {
    let output = generate_from_idl_json(SAMPLE_IDL).expect("codegen should succeed");

    // FFI function names
    assert!(output.ffi_code.contains("pub extern \"C\" fn my_multisig_create("));
    assert!(output.ffi_code.contains("pub extern \"C\" fn my_multisig_approve("));
    assert!(output.ffi_code.contains("pub extern \"C\" fn my_multisig_free_string("));
    assert!(output.ffi_code.contains("pub extern \"C\" fn my_multisig_version("));

    // AccountId parsing helper emitted in FFI
    assert!(output.ffi_code.contains("parse_account_id"));

    // FFI is self-contained (inline transaction building, no super::client import)
    assert!(!output.ffi_code.contains("use super::client::*"));

    // FFI emits full WalletCore transaction building
    assert!(output.ffi_code.contains("use wallet::WalletCore"));
    assert!(output.ffi_code.contains("tokio::runtime::Runtime::new"));
    assert!(output.ffi_code.contains("rt.block_on"));
    assert!(output.ffi_code.contains("send_tx_public"));

    // FFI returns tx_hash JSON
    assert!(output.ffi_code.contains("tx_hash"));
}

#[test]
fn test_header_generation() {
    let output = generate_from_idl_json(SAMPLE_IDL).expect("codegen should succeed");

    assert!(output.header.contains("MY_MULTISIG_FFI_H"));
    assert!(output.header.contains("char* my_multisig_create(const char* args_json)"));
    assert!(output.header.contains("char* my_multisig_approve(const char* args_json)"));
    assert!(output.header.contains("void my_multisig_free_string(char* s)"));
}

#[test]
fn test_account_order_in_client() {
    let output = generate_from_idl_json(SAMPLE_IDL).expect("codegen should succeed");

    // Account ordering is now enforced in the client (accounts struct + account_ids vec).
    // For approve: the IDL order is multisig_state, proposal, member.
    let client = &output.client_code;
    let approve_struct_start = client.find("pub struct ApproveAccounts").unwrap();
    let approve_section = &client[approve_struct_start..];

    let ms_pos = approve_section.find("multisig_state").unwrap();
    let prop_pos = approve_section.find("proposal").unwrap();
    let member_pos = approve_section.find("member").unwrap();

    assert!(ms_pos < prop_pos, "multisig_state should come before proposal in ApproveAccounts");
    assert!(prop_pos < member_pos, "proposal should come before member in ApproveAccounts");
}

#[test]
fn test_ffi_calls_client_methods() {
    let output = generate_from_idl_json(SAMPLE_IDL).expect("codegen should succeed");

    // The FFI impl builds instruction enum and submits transaction inline
    let ffi = &output.ffi_code;
    assert!(ffi.contains("Message::try_new"), "FFI should build Message");
    assert!(ffi.contains("send_tx_public"), "FFI should submit transaction");
    assert!(ffi.contains("MyMultisigInstruction"), "FFI should reference instruction enum");
}

#[test]
fn test_invalid_json_error() {
    let result = generate_from_idl_json("not json");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("failed to parse IDL JSON"));
}

#[test]
fn test_empty_instructions() {
    let idl = r#"{
        "version": "0.1.0",
        "name": "empty_program",
        "instructions": []
    }"#;
    let output = generate_from_idl_json(idl).expect("should handle empty instructions");
    assert!(output.client_code.contains("EmptyProgramInstruction"));
    assert!(output.ffi_code.contains("empty_program_free_string"));
}

#[test]
fn test_rest_accounts() {
    let idl = r#"{
        "version": "0.1.0",
        "name": "test_prog",
        "instructions": [{
            "name": "multi_sign",
            "accounts": [
                {"name": "state", "writable": true, "signer": false, "init": false},
                {"name": "signers", "writable": false, "signer": true, "init": false, "rest": true}
            ],
            "args": []
        }],
        "accounts": [],
        "types": [],
        "errors": []
    }"#;
    let output = generate_from_idl_json(idl).expect("should handle rest accounts");
    assert!(output.client_code.contains("pub signers: Vec<AccountId>"));
    // FFI should handle rest accounts as optional array, defaulting to empty
    assert!(output.ffi_code.contains("signers"));
}
