import Foundation

let oracle = Oracle()
let base = URL(string: "https://h/p/")!

func hex(_ d: Data?) -> String { d.map { $0.map { String(format: "%02x", $0) }.joined() } ?? "nil" }
func range(_ r: HLSByteRangeSpec?) -> String {
    guard let r else { return "-" }
    return "\(r.length)@\(r.explicitOffset.map(String.init) ?? "nil")"
}
func key(_ k: EncryptionKey?) -> String {
    guard let k else { return "-" }
    return "\(k.url.absoluteString)#\(hex(k.explicitIV))"
}
func initialization(_ i: HLSInitializationSection?) -> String {
    guard let i else { return "-" }
    return "\(i.url.absoluteString)|\(range(i.byteRangeSpec))|\(key(i.key))"
}

for path in CommandLine.arguments.dropFirst() {
    guard let data = FileManager.default.contents(atPath: path),
          let text = String(data: data, encoding: .utf8) else { continue }
    print("== \(path)")
    print("choose \(oracle.choose(text, base: base))")
    do {
        let parsed = try oracle.parseSegments(in: text, baseURL: base)
        print("media ok seq=\(parsed.mediaSequence) n=\(parsed.segments.count)")
        for s in parsed.segments {
            print("seg \(s.url.absoluteString) r=\(range(s.byteRangeSpec)) k=\(key(s.key)) m=\(initialization(s.initialization)) d=\(s.startsDiscontinuity ? 1 : 0)")
        }
    } catch let HLSExportError.unsupportedEncryptionMethod(m) {
        print("media err unsupportedEncryptionMethod \(m)")
    } catch {
        print("media err \(error)")
    }
}
