use group::Group;
use group::prime::PrimeCurveAffine;

/// A Schnorr proof in compact format.
///
/// This performs the standard folklore optimization of sending the
/// challenge in place of the commitments to the prover's randomness.
/// However, this optimization prevents batch verification.
///
/// This proof has `m+1` 32-byte elements, where `m` is the number of
/// secret variables.  This means there is no space savings for a
/// `CompactProof` over a `BatchableProof` when there is only one
/// statement.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct CompactProof<G: Group> {
    /// The Fiat-Shamir challenge.
    pub challenge: G::Scalar,
    /// The prover's responses, one per secret variable.
    pub responses: Vec<G::Scalar>,
}

// /// A Schnorr proof in batchable format.
// ///
// /// This proof has `m+n` 32-byte elements, where `m` is the number of
// /// secret variables and `n` is the number of statements.
// #[derive(Clone, Serialize, Deserialize, Debug)]
// pub struct BatchableProof<'de, G>
//     where G: PrimeCurveAffine + Group,
//           <G as Group>::Scalar: serde::Serialize + serde::Deserialize<'de>,
//     {
//     /// Commitments to the prover's blinding factors.
//     pub commitments: Vec<G>,
//     /// The prover's responses, one per secret variable.
//     pub responses: Vec<<G as group::Group>::Scalar>,
// }
