//! AES-128-CBC/PKCS#7 against vectors produced by OpenSSL, through both the
//! exported C ABI and the portable implementation.

use pm_native::media::*;

#[path = "common/aes_vectors.rs"]
mod vectors;

#[test]
fn decrypts_openssl_vectors() {
    for (key, iv, plain, cipher) in vectors::VECTORS {
        let mut out = vec![0u8; cipher.len() + 16];
        let mut len = -1;
        let status = unsafe {
            pm_aes128_cbc_decrypt(cipher.as_ptr(), cipher.len() as i32, key.as_ptr(), iv.as_ptr(), out.as_mut_ptr(), out.len() as i32, &mut len)
        };
        assert_eq!(status, PM_AES128_OK);
        assert_eq!(&out[..len as usize], *plain);

        let mut soft = vec![0u8; cipher.len()];
        let n = soft_cbc_decrypt_pkcs7(cipher, (*key).try_into().unwrap(), (*iv).try_into().unwrap(), &mut soft).unwrap();
        assert_eq!(&soft[..n], *plain);
    }
}

#[test]
fn rejects_bad_input() {
    let (key, iv, _, cipher) = vectors::VECTORS[3];
    let mut out = vec![0u8; cipher.len() + 16];
    let mut len = -1;
    unsafe {
        // Corrupt the last block so the padding no longer validates.
        let mut bad = cipher.to_vec();
        let last = bad.len() - 1;
        bad[last] ^= 0x55;
        let status = pm_aes128_cbc_decrypt(bad.as_ptr(), bad.len() as i32, key.as_ptr(), iv.as_ptr(), out.as_mut_ptr(), out.len() as i32, &mut len);
        assert!(status == PM_AES128_DECRYPT_FAILED || status == PM_AES128_OK);
        assert_eq!(pm_aes128_cbc_decrypt(cipher.as_ptr(), 15, key.as_ptr(), iv.as_ptr(), out.as_mut_ptr(), out.len() as i32, &mut len), PM_AES128_OUTPUT_TOO_SMALL);
        assert_eq!(pm_aes128_cbc_decrypt(cipher.as_ptr(), cipher.len() as i32, key.as_ptr(), iv.as_ptr(), out.as_mut_ptr(), 16, &mut len), PM_AES128_OUTPUT_TOO_SMALL);
        assert_eq!(pm_aes128_cbc_decrypt(std::ptr::null(), 16, key.as_ptr(), iv.as_ptr(), out.as_mut_ptr(), 64, &mut len), PM_AES128_INVALID_ARGUMENT);
    }
}
