//! Transaction building and submission.

use std::collections::HashMap;
use std::fs;
use std::process;
use nssa::program::Program;
use nssa::public_transaction::{Message, WitnessSet};
use nssa::{AccountId, PublicTransaction};
use nssa_core::program::ProgramId;
use spel_framework_core::idl::{IdlSeed, SpelIdl, IdlInstruction};
use crate::hex::{hex_encode, decode_bytes_32, parse_account_id};
use crate::parse::{parse_value, ParsedValue};
use crate::serialize::serialize_to_risc0;
use crate::pda::compute_pda_from_seeds;
use crate::cli::{snake_to_kebab, to_pascal_case};
use wallet::WalletCore;
use nssa_core::NullifierSecretKey as _NullifierSecretKey;
use risc0_zkvm::{ExecutorEnv, default_prover};
// IdlPreTxInputSource removed — source is now a plain string


/// Read one output field from the journal bytes at the given offset.
/// Returns (hex_encoded_value, bytes_consumed).
fn read_journal_field(journal: &[u8], offset: usize, name: &str) -> (String, usize) {
    if name == "receipt" {
        // Receipt = entire journal
        let remaining = &journal[offset..];
        (::hex::encode(remaining), remaining.len())
    } else {
        // Default: 32-byte field (nullifier, hash, etc.)
        if offset + 32 > journal.len() {
            eprintln!("❌ Journal too short to read '{}' at offset {}", name, offset);
            process::exit(1);
        }
        (::hex::encode(&journal[offset..offset+32]), 32)
    }
}

/// Execute a pre_tx hook: resolve inputs from wallet + args, run guest ELF, inject outputs into args.
async fn run_pre_tx_hook(
    hook: &spel_framework_core::idl::IdlPreTxHook,
    args: &mut HashMap<String, String>,
    wallet_core: &WalletCore,
) {
    println!("🔐 Running pre_tx hook (elf: {})...", hook.elf);

    // Resolve NSK for the caller account
    let caller_key = snake_to_kebab(&hook.signer_arg);
    let caller_str = args.get(&caller_key).unwrap_or_else(|| {
        eprintln!("❌ pre_tx hook requires --{} <Private/xxx>", caller_key);
        process::exit(1);
    }).clone();

    let id_str = caller_str.trim_start_matches("Private/");
    let account_id: nssa::AccountId = id_str.parse().unwrap_or_else(|e| {
        eprintln!("❌ Invalid account ID '{}': {}", caller_str, e);
        process::exit(1);
    });
    let nsk = wallet_core
        .get_account_nullifier_secret_key(account_id)
        .unwrap_or_else(|| {
            eprintln!("❌ Account '{}' not found in wallet keystore", caller_str);
            process::exit(1);
        });

    // Build a JSON object with all inputs for the guest to deserialize as a struct
    let mut env_builder = ExecutorEnv::builder();
    let mut json_map = serde_json::Map::new();

    for input in &hook.inputs {
        let value = if input.source == "wallet_nsk" {
            serde_json::json!(nsk.to_vec())
        } else {
            let raw_val = if let Some(arg_name) = input.source.strip_prefix("arg:") {
                let key = snake_to_kebab(arg_name);
                args.get(&key).unwrap_or_else(|| {
                    eprintln!("❌ pre_tx input '{}' requires --{}", input.name, key);
                    process::exit(1);
                }).clone()
            } else if let Some(literal) = input.source.strip_prefix("literal:") {
                literal.to_string()
            } else {
                eprintln!("❌ Unknown source '{}' for input '{}'", input.source, input.name);
                process::exit(1);
            };
            match input.type_.as_str() {
                "bytes32" => {
                    let bytes = ::hex::decode(&raw_val).unwrap_or_else(|e| {
                        eprintln!("❌ Invalid hex for bytes32 '{}': {}", raw_val, e);
                        process::exit(1);
                    });
                    serde_json::json!(bytes)
                }
                "u64" => {
                    let v: u64 = raw_val.parse().unwrap_or_else(|e| {
                        eprintln!("❌ Invalid u64 '{}': {}", raw_val, e);
                        process::exit(1);
                    });
                    serde_json::json!(v)
                }
                "string" => {
                    serde_json::json!(raw_val)
                }
                "vec_bytes32" => {
                    let items: Vec<Vec<u8>> = raw_val.split(',')
                        .map(|s| {
                            ::hex::decode(s.trim()).unwrap_or_else(|e| {
                                eprintln!("❌ Invalid hex in vec_bytes32 '{}': {}", s, e);
                                process::exit(1);
                            })
                        })
                        .collect();
                    serde_json::json!(items)
                }
                other => {
                    eprintln!("❌ Unknown pre_tx input type '{}' — supported: bytes32, u64, string, vec_bytes32", other);
                    process::exit(1);
                }
            }
        };
        json_map.insert(input.name.clone(), value);
    }

    let json_obj = serde_json::Value::Object(json_map);
    env_builder.write(&json_obj).expect("failed to write pre_tx inputs");

    let env = env_builder.build().unwrap_or_else(|e| {
        eprintln!("❌ Failed to build executor env: {}", e);
        process::exit(1);
    });

    // Load and execute the guest ELF
    let elf_path = std::env::var("SPEL_GUEST_ELF").unwrap_or_else(|_| hook.elf.clone());
    let elf_bytes = fs::read(&elf_path).unwrap_or_else(|e| {
        eprintln!("❌ Failed to read guest ELF '{}': {}", elf_path, e);
        eprintln!("   Set SPEL_GUEST_ELF to override path");
        process::exit(1);
    });

    println!("  Proving...");
    let prove_info = default_prover().prove(env, &elf_bytes).unwrap_or_else(|e| {
        eprintln!("❌ Proof generation failed: {}", e);
        process::exit(1);
    });

    let journal_bytes = prove_info.receipt.journal.bytes.clone();

    // Extract outputs from journal in order
    let mut offset = 0;
    for output_name in &hook.outputs {
        let (val_hex, consumed) = read_journal_field(&journal_bytes, offset, output_name);
        println!("  {} → {}...{}", output_name,
            &val_hex[..8.min(val_hex.len())],
            &val_hex[val_hex.len().saturating_sub(8)..]);
        args.insert(output_name.clone(), val_hex);
        offset += consumed;
    }

    println!("✅ pre_tx hook complete");
}

