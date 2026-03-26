# test-spel-project

A LEZ program built with [lez-framework](https://github.com/logos-co/spel).

## Prerequisites

- Rust + [risc0 toolchain](https://dev.risczero.com/api/zkvm/install)
- [LSSA wallet CLI](https://github.com/logos-blockchain/lssa) (`wallet` binary)
- A running sequencer

## Quick Start

```bash
# 1. Build the guest binary
make build

# 2. Generate the IDL (auto-extracts from #[lez_program] annotations)
make idl

# 3. Deploy to sequencer
make deploy

# 4. See available commands (auto-generated from your program)
make cli ARGS="--help"

# 5. Run an instruction
make cli ARGS="-p methods/guest/target/riscv32im-risc0-zkvm-elf/docker/test_spel_project.bin \\
  <command> --arg1 value1 --arg2 value2"

# Dry run (no submission):
make cli ARGS="--dry-run -p methods/guest/target/riscv32im-risc0-zkvm-elf/docker/test_spel_project.bin \\
  <command> --arg1 value1"
```

## Make Targets

| Target | Description |
|--------|-------------|
| `make build` | Build the guest binary (risc0) |
| `make idl` | Generate IDL JSON from program source |
| `make cli ARGS="..."` | Run the IDL-driven CLI |
| `make deploy` | Deploy program to sequencer |
| `make inspect` | Show ProgramId for built binary |
| `make setup` | Create accounts via wallet |
| `make status` | Show saved state and binary info |
| `make clean` | Remove saved state |

## Project Structure

```
test-spel-project/
├── test_spel_project_core/    # Shared types (used by guest + host)
│   └── src/lib.rs
├── methods/
│   └── guest/            # RISC Zero guest program (runs on-chain)
│       └── src/bin/test_spel_project.rs
├── examples/             # CLI tools
│   └── src/bin/
│       ├── generate_idl.rs    # One-liner IDL generator
│       └── test_spel_project_cli.rs # Three-line CLI wrapper
├── Makefile
└── test-spel-project-idl.json       # Auto-generated IDL
```

## How It Works

The `#[lez_program]` macro in your guest binary defines your on-chain program.
The framework automatically:

1. **Generates an `Instruction` enum** from your function signatures
2. **Generates an IDL** (Interface Description Language) describing your program
3. **Provides a full CLI** for building, inspecting, and submitting transactions

You write the program logic. The framework handles the rest.
