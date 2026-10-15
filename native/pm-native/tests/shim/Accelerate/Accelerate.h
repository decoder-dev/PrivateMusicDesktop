// Linux stand-in for the one vDSP call the C reference makes, so the C
// implementation can be compiled here and compared with the Rust one.
#pragma once
#include <math.h>
typedef unsigned long vDSP_Length;
typedef long vDSP_Stride;
static inline void vDSP_maxmgv(const float *a, vDSP_Stride s, float *c, vDSP_Length n) {
    float peak = 0.0f;
    for (vDSP_Length i = 0; i < n; ++i) {
        float m = fabsf(a[i * s]);
        if (m > peak) peak = m;
    }
    *c = peak;
}
