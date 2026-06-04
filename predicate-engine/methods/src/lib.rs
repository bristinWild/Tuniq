// Exposes the generated guest artifacts, e.g. CONFIDENTIAL_PREDICATE_ELF and
// CONFIDENTIAL_PREDICATE_ID, derived from the [[bin]] name in methods/guest.
include!(concat!(env!("OUT_DIR"), "/methods.rs"));
