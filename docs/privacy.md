# Privacy-Preserving Programs with SPEL

SPEL programs are **privacy-agnostic** — the same program code works identically with both public and private accounts. Privacy is handled at the transaction layer, not the program layer.

## How LEZ Privacy Works

LEZ uses a commitment/nullifier scheme:

- **Private accounts** are owned by the `auth-transfer` program and encrypted on-chain
- **Commitments** hide account state in a Merkle tree
- **Nullifiers** prove an account was spent without revealing which one
- **ZK proofs** (RISC0) verify execution correctness without revealing private data

The sequencer never sees plaintext private account state — only commitments, nullifiers, and ZK proofs.

## Using Private Accounts with SPEL

### 1. Create a private account

```bash
wallet account new private
# → Private/5jH7h9CfRDcbfZxCs7h93PcuL1ESW5EJxWbntBup2tJ8

wallet auth-transfer init --account-id Private/<id>
wallet account sync-private
```

### 2. Call any SPEL instruction with a private account

Simply pass the `Private/` prefixed account ID — `spel` detects it automatically and builds a `PrivacyPreservingTransaction`:

```bash
spel --idl my-program-idl.json -p my-program.bin \
  my_instruction \
  --owner Private/5jH7h9Cf...
```

That's it. The program logic doesn't change.

### 3. Verify the data was written

```bash
wallet account sync-private
wallet account get --account-id Private/<id>
# → {"balance": 0, "data_b64": "SGVsbG8h", ...}
```

The `data_b64` field contains the base64-encoded private data, decrypted by your wallet.

## What the Sequencer Sees

For a `PrivacyPreservingTransaction`:

| Field | Value |
|-------|-------|
| Account states | Encrypted ciphertext |
| New commitments | Merkle tree insertions |
| Spent nullifiers | Prevents replay |
| ZK proof | RISC0 receipt |

The sequencer verifies the proof but never sees plaintext account data.

## Privacy Transaction Types

| Account prefix | Transaction type | ZK proof |
|---------------|-----------------|----------|
| `Public/` | `PublicTransaction` | Signature |
| `Private/` | `PrivacyPreservingTransaction` | RISC0 receipt |
| Mixed | `PrivacyPreservingTransaction` | RISC0 receipt |

## Writing Privacy-Compatible SPEL Programs

No special annotations needed. A simple program works with both:

```rust
#[lez_program]
mod my_program {
    #[instruction]
    pub fn store_data(
        #[account(mut)]
        target: AccountWithMetadata,   // works as Public/ or Private/
        data: Vec<u8>,
    ) -> LezResult {
        let mut account = target.account.clone();
        account.data = data.try_into()?;
        Ok(LezOutput::states_only(vec![
            AccountPostState::new(account),
        ]))
    }
}
```

## Private Account Lifecycle

```
wallet account new private          # create keypair, derive NPK/NSK
wallet auth-transfer init           # register commitment on-chain
wallet account sync-private         # sync Merkle tree state
spel ... --account Private/<id>     # use in any SPEL instruction
wallet account sync-private         # sync updated state
wallet account get --account-id ... # read decrypted data
```

## IDL Privacy Metadata (optional)

You can mark accounts as intended for private use in the IDL:

```rust
#[instruction(execution = { private_owned: true })]
pub fn private_only_instruction(...) -> LezResult
```

This is informational — it signals to tooling that this instruction expects private accounts. The program logic remains the same.

## ZK-Aware Instructions: #[pre_tx_hook]

Some instructions need client-side ZK proof generation before the transaction is submitted — for example, proving you are a member of a multisig without revealing your identity.

SPEL's `#[pre_tx_hook]` attribute automates this pattern:

```rust
#[lez_program]
mod multisig {
    #[instruction]
    #[pre_tx_hook(signer = caller, method = vote_prove, outputs = [receipt, nullifier])]
    pub fn propose(ctx: Context, proposal_index: u64, ...) -> LezResult {
        // env::verify(receipt)?; — auto-injected by macro
        // custom logic only
    }
}
```

**What the macro does:**

1. Generates a `PreTxInput` struct with all instruction args + the caller's account ID
2. Adds `pre_tx` metadata to the IDL for this instruction
3. Auto-injects `env::verify(receipt)?;` as the first line of the function body

**What the CLI does:**

When `spel-cli` sees `pre_tx` in the IDL for an instruction:
1. Reads `--caller Private/xxx` from CLI args
2. Looks up the caller's NSK from the wallet keystore (no manual NSK entry needed)
3. Runs `vote_prove(PreTxInput { ... })` to generate the ZK receipt
4. Attaches `receipt` and `nullifier` to the transaction

**Developer experience:** Write only the business logic. SPEL handles NSK lookup, proof generation, and `env::verify()` automatically.

**Usage:**

```bash
# Create private accounts for each member
wallet account new private  # Alice
wallet account new private  # Bob
wallet account new private  # Carol

# Use --caller to specify voter identity (NSK never exposed)
spel --idl multisig.json propose \
  --caller Private/Alice_id \
  --multisig 3HLvtc4k... \
  --proposal-index 1
```

See [SPEL issue #85](https://github.com/logos-co/spel/issues/85) for full design details.

## Related

- [LEZ Privacy Technical Deep Dive](lez/lez-privacy-technical-deep-dive.md)
- [Private Multisig (LP-0002)](lez/lp-0002-rfc.md)
- [SPEL PR #83](https://github.com/logos-co/spel/pull/83) — `Private/` prefix auto-detection
