import Foundation
struct Rec { let id: String; let lastPlayedAt: Date; let byteCount: Int64; let auto: Bool }
func old(_ records: [String: Rec], protected: String?, required: Int64) -> [String] {
    var out: [String] = []
    if required > 0 {
        var released: Int64 = 0
        let candidates = records.values.filter { $0.auto && $0.id != protected }.sorted { $0.lastPlayedAt < $1.lastPlayedAt }
        for r in candidates { out.append(r.id); released += r.byteCount; if released >= required { break } }
    }
    return out
}
func new(_ records: [String: Rec], protected: String?, required: Int64) -> [String] {
    guard required > 0 else { return [] }
    let entries = Array(records.values)
    let candidates = entries.map { r in
        pm_eviction_candidate(last_played_at: r.lastPlayedAt.timeIntervalSinceReferenceDate, byte_count: r.byteCount, evictable: r.auto && r.id != protected)
    }
    var chosen = [Int32](repeating: 0, count: entries.count)
    let count = candidates.withUnsafeBufferPointer { input in
        chosen.withUnsafeMutableBufferPointer { output in
            Int(pm_offline_select_evictions(input.baseAddress, Int32(clamping: input.count), required, output.baseAddress, Int32(clamping: output.count)))
        }
    }
    return chosen.prefix(count).map { entries[Int($0)].id }
}
var bad = 0
for round in 0..<20000 {
    var recs: [String: Rec] = [:]
    let n = Int.random(in: 0...12)
    var times = Set<Double>()
    for i in 0..<n {
        var t = Double(Int.random(in: 0...100000)); while times.contains(t) { t += 0.5 }; times.insert(t)
        recs["t\(i)"] = Rec(id: "t\(i)", lastPlayedAt: Date(timeIntervalSinceReferenceDate: t), byteCount: Int64.random(in: 0...5_000_000), auto: Bool.random())
    }
    let prot: String? = Bool.random() ? "t\(Int.random(in: 0...12))" : nil
    let req = Int64.random(in: -100...12_000_000)
    if old(recs, protected: prot, required: req) != new(recs, protected: prot, required: req) { bad += 1 }
    _ = round
}
print("mismatches:", bad)
