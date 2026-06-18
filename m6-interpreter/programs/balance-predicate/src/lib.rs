//! M6 conformance target: balance >= threshold check.
//!
//! This is the same logical predicate proved in Experiment 2 and M0-M5,
//! now written as a normal Solana program so it compiles to real sBPF
//! bytecode via the standard toolchain. The M6 interpreter must execute
//! this bytecode correctly.
//!
//! Input (via instruction data):
//!   bytes [0..8]  — balance: u64
//!   bytes [8..16] — threshold: u64
//!
//! Output (via return code):
//!   0 = balance >= threshold (predicate true)
//!   1 = balance < threshold  (predicate false)

use solana_program::{
    account_info::AccountInfo, entrypoint, entrypoint::ProgramResult,
    pubkey::Pubkey, program_error::ProgramError,
};

entrypoint!(process_instruction);

pub fn process_instruction(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if instruction_data.len() < 16 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let balance   = u64::from_le_bytes(instruction_data[0..8].try_into().unwrap());
    let threshold = u64::from_le_bytes(instruction_data[8..16].try_into().unwrap());

    if balance >= threshold {
        Ok(())
    } else {
        Err(ProgramError::Custom(1))
    }
}
