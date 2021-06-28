use crate::{
    constants::{D_MESG, D_PBLC, MAX_N, MAX_P},
    hasher::Hasher,
    util::{
        coef::coef,
        dynamic_array::DynamicArray,
        ustr::{u16str, u8str},
    },
};

use super::{
    definitions::{IType, LmotsPublicKey, QType},
    parameter::LmotsParameter,
    signing::LmotsSignature,
};

#[allow(non_snake_case)]
#[allow(dead_code)]
pub fn verify_signature<P: LmotsParameter>(
    signature: &LmotsSignature<P>,
    public_key: &LmotsPublicKey<P>,
    message: &[u8],
) -> bool {
    let public_key_candidate =
        generate_public_key_canditate(signature, &public_key.I, &public_key.q, message);

    public_key_candidate == public_key.key
}

#[allow(non_snake_case)]
pub fn generate_public_key_canditate<P: LmotsParameter>(
    signature: &LmotsSignature<P>,
    I: &IType,
    q: &QType,
    message: &[u8],
) -> DynamicArray<u8, MAX_N> {
    let mut parameter = <P>::new();

    parameter.update(I);
    parameter.update(q);
    parameter.update(&D_MESG);
    parameter.update(signature.C.get_slice());
    parameter.update(message);

    let Q = parameter.finalize_reset();
    let Q_and_checksum = parameter.get_appended_with_checksum(Q.get_slice());

    let mut z: DynamicArray<DynamicArray<u8, MAX_N>, MAX_P> = DynamicArray::new();
    let max_w = 2u64.pow(parameter.get_w() as u32) - 1;

    for i in 0..parameter.get_p() {
        let a = coef(
            &Q_and_checksum.get_slice(),
            i as u64,
            parameter.get_w() as u64,
        );
        let mut tmp = signature.y[i as usize];

        for j in a..max_w {
            parameter.update(I);
            parameter.update(q);
            parameter.update(&u16str(i));
            parameter.update(&u8str(j as u8));
            parameter.update(tmp.get_slice());
            tmp = parameter.finalize_reset();
        }
        z[i as usize] = tmp;
    }

    parameter.update(I);
    parameter.update(q);
    parameter.update(&D_PBLC);

    for item in z.into_iter() {
        parameter.update(item.get_slice());
    }

    parameter.finalize()
}

#[cfg(test)]
mod tests {
    use crate::lm_ots::{
        definitions::{IType, LmotsPublicKey, QType, Seed},
        keygen::{generate_private_key, generate_public_key},
        parameter,
        signing::LmotsSignature,
        verify::verify_signature,
    };

    macro_rules! generate_test {
        ($name:ident, $type:ty) => {
            #[test]
            fn $name() {
                let i: IType = [2u8; 16];
                let q: QType = [0u8; 4];
                let seed: Seed = [
                    74, 222, 147, 88, 142, 55, 215, 148, 59, 52, 12, 170, 167, 93, 94, 237, 90,
                    176, 213, 104, 226, 71, 9, 74, 130, 187, 214, 75, 151, 184, 216, 175,
                ];

                let private_key = generate_private_key(i, q, seed);
                let public_key: LmotsPublicKey<$type> = generate_public_key(&private_key);

                let mut message = [1, 3, 5, 9, 0];

                let signature = LmotsSignature::sign(&private_key, &message);

                assert!(verify_signature(&signature, &public_key, &message) == true);

                message[0] = 5;
                assert!(verify_signature(&signature, &public_key, &message) == false);
            }
        };
    }

    generate_test!(lmots_sha256_n32_w1_verify_test, parameter::LmotsSha256N32W1);

    generate_test!(lmots_sha256_n32_w2_verify_test, parameter::LmotsSha256N32W2);
    generate_test!(lmots_sha256_n32_w4_verify_test, parameter::LmotsSha256N32W4);
    generate_test!(lmots_sha256_n32_w8_verify_test, parameter::LmotsSha256N32W8);
}
