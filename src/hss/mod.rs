pub mod aux;
pub mod definitions;
pub mod parameter;
pub mod reference_impl_private_key;
mod seed_derive;
pub mod signing;
pub mod verify;

use core::{convert::TryFrom, marker::PhantomData};
use tinyvec::ArrayVec;

use crate::{
    constants::{MAX_HSS_PUBLIC_KEY_LENGTH, REFERENCE_IMPL_PRIVATE_KEY_SIZE, SEED_LEN},
    extract_or,
    signature::{Error, SignerMut, Verifier},
    Hasher, Signature, VerifierSignature,
};

use self::{
    definitions::{HssPrivateKey, InMemoryHssPublicKey},
    parameter::HssParameter,
    reference_impl_private_key::ReferenceImplPrivateKey,
    signing::{HssSignature, InMemoryHssSignature},
};

#[derive(Clone)]
pub struct SigningKey<H: Hasher> {
    pub bytes: ArrayVec<[u8; REFERENCE_IMPL_PRIVATE_KEY_SIZE]>,
    phantom_data: PhantomData<H>,
}

impl<H: Hasher> SigningKey<H> {
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let bytes = ArrayVec::try_from(bytes).map_err(|_| Error::new())?;

        Ok(Self {
            bytes,
            phantom_data: PhantomData,
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.bytes.as_mut_slice()
    }

    pub fn get_lifetime(&self, aux_data: Option<&mut &mut [u8]>) -> Result<u64, Error> {
        let rfc_sk = ReferenceImplPrivateKey::from_binary_representation(&self.bytes)
            .map_err(|_| Error::new())?;

        let parsed_sk = HssPrivateKey::<H>::from(&rfc_sk, aux_data).map_err(|_| Error::new())?;

        Ok(parsed_sk.get_lifetime())
    }

    pub fn try_sign_with_aux(
        &mut self,
        msg: &[u8],
        aux_data: Option<&mut &mut [u8]>,
    ) -> Result<Signature, Error> {
        let private_key = self.bytes;
        let mut private_key_update_function = |new_key: &[u8]| {
            self.bytes.as_mut_slice().copy_from_slice(new_key);
            Ok(())
        };

        hss_sign::<H>(
            msg,
            private_key.as_slice(),
            &mut private_key_update_function,
            aux_data,
        )
    }
}

impl<H: Hasher> SignerMut<Signature> for SigningKey<H> {
    fn try_sign(&mut self, msg: &[u8]) -> Result<Signature, Error> {
        self.try_sign_with_aux(msg, None)
    }
}

#[derive(Clone)]
pub struct VerifyingKey<H: Hasher> {
    pub bytes: ArrayVec<[u8; MAX_HSS_PUBLIC_KEY_LENGTH]>,
    phantom_data: PhantomData<H>,
}

impl<H: Hasher> VerifyingKey<H> {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let bytes = ArrayVec::try_from(bytes).map_err(|_| Error::new())?;

        Ok(Self {
            bytes,
            phantom_data: PhantomData,
        })
    }

    pub fn as_slice(&self) -> &[u8] {
        self.bytes.as_slice()
    }
}

impl<H: Hasher> Verifier<Signature> for VerifyingKey<H> {
    fn verify(&self, msg: &[u8], signature: &Signature) -> Result<(), Error> {
        if !hss_verify::<H>(msg, signature.as_ref(), &self.bytes) {
            return Err(Error::new());
        }
        Ok(())
    }
}

impl<'a, H: Hasher> Verifier<VerifierSignature<'a>> for VerifyingKey<H> {
    fn verify(&self, msg: &[u8], signature: &VerifierSignature) -> Result<(), Error> {
        if !hss_verify::<H>(msg, signature.as_ref(), &self.bytes) {
            return Err(Error::new());
        }
        Ok(())
    }
}

/**
 * This function is used to verify a signature.
 *
 * # Arguments
 * * `Hasher` - The hasher implementation that should be used. ```Sha256Hasher``` is a standard software implementation.
 * * `message` - The message that should be verified.
 * * `signature` - The signature that should be used for verification.
 * * `public_key` - The public key that should be used for verification.
 */
pub fn hss_verify<H: Hasher>(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
    let signature = extract_or!(InMemoryHssSignature::<H>::new(signature), false);
    let public_key = extract_or!(InMemoryHssPublicKey::<H>::new(public_key), false);

    crate::hss::verify::verify(&signature, &public_key, message).is_ok()
}

