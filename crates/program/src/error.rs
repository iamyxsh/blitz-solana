use solana_program::program_error::ProgramError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SlashError {
    BadInstruction = 0,
    BadAccountData,
    WrongOwner,
    WrongPda,
    NotSigner,
    /// The evidence did not travel with an ed25519 verification instruction.
    NoEd25519Instruction = 10,
    /// The verified signature does not belong to the registered signing key.
    UnregisteredKey,
    /// An offset entry points at a different instruction, so what the ed25519
    /// program actually verified is not what this program is reading.
    OffsetsNotSelfContained,
    /// Exactly two signatures are required, over two whole receipts.
    WrongSignatureCount,
    MalformedReceipt,
    /// The two receipts belong to different runs of the log.
    MixedLogs = 20,
    /// The two receipts sit at different positions, so they do not contradict.
    DifferentSequence,
    /// The two receipts are byte-identical.
    NotContradictory,
    /// This exact contradiction has already been convicted.
    AlreadyConvicted,
    InsufficientBond = 30,
    NothingStaked,
    Overdraw,
    BondLocked,
}

impl From<SlashError> for ProgramError {
    fn from(error: SlashError) -> Self {
        ProgramError::Custom(error as u32)
    }
}
