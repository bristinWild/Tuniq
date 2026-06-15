//! risc0-serde journal decoding for `PrivacyPreservingCircuitOutput`.
//!
//! risc0-serde encodes every value as a stream of little-endian u32 words.
//! A `u8` becomes a single word with the byte in the low byte and zeros above.
//! A `Vec<T>` becomes `[len: u32][T; len]`. A `[u8; 32]` (Nullifier, Commitment,
//! CommitmentSetDigest) becomes 32 such words.
//!
//! `PrivacyPreservingCircuitOutput` field order:
//!   public_pre_states:  Vec<AccountWithMetadata>
//!   public_post_states: Vec<Account>
//!   ciphertexts:        Vec<Ciphertext>            where Ciphertext = Vec<u8>
//!   new_commitments:    Vec<Commitment>            where Commitment = [u8; 32]
//!   new_nullifiers:     Vec<(Nullifier, CommitmentSetDigest)>  each = 64 words
//!   block_validity_window / timestamp_validity_window (trailing, unused here)
//!
//! KNOWN LIMITATION: this walker only supports the shape produced by
//! `check_balance_over_threshold` — `public_pre_states` and `public_post_states`
//! are both empty (len == 0). If a future predicate produces non-empty values
//! for either, this decoder must be extended to know the encoded size of
//! `AccountWithMetadata` / `Account` to skip over them correctly.

use anchor_lang::prelude::*;

#[error_code]
pub enum JournalDecodeError {
    #[msg("journal too short")]
    JournalTooShort,
    #[msg("journal length is not a multiple of 4 (not word-aligned)")]
    NotWordAligned,
    #[msg("unsupported journal shape: public_pre_states/public_post_states must be empty")]
    UnsupportedShape,
    #[msg("nullifier entry index out of range")]
    NullifierIndexOutOfRange,
    #[msg("encoded byte has nonzero high bytes — not a valid u8 word")]
    InvalidByteWord,
}

/// Reads the u32 LE word at word-index `i` (i.e. byte offset `i * 4`).
fn read_word(journal: &[u8], i: usize) -> Result<u32> {
    let off = i
        .checked_mul(4)
        .ok_or(JournalDecodeError::JournalTooShort)?;
    let end = off
        .checked_add(4)
        .ok_or(JournalDecodeError::JournalTooShort)?;
    require!(end <= journal.len(), JournalDecodeError::JournalTooShort);
    Ok(u32::from_le_bytes(journal[off..end].try_into().unwrap()))
}

/// Decodes a risc0-serde-encoded `u8` at word-index `i`: the word must be
/// `0x000000XX` (high three bytes zero).
fn read_u8_word(journal: &[u8], i: usize) -> Result<u8> {
    let w = read_word(journal, i)?;
    require!(w <= 0xFF, JournalDecodeError::InvalidByteWord);
    Ok(w as u8)
}

/// Decodes a `[u8; 32]` (Nullifier / Commitment / CommitmentSetDigest) starting
/// at word-index `i` (32 consecutive words).
fn read_bytes32(journal: &[u8], i: usize) -> Result<[u8; 32]> {
    let mut out = [0u8; 32];
    for k in 0..32 {
        out[k] = read_u8_word(journal, i + k)?;
    }
    Ok(out)
}