/**
 * This function is used to generate a signature.
 *
 * # Arguments
 * * `Hasher` - The hasher implementation that should be used. ```Sha256Hasher``` is a standard software implementation.
 * * `message` - The message that should be signed.
 * * `private_key` - The private key that should be used.
 * * `private_key_update_function` - The update function that is called with the new private key. This function should save the new private key.
 * * `aux_data` - Auxiliary data to speedup signature generation if available
 */

pub fn hss_sign<H: Hasher>(
    message: &[u8],
    private_key: &[u8],
    private_key_update_function: &mut dyn FnMut(&[u8]) -> Result<(), ()>,
    aux_data: Option<&mut &mut [u8]>,
) -> Result<Signature, Error> {
    hss_sign_core::<H>(
        Some(message),
        None,
        private_key,
        private_key_update_function,
        aux_data,
    )
}

#[cfg(feature = "fast_verify")]
pub fn hss_sign_mut<H: Hasher>(
    message_mut: &mut [u8],
    private_key: &[u8],
    private_key_update_function: &mut dyn FnMut(&[u8]) -> Result<(), ()>,
    aux_data: Option<&mut &mut [u8]>,
) -> Result<Signature, Error> {
    if message_mut.len() <= H::OUTPUT_SIZE.into() {
        return Err(Error::new());
    }

    let (_, message_randomizer) = message_mut.split_at(message_mut.len() - H::OUTPUT_SIZE as usize);
    if !message_randomizer.iter().all(|&byte| byte == 0u8) {
        return Err(Error::new());
    }

    hss_sign_core::<H>(
        None,
        Some(message_mut),
        private_key,
        private_key_update_function,
        aux_data,
    )
}

fn hss_sign_core<H: Hasher>(
    message: Option<&[u8]>,
    message_mut: Option<&mut [u8]>,
    private_key: &[u8],
    private_key_update_function: &mut dyn FnMut(&[u8]) -> Result<(), ()>,
    aux_data: Option<&mut &mut [u8]>,
) -> Result<Signature, Error> {
    let mut rfc_private_key = ReferenceImplPrivateKey::from_binary_representation(private_key)
        .map_err(|_| Error::new())?;

    let mut parsed_private_key =
        HssPrivateKey::<H>::from(&rfc_private_key, aux_data).map_err(|_| Error::new())?;

    let hss_signature = HssSignature::sign(&mut parsed_private_key, message, message_mut)
        .map_err(|_| Error::new())?;

    // Advance private key
    rfc_private_key.increment(&parsed_private_key);
    private_key_update_function(&rfc_private_key.to_binary_representation())
        .map_err(|_| Error::new())?;

    let hash_iterations = {
        let mut hash_iterations: u32 = 0;
        for signed_public_key in hss_signature.signed_public_keys.iter() {
            hash_iterations += signed_public_key.sig.lmots_signature.hash_iterations as u32;
        }
        hash_iterations + hss_signature.signature.lmots_signature.hash_iterations as u32
    };

    Signature::from_bytes_verbose(&hss_signature.to_binary_representation(), hash_iterations)
}

/**
 * This function is used to generate a public and private key.
 * # Arguments
 *
 * * `Hasher` - The hasher implementation that should be used. ```Sha256Hasher``` is a standard software implementation.
 * * `parameters` - An array which specifies the Winternitz parameter and tree height of each individual HSS level. The first element describes Level 1, the second element Level 2 and so on.
 * * `seed` - An optional seed which will be used to generate the private key. It must be only used for testing purposes and not for production used key pairs.
 * * `aux_data` - The reference to a slice to auxiliary data. This can be used to speedup signature generation.
 *
 * # Example
 * ```
 * use rand::{rngs::OsRng, RngCore};
 * use hbs_lms::{keygen, HssParameter, LmotsAlgorithm, LmsAlgorithm, Seed, Sha256Hasher};
 *
 * let parameters = [
 *      HssParameter::new(LmotsAlgorithm::LmotsW4, LmsAlgorithm::LmsH5),
 *      HssParameter::new(LmotsAlgorithm::LmotsW1, LmsAlgorithm::LmsH5),
 * ];
 * let mut aux_data = vec![0u8; 10_000];
 * let aux_slice: &mut &mut [u8] = &mut &mut aux_data[..];
 * let mut seed = Seed::default();
 * OsRng.fill_bytes(&mut seed);
 *
 * let (signing_key, verifying_key) =
 *      keygen::<Sha256Hasher>(&parameters, &seed, Some(aux_slice)).unwrap();
 * ```
 */
