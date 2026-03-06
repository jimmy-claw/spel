# SPEL — Smart Program Engine for Logos

[![CI](https://github.com/logos-co/spel/actions/workflows/ci.yml/badge.svg)](https://github.com/logos-co/spel/actions/workflows/ci.yml)

> **SPEL** = **S**mart **P**rogram **E**ngine for **L**ogos
>
> The name captures what this is: a framework layer that makes writing smart programs for the Logos Execution Zone (LEZ) feel like writing normal Rust — with macros handling the boilerplate, a CLI doing the heavy lifting, and zkVM proving the execution.

Developer framework for building LEZ programs — inspired by [Anchor](https://www.anchor-lang.com/) for Solana.

Write your program logic with proc macros. Get IDL generation, a full typed CLI with TX submission, and project scaffolding for free.

## The Stack

```
Your Program (Rust)
  └── #[lez_program] macro        ← annotate instructions
       ├── IDL generation          ← types + accounts → JSON schema
       ├── zkVM guest binary       ← runs on-chain (risc0)
       └── lez-cli                 ← auto-generated typed CLI
```

## Quick Start

### Scaffold a new project

```bash
cargo install --path lez-cli
lez-cli init my-program
cd my-program
```

This generates a complete project:

```
my-program/
├── Cargo.toml
├── Makefile                   # build, idl, cli, deploy, inspect, setup
├── my_program_core/           # Shared types (guest + host)
├── methods/guest/             # RISC Zero guest (runs on-chain)
└── examples/src/bin/
    ├── generate_idl.rs        # One-liner IDL generator
    └── my_program_cli.rs      # Three-line CLI wrapper
```

### Build → Deploy → Transact

```bash
make build        # Build the guest binary (risc0)
make idl          # Generate IDL from #[lez_program] annotations
make deploy       # Deploy to sequencer
make cli ARGS="--help"
```

## Writing Programs

```rust
#[lez_program]
mod my_program {
    #[instruction]
    pub fn initialize(ctx: Context<Initialize>, owner: AccountId) -> ProgramResult {
        ctx.accounts.state.owner = owner;
        Ok(())
    }
}
```

The macro emits the IDL. The CLI reads the IDL. You write logic.

## Repos in the SPEL ecosystem

| Repo | Description |
|------|-------------|
| [spel](https://github.com/logos-co/spel) | This repo — framework, macros, CLI |
| [spelbook](https://github.com/logos-co/spelbook) | On-chain program registry (SPELbook) |
| [lez-multisig-framework](https://github.com/logos-co/lez-multisig-framework) | Multisig governance program — full demo |
| [lmao](https://github.com/jimmy-claw/lmao) | Logos Module for Agent Orchestration (A2A over Waku) |

## v0.1.0

Tagged [v0.1.0](https://github.com/logos-co/spel/releases/tag/v0.1.0) — full end-to-end demo passing with lez-multisig-framework (deploy → registry → multisig governance → token ChainedCall).
