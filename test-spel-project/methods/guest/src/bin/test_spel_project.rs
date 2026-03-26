#![no_main]

use lez_framework::prelude::*;

risc0_zkvm::guest::entry!(main);

#[lez_program]
mod test_spel_project {
    #[allow(unused_imports)]
    use super::*;

    /// Initialize the program state.
    #[instruction]
    pub fn initialize(
        #[account(init, pda = literal("state"))]
        state: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
    ) -> LezResult {
        // TODO: implement initialization logic
        Ok(LezOutput::states_only(vec![
            AccountPostState::new_claimed(state.account.clone()),
            AccountPostState::new(owner.account.clone()),
        ]))
    }

    /// Example instruction — replace with your own.
    #[instruction]
    pub fn do_something(
        #[account(mut, pda = literal("state"))]
        state: AccountWithMetadata,
        #[account(signer)]
        owner: AccountWithMetadata,
        amount: u64,
    ) -> LezResult {
        // TODO: implement your logic
        Ok(LezOutput::states_only(vec![
            AccountPostState::new(state.account.clone()),
            AccountPostState::new(owner.account.clone()),
        ]))
    }
}
