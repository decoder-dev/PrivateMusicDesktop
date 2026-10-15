// Linux stand-in so the C reference compiles; AES is verified separately
// against OpenSSL-generated vectors, so this stub always reports failure.
#pragma once
#include <stddef.h>
#include <stdint.h>
typedef int32_t CCCryptorStatus;
enum { kCCSuccess = 0, kCCParamError = -4300 };
enum { kCCDecrypt = 1 };
enum { kCCAlgorithmAES = 0 };
enum { kCCOptionPKCS7Padding = 1 };
static inline CCCryptorStatus CCCrypt(uint32_t op, uint32_t alg, uint32_t opts,
    const void *key, size_t kl, const void *iv, const void *in, size_t il,
    void *out, size_t oa, size_t *moved) {
    (void)op; (void)alg; (void)opts; (void)key; (void)kl; (void)iv; (void)in;
    (void)il; (void)out; (void)oa; (void)moved;
    return kCCParamError;
}
