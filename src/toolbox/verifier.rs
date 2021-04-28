use std::ops::{AddAssign, Mul};

use ff::Field;
use group::{Group, GroupEncoding};
use rand::thread_rng;
use serde::{Serialize, de::DeserializeOwned};

use crate::toolbox::{SchnorrCS, TranscriptProtocol};
use crate::{BatchableProof, CompactProof, ProofError, Transcript};

/// Used to produce verification results.
///
/// To use a [`Verifier`], first construct one using [`Verifier::new()`],
/// supplying a domain separation label, as well as the transcript to
/// operate on.
///
/// Then, allocate secret ([`Verifier::allocate_scalar`]) variables
/// and allocate and assign public ([`Verifier::allocate_point`])
/// variables, and use those variables to define the proof statements.
/// Note that no assignments to secret variables are assigned, since
/// the verifier doesn't know the secrets.
///
/// Finally, use [`Verifier::verify_compact`] or
/// [`Verifier::verify_batchable`] to consume the verifier and produce
/// a verification result.
pub struct Verifier<'t, G> {
    transcript: &'t mut Transcript,
    num_scalars: usize,
    points: Vec<G>,
    point_labels: Vec<&'static [u8]>,
    constraints: Vec<(PointVar, Vec<(ScalarVar, PointVar)>)>,
}

/// A secret variable used during verification.
///
/// Note that this variable is only a placeholder; it has no
/// assignment, because the verifier doesn't know the secrets.
#[derive(Copy, Clone)]
pub struct ScalarVar(usize);
/// A public variable used during verification.
#[derive(Copy, Clone)]
pub struct PointVar(usize);

impl<'t, G> Verifier<'t, G>
    where G: GroupEncoding + Group + Mul<<G as Group>::Scalar, Output=G>,
          <G as Group>::Scalar: AddAssign + Mul + Serialize + DeserializeOwned,
    {
    /// Construct a verifier for the proof statement with the given
    /// `proof_label`, operating on the given `transcript`.
    pub fn new(proof_label: &'static [u8], transcript: &'t mut Transcript) -> Self {
        TranscriptProtocol::<G>::domain_sep(transcript, proof_label);
        Verifier {
            transcript,
            num_scalars: 0,
            points: Vec::default(),
            point_labels: Vec::default(),
            constraints: Vec::default(),
        }
    }

    /// Allocate a placeholder scalar variable, without an assignment.
    pub fn allocate_scalar(&mut self, label: &'static [u8]) -> ScalarVar {
        TranscriptProtocol::<G>::append_scalar_var(self.transcript, label);
        self.num_scalars += 1;
        ScalarVar(self.num_scalars - 1)
    }

    /// Attempt to allocate a point variable, or fail verification if
    /// the assignment is invalid.
    pub fn allocate_point(
        &mut self,
        label: &'static [u8],
        assignment: G,
    ) -> Result<PointVar, ProofError> {
        self.transcript
            .validate_and_append_point_var(label, &assignment)?;
        self.points.push(assignment);
        self.point_labels.push(label);
        Ok(PointVar(self.points.len() - 1))
    }

    /// Consume the verifier to produce a verification of a [`CompactProof`].
    pub fn verify_compact(self, proof: &CompactProof<G>) -> Result<(), ProofError> {
        // Check that there are as many responses as secret variables
        if proof.responses.len() != self.num_scalars {
            return Err(ProofError::VerificationFailure);
        }

        // WSK: I don't think we need to worry about this, because we're not using compressed points
        // there's also no Option<> to worry about

        // Decompress all parameters or fail verification.
        // let points = self
        //     .points
        //     .iter()
        //     // .map(|pt| pt.decompress())
        //     .collect::<Option<Vec<G1Affine>>>()
        //     .ok_or(ProofError::VerificationFailure)?;

        // Recompute the prover's commitments based on their claimed challenge value:
        let minus_c = -proof.challenge;
        for (lhs_var, rhs_lc) in &self.constraints {

            let mut commitment = self.points[lhs_var.0] * minus_c;
            for (sc_var, pt_var) in rhs_lc.iter() {
                commitment += self.points[pt_var.0] * proof.responses[sc_var.0];
            }
            // let commitment = RistrettoPoint::vartime_multiscalar_mul(
            //     rhs_lc
            //         .iter()
            //         .map(|(sc_var, _pt_var)| proof.responses[sc_var.0])
            //         .chain(iter::once(minus_c)),
            //     rhs_lc
            //         .iter()
            //         .map(|(_sc_var, pt_var)| points[pt_var.0])
            //         .chain(iter::once(points[lhs_var.0])),
            // );

            self.transcript
                .append_blinding_commitment(self.point_labels[lhs_var.0], &G::from(commitment));
        }

        // Recompute the challenge and check if it's the claimed one
        let challenge: <G as Group>::Scalar = TranscriptProtocol::<G>::get_challenge(self.transcript, b"chal");

        if challenge == proof.challenge {
            Ok(())
        } else {
            Err(ProofError::VerificationFailure)
        }
    }

    /// Consume the verifier to produce a verification of a [`BatchableProof`].
    pub fn verify_batchable<'tmp, 'proof>(self, proof: &'proof BatchableProof<G>) -> Result<(), ProofError>
    where G::Scalar: Serialize + DeserializeOwned,
    {
        // Check that there are as many responses as secret variables
        if proof.responses.len() != self.num_scalars {
            return Err(ProofError::VerificationFailure);
        }
        // Check that there are as many commitments as constraints
        if proof.commitments.len() != self.constraints.len() {
            return Err(ProofError::VerificationFailure);
        }

        // Feed the prover's commitments into the transcript:
        for (i, commitment) in proof.commitments.iter().enumerate() {
            let (ref lhs_var, ref _rhs_lc) = self.constraints[i];
            self.transcript.validate_and_append_blinding_commitment(
                self.point_labels[lhs_var.0],
                commitment,
            )?;
        }

        let minus_c = -TranscriptProtocol::<G>::get_challenge(self.transcript, b"chal");

        let commitments_offset = self.points.len();
        let mut coeffs = vec![<G as Group>::Scalar::zero(); self.points.len() + proof.commitments.len()];
        // For each constraint of the form Q = sum(P_i, x_i),
        // we want to ensure Q_com = sum(P_i, resp_i) - c * Q,
        // so add the check rand*( sum(P_i, resp_i) - c * Q - Q_com ) == 0
        for i in 0..self.constraints.len() {
            let (ref lhs_var, ref rhs_lc) = self.constraints[i];
            let random_factor = <G as Group>::Scalar::random(&mut thread_rng());

            coeffs[commitments_offset + i] += -random_factor;
            coeffs[lhs_var.0] += random_factor * minus_c;
            for (sc_var, pt_var) in rhs_lc {
                coeffs[pt_var.0] += random_factor * proof.responses[sc_var.0];
            }
        }

        let mut check = G::identity();
        for i in 0..self.points.len() {
            check += self.points[i] * coeffs[i];
        }
        for i in 0..proof.commitments.len() {
            check += proof.commitments[i] * coeffs[i+commitments_offset];
        }

        if bool::from(<G as Group>::is_identity(&check)) {
            Ok(())
        } else {
            Err(ProofError::VerificationFailure)
        }
    }
}

impl<'a, G> SchnorrCS for Verifier<'a, G> where G: Group {
    type ScalarVar = ScalarVar;
    type PointVar = PointVar;

    fn constrain(&mut self, lhs: PointVar, linear_combination: Vec<(ScalarVar, PointVar)>) {
        self.constraints.push((lhs, linear_combination));
    }
}