pub fn hss_keygen<H: Hasher>(
    parameters: &[HssParameter<H>],
    seed: &[u8; SEED_LEN],
    aux_data: Option<&mut &mut [u8]>,
) -> Result<(SigningKey<H>, VerifyingKey<H>), Error> {
    let private_key =
        ReferenceImplPrivateKey::generate(parameters, seed).map_err(|_| Error::new())?;

    let hss_key = HssPrivateKey::from(&private_key, aux_data).map_err(|_| Error::new())?;

    let signing_key = SigningKey::from_bytes(&private_key.to_binary_representation())?;
    let verifying_key =
        VerifyingKey::from_bytes(&hss_key.get_public_key().to_binary_representation())?;
    Ok((signing_key, verifying_key))
}

#[cfg(test)]
mod tests {

    #[cfg(feature = "fast_verify")]
    use crate::constants::MAX_HASH_SIZE;
    use crate::hasher::sha256::Sha256Hasher;
    use crate::hasher::shake256::Shake256Hasher;
    use crate::hasher::Hasher;
    use crate::{
        constants::{LMS_LEAF_IDENTIFIERS_SIZE, SEED_LEN},
        LmotsAlgorithm, LmsAlgorithm, Seed,
    };

    use super::*;

    use rand::{rngs::OsRng, RngCore};

    #[test]
    fn update_keypair() {
        let message = [
            32u8, 48, 2, 1, 48, 58, 20, 57, 9, 83, 99, 255, 0, 34, 2, 1, 0,
        ];
        let mut seed = Seed::default();
        OsRng.fill_bytes(&mut seed);
        type H = Sha256Hasher;

        let lmots = LmotsAlgorithm::LmotsW4;
        let lms = LmsAlgorithm::LmsH5;
        let parameters = [HssParameter::new(lmots, lms)];

        let (mut signing_key, verifying_key) =
            hss_keygen::<H>(&parameters, &seed, None).expect("Should generate HSS keys");

        let signing_key_const = signing_key.clone();

        let mut update_private_key = |new_key: &[u8]| {
            signing_key.as_mut_slice().copy_from_slice(new_key);
            Ok(())
        };

        let signature = hss_sign::<H>(
            &message,
            signing_key_const.as_slice(),
            &mut update_private_key,
            None,
        )
        .expect("Signing should complete without error.");

        assert!(hss_verify::<H>(
            &message,
            signature.as_ref(),
            verifying_key.as_slice()
        ));

        assert_ne!(signing_key.as_slice(), signing_key_const.as_slice());
        assert_eq!(
            signing_key.as_slice()[LMS_LEAF_IDENTIFIERS_SIZE..],
            signing_key_const.as_slice()[LMS_LEAF_IDENTIFIERS_SIZE..]
        );
    }

    #[test]
    fn exhaust_keypair() {
        let message = [
            32u8, 48, 2, 1, 48, 58, 20, 57, 9, 83, 99, 255, 0, 34, 2, 1, 0,
        ];
        let mut seed = Seed::default();
        OsRng.fill_bytes(&mut seed);
        type H = Sha256Hasher;

        let lmots = LmotsAlgorithm::LmotsW2;
        let lms = LmsAlgorithm::LmsH2;
        let parameters = [HssParameter::new(lmots, lms), HssParameter::new(lmots, lms)];

        let (mut signing_key, verifying_key) =
            hss_keygen::<H>(&parameters, &seed, None).expect("Should generate HSS keys");
        let keypair_lifetime = signing_key.get_lifetime(None).unwrap();

        assert_ne!(
            signing_key.as_slice()[(REFERENCE_IMPL_PRIVATE_KEY_SIZE - SEED_LEN)..],
            [0u8; SEED_LEN],
        );

        for index in 0..keypair_lifetime {
            assert_eq!(
                signing_key.as_slice()[..LMS_LEAF_IDENTIFIERS_SIZE],
                index.to_be_bytes(),
            );
            assert_eq!(
                keypair_lifetime - signing_key.get_lifetime(None).unwrap(),
                index
            );

            let signing_key_const = signing_key.clone();

            let mut update_private_key = |new_key: &[u8]| {
                signing_key.as_mut_slice().copy_from_slice(new_key);
                Ok(())
            };

            let signature = hss_sign::<H>(
                &message,
                signing_key_const.as_slice(),
                &mut update_private_key,
                None,
            )
            .expect("Signing should complete without error.");

            assert!(hss_verify::<H>(
                &message,
                signature.as_ref(),
                verifying_key.as_slice()
            ));
        }
        assert_eq!(
            signing_key.as_slice()[(REFERENCE_IMPL_PRIVATE_KEY_SIZE - SEED_LEN)..],
            [0u8; SEED_LEN],
        );
    }

