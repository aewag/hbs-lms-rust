use crate::{
    constants::{MAX_H, MAX_M, MAX_N, MAX_P, MAX_PRIVATE_KEY_LENGTH},
    lm_ots::parameter::LmotsParameter,
    lms::{self, definitions::LmsAlgorithmParameter},
    util::{
        dynamic_array::DynamicArray,
        ustr::{str32u, u32str},
    },
    LmsAlgorithmType,
};

pub struct HssBinaryData {
    pub public_key: DynamicArray<u8, { 4 + 4 + 4 + 16 + MAX_M }>,
    pub private_key: DynamicArray<u8, MAX_PRIVATE_KEY_LENGTH>,
}

pub struct HssSignResult {
    pub advanced_private_key: DynamicArray<u8, MAX_PRIVATE_KEY_LENGTH>,
    pub signature:
        DynamicArray<u8, { 4 + 4 + (4 + MAX_N + (MAX_N * MAX_P)) + 4 + (MAX_M * MAX_H) }>,
}

pub fn hss_verify<P: LmotsParameter>(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
    // Todo: Check if HSS Levels = 1 and then forward data;

    if public_key.len() <= 4 {
        return false;
    }

    let hss_levels = str32u(&public_key[0..4]);

    // Needed to be compatible with reference implementation
    if hss_levels != 1 {
        panic!("HSS Levels greater than 1 are note supported yet.");
    }

    if signature.len() <= 4 {
        return false;
    }

    let signature_hss_levels = str32u(&signature[0..4]);

    // Needed to be compatible with reference implementation
    if signature_hss_levels != 0 {
        panic!("HSS Levels greater than 1 are note supported yet.")
    }

    crate::lms::verify::<P>(message, &signature[4..], &public_key[4..])
}

pub fn hss_sign<P: LmotsParameter>(message: &[u8], private_key: &[u8]) -> Option<HssSignResult> {
    let mut private_key =
        match lms::definitions::LmsPrivateKey::<P>::from_binary_representation(private_key) {
            None => return None,
            Some(x) => x,
        };

    let signature = lms::signing::LmsSignature::sign(&mut private_key, message);

    if signature.is_err() {
        return None;
    }

    let signature = signature.unwrap();

    let mut hss_signature = DynamicArray::new();
    let hss_levels = u32str(0); // Needed to be compatible with reference implementation

    hss_signature.append(&hss_levels);
    hss_signature.append(&signature.to_binary_representation().get_slice());

    let result = HssSignResult {
        advanced_private_key: private_key.to_binary_representation(),
        signature: hss_signature,
    };

    Some(result)
}

pub fn hss_keygen<P: LmotsParameter>(lms_type: LmsAlgorithmType) -> HssBinaryData {
    let lms_parameter = LmsAlgorithmParameter::new(lms_type);
    let private_key = crate::lms::generate_private_key::<P>(lms_parameter);
    let public_key = crate::lms::generate_public_key(&private_key);

    let private_key = private_key.to_binary_representation();
    let public_key = public_key.to_binary_representation();

    let mut hss_public_key = DynamicArray::new();
    let hss_levels = u32str(1); // Needed to be compatible with reference implementation

    hss_public_key.append(&hss_levels);
    hss_public_key.append(&public_key.get_slice());

    HssBinaryData {
        private_key,
        public_key: hss_public_key,
    }
}

#[cfg(test)]
mod tests {

    use crate::lm_ots::parameter;

    use super::*;

    #[test]
    fn test_signing() {
        type LmotsType = parameter::LmotsSha256N32W2;

        let keys = hss_keygen::<LmotsType>(LmsAlgorithmType::LmsSha256M32H5);

        let mut message = [
            32u8, 48, 2, 1, 48, 58, 20, 57, 9, 83, 99, 255, 0, 34, 2, 1, 0,
        ];

        let signature = hss_sign::<LmotsType>(&message, &keys.private_key.get_slice())
            .expect("Signing should complete without error.")
            .signature;

        assert!(hss_verify::<LmotsType>(
            &message,
            signature.get_slice(),
            keys.public_key.get_slice()
        ));

        message[0] = 33;

        assert!(
            hss_verify::<LmotsType>(&message, signature.get_slice(), keys.public_key.get_slice())
                == false
        );
    }
}
