import Foundation
struct Coefficients { var b0: Float; var b1: Float; var b2: Float; var a1: Float; var a2: Float }
enum Old {
    static func peakingCoefficients(
        frequency: Double,
        gain: Double,
        sampleRate: Double
    ) -> Coefficients {
        let safeFrequency = min(frequency, sampleRate * 0.45)
        let amplitude = pow(10, gain / 40)
        let omega = 2 * Double.pi * safeFrequency / sampleRate
        let alpha = sin(omega) / (2 * 1.15)
        let cosine = cos(omega)
        let a0 = 1 + alpha / amplitude

        return Coefficients(
            b0: Float((1 + alpha * amplitude) / a0),
            b1: Float((-2 * cosine) / a0),
            b2: Float((1 - alpha * amplitude) / a0),
            a1: Float((-2 * cosine) / a0),
            a2: Float((1 - alpha / amplitude) / a0)
        )
    }

    static func lowShelfCoefficients(
        frequency: Double,
        gain: Double,
        sampleRate: Double
    ) -> Coefficients {
        let safeFrequency = min(max(frequency, 20), sampleRate * 0.45)
        let amplitude = pow(10, gain / 40)
        let omega = 2 * Double.pi * safeFrequency / sampleRate
        let cosine = cos(omega)
        let sine = sin(omega)
        let beta = sqrt(amplitude) / 1.0
        let a0 = (amplitude + 1)
            + (amplitude - 1) * cosine
            + beta * sine
        return Coefficients(
            b0: Float(
                (
                    amplitude
                        * (
                            (amplitude + 1)
                                - (amplitude - 1) * cosine
                                + beta * sine
                        )
                ) / a0
            ),
            b1: Float(
                (
                    2 * amplitude
                        * ((amplitude - 1) - (amplitude + 1) * cosine)
                ) / a0
            ),
            b2: Float(
                (
                    amplitude
                        * (
                            (amplitude + 1)
                                - (amplitude - 1) * cosine
                                - beta * sine
                        )
                ) / a0
            ),
            a1: Float(
                (-2 * ((amplitude - 1) + (amplitude + 1) * cosine)) / a0
            ),
            a2: Float(
                (
                    (amplitude + 1)
                        + (amplitude - 1) * cosine
                        - beta * sine
                ) / a0
            )
        )
    }

    static func highPassCoefficients(
        frequency: Double,
        sampleRate: Double
    ) -> Coefficients {
        let safeFrequency = min(max(frequency, 20), sampleRate * 0.45)
        let omega = 2 * Double.pi * safeFrequency / sampleRate
        let cosine = cos(omega)
        let sine = sin(omega)
        let alpha = sine / (2 * (1 / sqrt(2.0)))
        let a0 = 1 + alpha
        return Coefficients(
            b0: Float(((1 + cosine) / 2) / a0),
            b1: Float((-(1 + cosine)) / a0),
            b2: Float(((1 + cosine) / 2) / a0),
            a1: Float((-2 * cosine) / a0),
            a2: Float((1 - alpha) / a0)
        )
    }
}
func same(_ a: Coefficients, _ b: PMBiquadCoeffs) -> Bool {
    a.b0.bitPattern == b.b0.bitPattern && a.b1.bitPattern == b.b1.bitPattern &&
    a.b2.bitPattern == b.b2.bitPattern && a.a1.bitPattern == b.a1.bitPattern &&
    a.a2.bitPattern == b.a2.bitPattern
}
var checked = 0, differ = 0, worst: Float = 0
func ulps(_ a: Coefficients, _ b: PMBiquadCoeffs) -> Float {
    max(abs(a.b0 - b.b0), abs(a.b1 - b.b1), abs(a.b2 - b.b2), abs(a.a1 - b.a1), abs(a.a2 - b.a2))
}
let rates: [Double] = [8000, 11025, 22050, 32000, 44100, 48000, 88200, 96000, 192000]
var freqs: [Double] = [0, 5, 20, 31, 62, 90, 125, 140, 250, 500, 1000, 2000, 4000, 8000, 16000, 20000, 30000, 100000]
var gains: [Double] = [-24, -12, -6, -3.5, -0.0001, 0, 0.0002, 1, 3, 6, 12, 24]
for sr in rates { for f in freqs {
    for g in gains {
        let a = Old.peakingCoefficients(frequency: f, gain: g, sampleRate: sr)
        let b = pm_eq_peaking_coefficients(f, g, sr)
        checked += 1; if !same(a, b) { differ += 1; worst = max(worst, ulps(a, b)) }
        let c = Old.lowShelfCoefficients(frequency: f, gain: g, sampleRate: sr)
        let d = pm_eq_low_shelf_coefficients(f, g, sr)
        checked += 1; if !same(c, d) { differ += 1; worst = max(worst, ulps(c, d)) }
    }
    let e = Old.highPassCoefficients(frequency: f, sampleRate: sr)
    let h = pm_eq_high_pass_coefficients(f, sr)
    checked += 1; if !same(e, h) { differ += 1; worst = max(worst, ulps(e, h)) }
} }
var rng = SystemRandomNumberGenerator()
for _ in 0..<20000 {
    let sr = Double.random(in: 4000...200000, using: &rng)
    let f = Double.random(in: 1...60000, using: &rng)
    let g = Double.random(in: -30...30, using: &rng)
    let a = Old.peakingCoefficients(frequency: f, gain: g, sampleRate: sr)
    let b = pm_eq_peaking_coefficients(f, g, sr)
    checked += 1; if !same(a, b) { differ += 1; worst = max(worst, ulps(a, b)) }
    let c = Old.lowShelfCoefficients(frequency: f, gain: g, sampleRate: sr)
    let d = pm_eq_low_shelf_coefficients(f, g, sr)
    checked += 1; if !same(c, d) { differ += 1; worst = max(worst, ulps(c, d)) }
    let e = Old.highPassCoefficients(frequency: f, sampleRate: sr)
    let h = pm_eq_high_pass_coefficients(f, sr)
    checked += 1; if !same(e, h) { differ += 1; worst = max(worst, ulps(e, h)) }
}
print("checked \(checked), not bit-identical \(differ), worst abs diff \(worst)")