    #[test]
    #[should_panic(expected = "Signing should panic!")]
    fn use_exhausted_keypair() {
        let message = [
            32u8, 48, 2, 1, 48, 58, 20, 57, 9, 83, 99, 255, 0, 34, 2, 1, 0,
        ];
        let mut seed = Seed::default();
        OsRng.fill_bytes(&mut seed);
        type H = Sha256Hasher;

        let lmots = LmotsAlgorithm::LmotsW2;
        let lms = LmsAlgorithm::LmsH2;
        let parameters = [HssParameter::new(lmots, lms), HssParameter::new(lmots, lms)];

        let (mut signing_key, verifying_key) =
            hss_keygen::<H>(&parameters, &seed, None).expect("Should generate HSS keys");
        let keypair_lifetime = signing_key.get_lifetime(None).unwrap();

        for index in 0..(1u64 + keypair_lifetime) {
            let signing_key_const = signing_key.clone();

            let mut update_private_key = |new_key: &[u8]| {
                signing_key.as_mut_slice().copy_from_slice(new_key);
                Ok(())
            };

            let signature = hss_sign::<H>(
                &message,
                signing_key_const.as_slice(),
                &mut update_private_key,
                None,
            )
            .unwrap_or_else(|_| {
                if index < keypair_lifetime {
                    panic!("Signing should complete without error.");
                } else {
                    assert!(signing_key.get_lifetime(None).is_err());
                    panic!("Signing should panic!");
                }
            });

            assert!(hss_verify::<H>(
                &message,
                signature.as_ref(),
                verifying_key.as_slice()
            ));
        }
    }

    #[test]
    fn test_signing_sha256() {
        test_signing_core::<Sha256Hasher>();
    }
    #[test]
    fn test_signing_shake256() {
        test_signing_core::<Shake256Hasher>();
    }

    fn test_signing_core<H: Hasher>() {
        let mut seed = Seed::default();
        OsRng.fill_bytes(&mut seed);
        let (mut signing_key, verifying_key) = hss_keygen::<H>(
            &[
                HssParameter::construct_default_parameters(),
                HssParameter::construct_default_parameters(),
                HssParameter::construct_default_parameters(),
            ],
            &seed,
            None,
        )
        .expect("Should generate HSS keys");

        let message_values = [
            32u8, 48, 2, 1, 48, 58, 20, 57, 9, 83, 99, 255, 0, 34, 2, 1, 0,
        ];
        let mut message = [0u8; 64];
        message[..message_values.len()].copy_from_slice(&message_values);

        let signing_key_const = signing_key.clone();

        let mut update_private_key = |new_key: &[u8]| {
            signing_key.as_mut_slice().copy_from_slice(new_key);
            Ok(())
        };

        let signature = hss_sign::<H>(
            &message,
            signing_key_const.as_slice(),
            &mut update_private_key,
            None,
        )
        .expect("Signing should complete without error.");

        assert!(hss_verify::<H>(
            &message,
            signature.as_ref(),
            verifying_key.as_slice(),
        ));

        message[0] = 33;

        assert!(!hss_verify::<H>(
            &message,
            signature.as_ref(),
            verifying_key.as_slice(),
        ));
    }

    #[cfg(feature = "fast_verify")]
    #[test]
    fn test_signing_fast_verify() {
        type H = Sha256Hasher;
        let mut seed = Seed::default();
        OsRng.fill_bytes(&mut seed);

        let (mut signing_key, verifying_key) = hss_keygen::<H>(
            &[
                HssParameter::construct_default_parameters(),
                HssParameter::construct_default_parameters(),
                HssParameter::construct_default_parameters(),
            ],
            &seed,
            None,
        )
        .expect("Should generate HSS keys");

        let message_values = [
            32u8, 48, 2, 1, 48, 58, 20, 57, 9, 83, 99, 255, 0, 34, 2, 1, 0,
        ];
        let mut message = [0u8; 64];
        message[..message_values.len()].copy_from_slice(&message_values);

        let signing_key_const = signing_key.clone();

        let mut update_private_key = |new_key: &[u8]| {
            signing_key.as_mut_slice().copy_from_slice(new_key);
            Ok(())
        };

        let signature = hss_sign_mut::<H>(
            &mut message,
            signing_key_const.as_slice(),
            &mut update_private_key,
            None,
        )
        .expect("Signing should complete without error.");

        assert!(H::OUTPUT_SIZE == MAX_HASH_SIZE as u16);
        assert_ne!(
            message[(message.len() - MAX_HASH_SIZE)..],
            [0u8; MAX_HASH_SIZE]
        );

        assert!(hss_verify::<H>(
            &message,
            signature.as_ref(),
            verifying_key.as_slice()
        ));
    }
}