/// Walks `journal` (a `PrivacyPreservingCircuitOutput`, risc0-serde-encoded) and
/// decodes the `entry_index`-th `Nullifier` from `new_nullifiers`.
///
/// Returns the nullifier as `[u8; 32]`. This is the value used as the
/// replay-guard PDA seed — it is read directly from the proof-committed
/// journal, so the caller cannot misrepresent it.
pub fn decode_nullifier_from_journal(journal: &[u8], entry_index: u8) -> Result<[u8; 32]> {
    require!(journal.len() % 4 == 0, JournalDecodeError::NotWordAligned);
    require!(journal.len() >= 4, JournalDecodeError::JournalTooShort);

    let mut word = 0usize;

    // public_pre_states: Vec<AccountWithMetadata> — must be empty.
    let pre_len = read_word(journal, word)?;
    require!(pre_len == 0, JournalDecodeError::UnsupportedShape);
    word += 1;

    // public_post_states: Vec<Account> — must be empty.
    let post_len = read_word(journal, word)?;
    require!(post_len == 0, JournalDecodeError::UnsupportedShape);
    word += 1;

    // ciphertexts: Vec<Ciphertext>, Ciphertext = Vec<u8>.
    // Each ciphertext is [inner_len: u32][inner_len words of u8].
    let ciphertext_count = read_word(journal, word)?;
    word += 1;
    for _ in 0..ciphertext_count {
        let inner_len = read_word(journal, word)? as usize;
        word += 1;
        word = word
            .checked_add(inner_len)
            .ok_or(JournalDecodeError::JournalTooShort)?;
    }

    // new_commitments: Vec<Commitment>, Commitment = [u8; 32] = 32 words each.
    let commitment_count = read_word(journal, word)?;
    word += 1;
    word = word
        .checked_add(
            (commitment_count as usize)
                .checked_mul(32)
                .ok_or(JournalDecodeError::JournalTooShort)?,
        )
        .ok_or(JournalDecodeError::JournalTooShort)?;

    // new_nullifiers: Vec<(Nullifier, CommitmentSetDigest)>, each = 32 + 32 = 64 words.
    let nullifier_count = read_word(journal, word)?;
    word += 1;
    require!(
        (entry_index as u32) < nullifier_count,
        JournalDecodeError::NullifierIndexOutOfRange
    );

    let entry_offset = word
        .checked_add(
            (entry_index as usize)
                .checked_mul(64)
                .ok_or(JournalDecodeError::JournalTooShort)?,
        )
        .ok_or(JournalDecodeError::JournalTooShort)?;

    // The Nullifier is the first 32 words of the (Nullifier, CommitmentSetDigest) tuple.
    read_bytes32(journal, entry_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal journal matching the shape verified against the real
    /// M0–M4 artifact: empty pre/post states, one 68-byte ciphertext, one
    /// commitment, one nullifier + digest, plus 4 trailing words.
    fn build_test_journal(nullifier: [u8; 32], digest: [u8; 32]) -> Vec<u8> {
        let mut w: Vec<u32> = Vec::new();
        w.push(0); // public_pre_states len
        w.push(0); // public_post_states len
        w.push(1); // ciphertexts len
        w.push(68); // ciphertext[0] inner len
        w.extend(std::iter::repeat(0u32).take(68)); // ciphertext bytes
        w.push(1); // new_commitments len
        w.extend(std::iter::repeat(0u32).take(32)); // commitment[0]
        w.push(1); // new_nullifiers len
        for b in nullifier {
            w.push(b as u32);
        }
        for b in digest {
            w.push(b as u32);
        }
        w.extend(std::iter::repeat(0u32).take(4)); // validity windows

        let mut bytes = Vec::with_capacity(w.len() * 4);
        for word in w {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn decodes_known_nullifier() {
        let nullifier =
            hex::decode("39e15eadcbc684bfca46f76bec4182d71cf5b26833d6e20af0b283515d9f92b2")
                .unwrap()
                .try_into()
                .unwrap();
        let digest = [0xABu8; 32];
        let journal = build_test_journal(nullifier, digest);

        // Sanity: matches the real artifact's length and known offset.
        assert_eq!(journal.len(), 696);

        let decoded = decode_nullifier_from_journal(&journal, 0).unwrap();
        assert_eq!(decoded, nullifier);
    }

    #[test]
    fn rejects_out_of_range_index() {
        let nullifier = [0x39u8; 32];
        let digest = [0xABu8; 32];
        let journal = build_test_journal(nullifier, digest);
        assert!(decode_nullifier_from_journal(&journal, 1).is_err());
    }

    #[test]
    fn rejects_non_word_aligned_journal() {
        let journal = vec![0u8; 7];
        assert!(decode_nullifier_from_journal(&journal, 0).is_err());
    }

    #[test]
    fn rejects_nonempty_public_states() {
        let mut journal = build_test_journal([0u8; 32], [0u8; 32]);
        // Corrupt public_pre_states len to 1.
        journal[0..4].copy_from_slice(&1u32.to_le_bytes());
        assert!(decode_nullifier_from_journal(&journal, 0).is_err());
    }
}
