# Code scanning review — 26 October 2026

The CodeQL rules remain enabled. This review distinguishes source fixes from test-only findings and documented behavior required by the application.

## Source fixes

- Biased playback IDs: generate a uniform 31-bit value and reject the two unsupported endpoints instead of reducing a 32-bit value modulo the output range.
- Emulator report rendering: encode Markdown/HTML metacharacters in both step names and diagnostic details, bound the text length, and preserve Unicode characters.

## Reviewed findings

### #3, #4

Reviewed: this is an Android emulator diagnostic harness. The HTTP source is the local ADB-forwarded WebView DevTools endpoint (127.0.0.1); its data is intentionally saved as JSON at a fixed output-directory filename. Network data never determines the filesystem path and is not executed. This is the expected diagnostic artifact, not attacker-controlled file access.

### #5

Reviewed intended functionality: JavaScript is required by the VK sign-in application. LoginActivity.createWebView disables file/content access, file-URL privileges, mixed content and automatic windows; LoginClient applies VkUrlPolicy to navigation and subresource requests, and no JavaScript interface is exposed. Disabling JavaScript would break sign-in. The constrained login WebView is not a general-purpose browser.

### #6

Reviewed installUpdate: the APK must be a real file directly inside the app-private cache/updates directory with a validated release filename. UpdatePolicy also requires the current package ID, a strictly newer version and an identical nonempty signing-certificate set before invoking the Android package installer. The user initiates installation and Android retains its normal confirmation/permission flow. Arbitrary packages or signers are rejected.

### #7

Reviewed: the transport only accepts the fixed https://login.vk.ru/?act=web_token endpoint, rejects redirects and uses HttpsURLConnection with the platform certificate chain and hostname validation unchanged. We do not control VK certificate rotation. Android explicitly advises against pinning third-party certificates: https://developer.android.com/privacy-and-security/security-ssl#Pinning . Standard platform TLS is retained; no trust-all manager or hostname bypass is used.

### #8, #9

Reviewed data flow: LoginActivity.createWebView explicitly sets settings.allowContentAccess = false before loading any page, alongside allowFileAccess = false. BrowsingData.wipe only clears that same hardened login WebView and never enables content access. The reported apply/lambda use does not represent a WebView with content:// access enabled.

### #10

This location is a cfg(test) regression assertion using the literal SECRET-TOKEN canary. It verifies that catalog_shape excludes token values and private labels. The panic diagnostic can contain only synthetic test data, and this test code is absent from production builds.

### #11

These are fixed AES-CBC known-answer test vectors under native/pm-native/tests/common/aes_vectors.rs. A reproducible key/IV is necessary to compare the Rust implementation with OpenSSL-generated ciphertext; no production encryption key or user data is embedded.

### #12

sequence_iv derives the HLS decryption IV from the segment media sequence number; only the high eight padding bytes are zero-initialized. This is the required RFC 8216 section 5.2 fallback when EXT-X-KEY has no IV. The function decrypts externally produced segments and does not encrypt new data with a fixed IV. https://datatracker.ietf.org/doc/html/rfc8216#section-5.2

### #13, #14, #15, #16

The cookie construction is inside the cfg(test) login.rs unit-test module. These synthetic Cookie values are passed to pure header/diagnostic selection helpers and are never installed in a WebView, HTTP client or production cookie jar. The production smoke-test cookies already set secure(true).

### #17

Reviewed common::sym and the call site: dlsym returns the address of a symbol in the dlopen-loaded C reference library. transmute_copy copies the pointer value out of local variable p; it does not return the address of p. The library handle is never dlclose-d before this dereference. Consequently the pointed-to static symbol remains alive when read; no stack-local address escapes.