/// Execute an instruction: parse args, build TX, optionally submit.
pub async fn execute_instruction(
    idl: &SpelIdl,
    ix: &IdlInstruction,
    args: &HashMap<String, String>,
    program_path: &str,
    program_id_hex: Option<&str>,
    dry_run: bool,
    extra_bins: &HashMap<String, String>,
) {
    println!("📋 Instruction: {}", ix.name);
    println!();

    let mut args = args.clone();

    // Execute pre_tx hook if present (ZK proof generation before building tx)
    if let Some(hook) = &ix.pre_tx {
        let wallet_core = WalletCore::from_env().unwrap_or_else(|e| {
            eprintln!("❌ Failed to initialize wallet for pre_tx hook: {:?}", e);
            process::exit(1);
        });
        run_pre_tx_hook(hook, &mut args, &wallet_core).await;
    }

    // Auto-fill program-id args from binary paths
    for (key, bin_path) in extra_bins {
        if !args.contains_key(key) {
            if let Ok(bytes) = fs::read(bin_path) {
                if let Ok(program) = Program::new(bytes) {
                    let id = program.id();
                    let id_str: Vec<String> = id.iter().map(|w| w.to_string()).collect();
                    let val = id_str.join(",");
                    println!("  ℹ️  Auto-filled --{} from {}", key, bin_path);
                    args.insert(key.clone(), val);
                }
            }
        }
    }

    // Validate required args
    let mut missing = vec![];
    for arg in &ix.args {
        let key = snake_to_kebab(&arg.name);
        if !args.contains_key(&key) {
            missing.push(format!("--{}", key));
        }
    }
    for acc in &ix.accounts {
        // rest accounts are variadic (0 or more) — never required
        if acc.pda.is_none() && !acc.rest {
            let key = snake_to_kebab(&acc.name);
            if !args.contains_key(&key) {
                missing.push(format!("--{}", key));
            }
        }
    }
    if !missing.is_empty() {
        eprintln!("❌ Missing required arguments: {}", missing.join(", "));
        process::exit(1);
    }

    // Parse instruction args
    let mut parsed_args: Vec<(&str, &spel_framework_core::idl::IdlType, ParsedValue)> = Vec::new();
    let mut has_errors = false;
    for arg in &ix.args {
        let key = snake_to_kebab(&arg.name);
        let raw = args.get(&key).unwrap();
        match parse_value(raw, &arg.type_) {
            Ok(val) => parsed_args.push((&arg.name, &arg.type_, val)),
            Err(e) => { eprintln!("❌ --{}: {}", key, e); has_errors = true; }
        }
    }

    // Parse non-PDA account IDs
    let mut parsed_accounts: Vec<(&str, Vec<u8>, bool)> = Vec::new();
    // rest accounts are variadic: each expands to 0 or more AccountIds
    let mut rest_accounts: Vec<(&str, Vec<(Vec<u8>, bool)>)> = Vec::new();
    for acc in &ix.accounts {
        if acc.pda.is_some() { continue; }
        if acc.rest { let key = snake_to_kebab(&acc.name); if !args.contains_key(&key) { continue; } }
        let key = snake_to_kebab(&acc.name);
        if acc.rest {
            // variadic: optional, comma-separated list of account IDs (0 entries is valid)
            let entries: Vec<(Vec<u8>, bool)> = if let Some(raw) = args.get(&key) {
                raw.split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        match parse_account_id(s) {
                            Ok((bytes, is_priv)) => (bytes.to_vec(), is_priv),
                            Err(e) => { eprintln!("❌ --{}: {}", key, e); has_errors = true; (vec![], false) }
                        }
                    })
                    .collect()
            } else {
                vec![] // rest accounts are optional — 0 is valid
            };
            rest_accounts.push((&acc.name, entries));
        } else {
            let raw = args.get(&key).unwrap();
            match parse_account_id(raw) {
                Ok((bytes, is_priv)) => parsed_accounts.push((&acc.name, bytes.to_vec(), is_priv)),
                Err(e) => { eprintln!("❌ --{}: {}", key, e); has_errors = true; }
            }
        }
    }
    if has_errors { process::exit(1); }

    // Build risc0 serialized data
    let ix_index = idl.instructions.iter().position(|i| i.name == ix.name).unwrap_or(0);
    let risc0_args: Vec<_> = parsed_args.iter().map(|(_, ty, val)| (*ty, val)).collect();
    let instruction_data = serialize_to_risc0(ix_index as u32, &risc0_args);

    // Display
    println!("Accounts:");
    for acc in &ix.accounts {
        if acc.pda.is_some() {
            println!("  📦 {} → auto-computed (PDA)", acc.name);
        } else if acc.rest {
            if let Some((_, entries)) = rest_accounts.iter().find(|(n, _)| *n == acc.name) {
                if entries.is_empty() {
                    println!("  📦 {} → (none — variadic rest)", acc.name);
                } else {
                    for (e, _) in entries {
                        println!("  📦 {} → 0x{}", acc.name, hex_encode(e));
                    }
                }
            }
        } else {
            let account_bytes = parsed_accounts.iter().find(|(n, _, _)| *n == acc.name).unwrap();
            println!("  📦 {} → 0x{}", acc.name, hex_encode(&account_bytes.1));
        }
    }
    println!();
    println!("Arguments (parsed):");
    for (name, _, val) in &parsed_args {
        println!("  {} = {}", name, val);
    }
    println!();
    println!("🔧 Transaction:");
    if let Some(pid) = program_id_hex {
        println!("  program-id: {}", pid);
    } else {
        println!("  program: {}", program_path);
    }
    println!("  instruction index: {}", ix_index);
    println!("  instruction: {} {{", to_pascal_case(&ix.name));
    for (name, _, val) in &parsed_args {
        println!("    {}: {},", name, val);
    }
    println!("  }}");
    println!();
    println!("  Serialized instruction data ({} u32 words):", instruction_data.len());
    let hex_words: Vec<String> = instruction_data.iter().map(|w| format!("{:08x}", w)).collect();
    println!("    [{}]", hex_words.join(", "));
    println!();

    if dry_run {
        println!("⚠️  Dry run — omit --dry-run to submit the transaction.");
        return;
    }

    // ─── Transaction submission ──────────────────────────────────
    println!("📤 Submitting transaction...");

    // Resolve program_id: from --program-id hex flag, or by loading the binary
    let (program_id, program_obj): (ProgramId, Option<Program>) = if let Some(hex) = program_id_hex {
        let bytes = decode_bytes_32(hex).unwrap_or_else(|e| {
            eprintln!("❌ Invalid --program-id '{}': {}", hex, e);
            process::exit(1);
        });
        let mut pid = [0u32; 8];
        for (i, chunk) in bytes.chunks(4).enumerate() {
            pid[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        (pid, None)
    } else {
        let program_bytecode = fs::read(program_path).unwrap_or_else(|e| {
            eprintln!("❌ Failed to read program binary '{}': {}", program_path, e);
            eprintln!("   Hint: pass --program-id <hex> to skip loading the binary");
            process::exit(1);
        });
        let program = Program::new(program_bytecode).unwrap_or_else(|e| {
            eprintln!("❌ Failed to load program: {:?}", e);
            process::exit(1);
        });
        let pid = program.id();
        (pid, Some(program))
    };
    println!("  Program ID: {:?}", program_id);

    // Build account map for PDA resolution
    let mut account_map: HashMap<String, AccountId> = HashMap::new();
    for (name, bytes, _) in &parsed_accounts {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        account_map.insert(name.to_string(), AccountId::new(arr));
    }
    // Note: rest accounts are variadic; store first entry (if any) for PDA seed resolution
    for (name, entries) in &rest_accounts {
        if let Some((first, _)) = entries.first() {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(first);
            account_map.insert(name.to_string(), AccountId::new(arr));
        }
    }

    // Resolve external account references needed by PDA seeds
    for acc in &ix.accounts {
        if let Some(pda) = &acc.pda {
            for seed in &pda.seeds {
                if let IdlSeed::Account { path } = seed {
                    if !account_map.contains_key(path) {
                        let key = snake_to_kebab(path);
                        if let Some(raw) = args.get(&key) {
                            match decode_bytes_32(raw) {
                                Ok(bytes) => {
                                    println!("  ℹ️  Using --{} for PDA seed '{}'", key, path);
                                    account_map.insert(path.clone(), AccountId::new(bytes));
                                }
                                Err(e) => { eprintln!("❌ --{}: {}", key, e); process::exit(1); }
                            }
                        } else {
                            eprintln!("❌ PDA '{}' requires account '{}' — provide --{}", acc.name, path, key);
                            process::exit(1);
                        }
                    }
                }
            }
        }
    }

    let mut parsed_arg_map: HashMap<String, ParsedValue> = HashMap::new();
    for (name, _, val) in &parsed_args {
        parsed_arg_map.insert(name.to_string(), val.clone());
    }

    // Resolve PDA accounts
    for acc in &ix.accounts {
        if let Some(pda) = &acc.pda {
            match compute_pda_from_seeds(&pda.seeds, &program_id, &account_map, &parsed_arg_map) {
                Ok(id) => {
                    println!("  PDA {} → {}", acc.name, id);
                    account_map.insert(acc.name.clone(), id);
                }
                Err(e) => {
                    eprintln!("❌ Failed to compute PDA for '{}': {}", acc.name, e);
                    process::exit(1);
                }
            }
        }
    }

    let wallet_core = WalletCore::from_env().unwrap_or_else(|e| {
        eprintln!("❌ Failed to initialize wallet: {:?}", e);
        eprintln!("   Set NSSA_WALLET_HOME_DIR environment variable");
        process::exit(1);
    });

    // Check if any account has a Private/ prefix
    let has_private = parsed_accounts.iter().any(|(_, _, is_priv)| *is_priv)
        || rest_accounts.iter().any(|(_, entries)| entries.iter().any(|(_, is_priv)| *is_priv));

    if has_private {
        // ─── Privacy-preserving transaction ──────────────────
        use wallet::PrivacyPreservingAccount;
        use nssa::privacy_preserving_transaction::circuit::ProgramWithDependencies;

        let program = program_obj.unwrap_or_else(|| {
            eprintln!("❌ Privacy-preserving transactions require the program binary (not --program-id)");
            process::exit(1);
        });

        // Build dependencies from extra_bins
        let mut dependencies = HashMap::new();
        for (_, bin_path) in extra_bins {
            if let Ok(bytes) = fs::read(bin_path) {
                if let Ok(dep_program) = Program::new(bytes) {
                    dependencies.insert(dep_program.id(), dep_program);
                }
            }
        }
        let program_with_deps = ProgramWithDependencies::new(program, dependencies);

        // Build privacy-preserving account list
        let mut pp_accounts: Vec<PrivacyPreservingAccount> = Vec::new();
        for acc in &ix.accounts {
            if acc.rest {
                if let Some((_, entries)) = rest_accounts.iter().find(|(n, _)| *n == acc.name) {
                    for (bytes, is_priv) in entries {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(bytes);
                        let account_id = AccountId::new(arr);
                        if *is_priv {
                            pp_accounts.push(PrivacyPreservingAccount::PrivateOwned(account_id));
                        } else {
                            pp_accounts.push(PrivacyPreservingAccount::Public(account_id));
                        }
                    }
                }
            } else if let Some((_, _, is_priv)) = parsed_accounts.iter().find(|(n, _, _)| *n == acc.name) {
                let id = *account_map.get(&acc.name).unwrap_or_else(|| {
                    eprintln!("❌ Account '{}' not resolved", acc.name);
                    process::exit(1);
                });
                if *is_priv {
                    pp_accounts.push(PrivacyPreservingAccount::PrivateOwned(id));
                } else {
                    pp_accounts.push(PrivacyPreservingAccount::Public(id));
                }
            } else {
                // PDA account — always public
                let id = *account_map.get(&acc.name).unwrap_or_else(|| {
                    eprintln!("❌ Account '{}' not resolved", acc.name);
                    process::exit(1);
                });
                pp_accounts.push(PrivacyPreservingAccount::Public(id));
            }
        }

        let (response, _shared_secrets) = wallet_core.send_privacy_preserving_tx(
            pp_accounts,
            instruction_data,
            &program_with_deps,
        ).await.unwrap_or_else(|e| {
            eprintln!("❌ Failed to submit privacy-preserving transaction: {:?}", e);
            process::exit(1);
        });

        println!("📤 Privacy-preserving transaction submitted!");
        println!("   tx_hash: {}", response.tx_hash);
        println!("   Waiting for confirmation...");

        let poller = wallet::poller::TxPoller::new(
            wallet_core.config().clone(),
            wallet_core.sequencer_client.clone(),
        );

        match poller.poll_tx(response.tx_hash).await {
            Ok(_) => println!("✅ Transaction confirmed — included in a block."),
            Err(e) => {
                eprintln!("❌ Transaction NOT confirmed: {e:#}");
                process::exit(1);
            }
        }
    } else {
        // ─── Public transaction (existing path) ──────────────
        let mut account_ids: Vec<AccountId> = Vec::new();
        for acc in &ix.accounts {
            if acc.rest {
                if let Some((_, entries)) = rest_accounts.iter().find(|(n, _)| *n == acc.name) {
                    for (bytes, _) in entries {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(bytes);
                        account_ids.push(AccountId::new(arr));
                    }
                }
            } else {
                let id = account_map.get(&acc.name).unwrap_or_else(|| {
                    eprintln!("❌ Account '{}' not resolved", acc.name);
                    process::exit(1);
                });
                account_ids.push(*id);
            }
        }

        let signer_accounts: Vec<AccountId> = ix.accounts.iter()
            .filter(|a| a.signer)
            .map(|a| *account_map.get(&a.name).unwrap())
            .collect();

        let nonces = if signer_accounts.is_empty() {
            vec![]
        } else {
            wallet_core.get_accounts_nonces(signer_accounts.clone()).await.unwrap_or_else(|e| {
                eprintln!("❌ Failed to fetch nonces: {:?}", e);
                process::exit(1);
            })
        };

        let signing_keys: Vec<_> = signer_accounts.iter().map(|id| {
            wallet_core.storage().user_data.get_pub_account_signing_key(*id).unwrap_or_else(|| {
                eprintln!("❌ Signing key not found for account {}", id);
                process::exit(1);
            })
        }).collect();

        let message = Message::new_preserialized(program_id, account_ids, nonces, instruction_data);
        let witness_set = WitnessSet::for_message(&message, &signing_keys);
        let tx = PublicTransaction::new(message, witness_set);

        let response = wallet_core.sequencer_client.send_tx_public(tx).await.unwrap_or_else(|e| {
            eprintln!("❌ Failed to submit transaction: {:?}", e);
            process::exit(1);
        });

        println!("📤 Transaction submitted!");
        println!("   tx_hash: {}", response.tx_hash);
        println!("   Waiting for confirmation...");

        let poller = wallet::poller::TxPoller::new(
            wallet_core.config().clone(),
            wallet_core.sequencer_client.clone(),
        );

        match poller.poll_tx(response.tx_hash).await {
            Ok(_) => println!("✅ Transaction confirmed — included in a block."),
            Err(e) => {
                eprintln!("❌ Transaction NOT confirmed: {e:#}");
                process::exit(1);
            }
        }
    }
}
