use magicblock_core::Slot;

/// Reads the ER slot at the instant a receipt is sequenced.
///
/// The writer calls this itself rather than accepting a slot from the caller,
/// so `ingress_slot` advances with `seq` and can never disagree with it.
pub type SlotSource = Box<dyn Fn() -> Slot + Send>;
