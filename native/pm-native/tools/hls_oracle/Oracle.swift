import Foundation

enum HLSExportError: Error {
    case invalidEncryptionKey
    case unsupportedEncryptionMethod(String)
    case invalidInitializationSection
    case invalidByteRange
    case encryptedInitializationRequiresExplicitIV
    case tooManySegments
}

    struct HLSByteRangeSpec: Equatable, Sendable {
        let length: Int
        let explicitOffset: Int?
    }

    struct HLSInitializationSection: Equatable, Sendable {
        let url: URL
        let byteRangeSpec: HLSByteRangeSpec?
        let key: EncryptionKey?

        var cacheKey: InitializationCacheKey {
            InitializationCacheKey(
                url: url,
                lowerBound: byteRangeSpec?.explicitOffset,
                upperBound: byteRangeSpec.flatMap { spec in
                    spec.explicitOffset.map { $0 + spec.length }
                }
            )
        }
    }

    struct HLSSegment: Sendable {
        let url: URL
        let byteRangeSpec: HLSByteRangeSpec?
        let key: EncryptionKey?
        let initialization: HLSInitializationSection?
        let startsDiscontinuity: Bool
    }

    struct HLSMediaPlaylist: Sendable {
        let segments: [HLSSegment]
        let mediaSequence: Int
    }

    struct HLSVariant {
        var url: URL
        let bandwidth: Int
        let codecs: [String]
        let audioGroupID: String?
    }

    struct HLSAudioRendition {
        let groupID: String
        let name: String?
        let url: URL
        let isDefault: Bool
        let isAutoselect: Bool
    }

    struct InitializationCacheKey: Hashable {
        let url: URL
        let lowerBound: Int?
        let upperBound: Int?
    }

    struct EncryptionKey: Equatable, Sendable {
        let url: URL
        let explicitIV: Data?

        var hasExplicitIV: Bool {
            explicitIV != nil
        }

        func iv(_ segmentNumber: Int) -> Data {
            if let explicitIV {
                return explicitIV
            }
            // RFC 8216 §5.2: default IV is the media sequence as a
            // 128-bit big-endian integer (8 zero bytes + 8 sequence bytes).
            var iv = Data(repeating: 0, count: 16)
            var sequence = UInt64(segmentNumber)
            for byteIndex in stride(from: 15, through: 8, by: -1) {
                iv[byteIndex] = UInt8(sequence & 0xff)
                sequence >>= 8
            }
            return iv
        }
    }


final class Oracle {
    let segmentLimit = 4000

    func parseVariants(
        in playlist: String,
        baseURL: URL
    ) -> [HLSVariant] {
        let lines = playlist.components(separatedBy: .newlines)
        var variants: [HLSVariant] = []
        var pending: HLSVariant?
        for rawLine in lines {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            if line.hasPrefix("#EXT-X-STREAM-INF") {
                let attributes = parseAttributeList(
                    String(line.dropFirst("#EXT-X-STREAM-INF:".count))
                )
                pending = HLSVariant(
                    url: URL(fileURLWithPath: "pending"),
                    bandwidth: Int(attributes["BANDWIDTH"] ?? "") ?? 0,
                    codecs: (attributes["CODECS"] ?? "")
                        .split(separator: ",")
                        .map { String($0).trimmingCharacters(
                            in: .whitespacesAndNewlines
                        ) },
                    audioGroupID: attributes["AUDIO"]
                )
                continue
            }
            guard !line.hasPrefix("#"), !line.isEmpty else { continue }
            if var variant = pending {
                variant.url = resolvedURL(line, base: baseURL)
                variants.append(variant)
                pending = nil
            }
        }
        return variants
    }

    func parseAudioRenditions(
        in playlist: String,
        baseURL: URL
    ) -> [HLSAudioRendition] {
        let lines = playlist.components(separatedBy: .newlines)
        var renditions: [HLSAudioRendition] = []
        for rawLine in lines {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            guard line.hasPrefix("#EXT-X-MEDIA:") else { continue }
            let attributes = parseAttributeList(
                String(line.dropFirst("#EXT-X-MEDIA:".count))
            )
            guard attributes["TYPE"]?.uppercased() == "AUDIO",
                  let groupID = attributes["GROUP-ID"],
                  let uri = attributes["URI"] else {
                continue
            }
            renditions.append(HLSAudioRendition(
                groupID: groupID,
                name: attributes["NAME"],
                url: resolvedURL(uri, base: baseURL),
                isDefault: attributes["DEFAULT"] == "YES",
                isAutoselect: attributes["AUTOSELECT"] == "YES"
            ))
        }
        return renditions
    }

    /// Selection order: `DEFAULT=YES` rendition → `AUTOSELECT=YES`
    /// rendition → first URI in a group referenced by a variant → best
    /// audio-only variant (codec mp4a/aac/ac-3/ec-3/mp3 and no video codec).
    /// Video-only variants are never chosen.
    func chooseAudioPlaylist(
        variants: [HLSVariant],
        renditions: [HLSAudioRendition]
    ) -> URL? {
        if let defaulted = renditions.first(where: { $0.isDefault }) {
            return defaulted.url
        }
        if let autoselected = renditions.first(where: { $0.isAutoselect }) {
            return autoselected.url
        }
        let referencedGroups = Set(
            variants.compactMap(\.audioGroupID)
        )
        if let groupID = renditions.map(\.groupID).first(
            where: { referencedGroups.contains($0) }
        ), let rendition = renditions.first(where: {
            $0.groupID == groupID
        }) {
            return rendition.url
        }
        let audioOnly = variants.filter { variant in
            let audioCodec = variant.codecs.contains {
                $0.hasPrefix("mp4a") || $0.hasPrefix("aac")
                    || $0.hasPrefix("ac-3") || $0.hasPrefix("ec-3")
                    || $0.hasPrefix("mp3")
            }
            let videoCodec = variant.codecs.contains {
                $0.hasPrefix("avc1") || $0.hasPrefix("hvc1")
                    || $0.hasPrefix("hev1") || $0.hasPrefix("vp9")
                    || $0.hasPrefix("av01")
            }
            return audioCodec && !videoCodec
        }
        return audioOnly
            .max(by: { $0.bandwidth < $1.bandwidth })?
            .url
    }

    // MARK: - Playlist parsing

    func parseSegments(
        in playlist: String,
        baseURL: URL
    ) throws -> HLSMediaPlaylist {
        var segments: [HLSSegment] = []
        var mediaSequence = 0
        var currentKey: EncryptionKey?
        var pendingByteRange: HLSByteRangeSpec?
        var pendingInitialization: HLSInitializationSection?
        var startsDiscontinuity = false

        for rawLine in playlist.components(separatedBy: .newlines) {
            let line = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
            if line.hasPrefix("#EXT-X-MEDIA-SEQUENCE:") {
                mediaSequence =
                    Int(line.replacingOccurrences(
                        of: "#EXT-X-MEDIA-SEQUENCE:",
                        with: ""
                    )) ?? 0
            } else if line.hasPrefix("#EXT-X-KEY:") {
                currentKey = try parseKey(
                    line,
                    baseURL: baseURL
                )
            } else if line.hasPrefix("#EXT-X-BYTERANGE:") {
                pendingByteRange = parseByteRangeSpec(line)
            } else if line.hasPrefix("#EXT-X-MAP:") {
                pendingInitialization = try parseInitializationSection(
                    line,
                    baseURL: baseURL,
                    currentKey: currentKey
                )
            } else if line.hasPrefix("#EXT-X-DISCONTINUITY") {
                startsDiscontinuity = true
            } else if !line.hasPrefix("#"), !line.isEmpty {
                guard segments.count < segmentLimit else {
                    throw HLSExportError.tooManySegments
                }
                segments.append(HLSSegment(
                    url: resolvedURL(line, base: baseURL),
                    byteRangeSpec: pendingByteRange,
                    key: currentKey,
                    initialization: pendingInitialization,
                    startsDiscontinuity: startsDiscontinuity
                ))
                pendingByteRange = nil
                startsDiscontinuity = false
            }
        }
        return HLSMediaPlaylist(
            segments: segments,
            mediaSequence: mediaSequence
        )
    }

    /// Parses `#EXT-X-KEY`. `METHOD=NONE` clears the current key;
    /// `METHOD=AES-128` requires a URI and accepts an optional IV; any other
    /// method (SAMPLE-AES, ...) is rejected instead of being treated as
    /// clear content.
    func parseKey(_ line: String, baseURL: URL) throws -> EncryptionKey? {
        let attributes = parseAttributeList(
            String(line.dropFirst("#EXT-X-KEY:".count))
        )
        guard let method = attributes["METHOD"] else {
            throw HLSExportError.invalidEncryptionKey
        }
        switch method.uppercased() {
        case "NONE":
            return nil
        case "AES-128":
            guard let uri = attributes["URI"] else {
                throw HLSExportError.invalidEncryptionKey
            }
            var explicitIV: Data?
            if let ivString = attributes["IV"] {
                var hex = ivString
                if hex.hasPrefix("0x") || hex.hasPrefix("0X") {
                    hex = String(hex.dropFirst(2))
                }
                if hex.count % 2 == 1 { hex = "0" + hex }
                var bytes = Data()
                var index = hex.startIndex
                while index < hex.endIndex {
                    let next = hex.index(index, offsetBy: 2)
                    if let byte = UInt8(hex[index..<next], radix: 16) {
                        bytes.append(byte)
                    }
                    index = next
                }
                if bytes.count == 16 {
                    explicitIV = bytes
                }
            }
            return EncryptionKey(
                url: resolvedURL(uri, base: baseURL),
                explicitIV: explicitIV
            )
        default:
            throw HLSExportError.unsupportedEncryptionMethod(method)
        }
    }

    func parseByteRangeSpec(_ line: String) -> HLSByteRangeSpec? {
        let value = line.replacingOccurrences(
            of: "#EXT-X-BYTERANGE:",
            with: ""
        )
        let parts = value.split(separator: "@")
        guard let length = Int(parts.first ?? "") else { return nil }
        let explicitOffset = parts.count > 1
            ? Int(parts[1])
            : nil
        return HLSByteRangeSpec(
            length: length,
            explicitOffset: explicitOffset
        )
    }

    /// Parses `#EXT-X-MAP:URI="init.mp4",BYTERANGE="720@0"`. The map keeps a
    /// snapshot of the current key; an AES-128-encrypted map must carry an
    /// explicit IV (the media-sequence default does not apply to maps).
    func parseInitializationSection(
        _ line: String,
        baseURL: URL,
        currentKey: EncryptionKey?
    ) throws -> HLSInitializationSection {
        let attributes = parseAttributeList(
            String(line.dropFirst("#EXT-X-MAP:".count))
        )
        guard let uri = attributes["URI"] else {
            throw HLSExportError.invalidInitializationSection
        }
        let byteRangeSpec: HLSByteRangeSpec?
        if let rawRange = attributes["BYTERANGE"] {
            let parts = rawRange.split(separator: "@")
            guard let length = Int(parts.first ?? "") else {
                throw HLSExportError.invalidByteRange
            }
            let explicitOffset = parts.count > 1
                ? Int(parts[1])
                : nil
            byteRangeSpec = HLSByteRangeSpec(
                length: length,
                explicitOffset: explicitOffset
            )
        } else {
            byteRangeSpec = nil
        }
        if let key = currentKey, !key.hasExplicitIV {
            throw HLSExportError.encryptedInitializationRequiresExplicitIV
        }
        return HLSInitializationSection(
            url: resolvedURL(uri, base: baseURL),
            byteRangeSpec: byteRangeSpec,
            key: currentKey
        )
    }

    /// Quote-aware attribute list parser: quoted values may contain commas.
    func parseAttributeList(
        _ rawValue: String
    ) -> [String: String] {
        var result: [String: String] = [:]
        var tokens: [String] = []
        var token = ""
        var isInsideQuotes = false

        for character in rawValue {
            if character == "\"" {
                isInsideQuotes.toggle()
                token.append(character)
            } else if character == ",", !isInsideQuotes {
                tokens.append(token)
                token = ""
            } else {
                token.append(character)
            }
        }
        if !token.isEmpty {
            tokens.append(token)
        }

        for token in tokens {
            guard let equals = token.firstIndex(of: "=") else {
                continue
            }
            let key = String(token[..<equals])
                .trimmingCharacters(in: .whitespaces)
                .uppercased()
            var value = String(token[token.index(after: equals)...])
                .trimmingCharacters(in: .whitespaces)
            if value.hasPrefix("\""), value.hasSuffix("\""),
               value.count >= 2 {
                value.removeFirst()
                value.removeLast()
            }
            result[key] = value
        }
        return result
    }

    /// Resolves a media-segment BYTERANGE spec. An implicit offset continues
    /// the previous *media* range of the same URL (RFC 8216 §4.4.4.2). Map
    /// ranges are independent and must not advance this cursor — vkpymusic
    /// PR #41 / RFC 8216 §4.4.4.5.
    func resolveByteRange(
        _ spec: HLSByteRangeSpec?,
        for url: URL,
        nextOffsetByURL: inout [URL: Int]
    ) throws -> Range<Int>? {
        guard let spec else { return nil }
        guard spec.length > 0 else {
            throw HLSExportError.invalidByteRange
        }

        let start = spec.explicitOffset
            ?? nextOffsetByURL[url]
            ?? 0
        guard start >= 0,
              start <= Int.max - spec.length else {
            throw HLSExportError.invalidByteRange
        }

        let end = start + spec.length
        nextOffsetByURL[url] = end
        return start..<end
    }

    /// Map BYTERANGE: omitted offset starts at byte 0 of the resource, never
    /// at the next media-segment cursor.
    func resolveMapByteRange(
        _ spec: HLSByteRangeSpec?
    ) throws -> Range<Int>? {
        guard let spec else { return nil }
        guard spec.length > 0 else {
            throw HLSExportError.invalidByteRange
        }
        let start = spec.explicitOffset ?? 0
        guard start >= 0,
              start <= Int.max - spec.length else {
            throw HLSExportError.invalidByteRange
        }
        return start..<(start + spec.length)
    }

    func resolvedURL(_ string: String, base: URL) -> URL {
        if let absolute = URL(string: string), absolute.scheme != nil {
            return absolute
        }
        // URL(string:relativeTo:) resolves per RFC 3986, handling ../
        // and other relative components that appendPathComponent breaks.
        return URL(string: string, relativeTo: base)
            ?? base.appendingPathComponent(string)
    }


    func choose(_ text: String, base: URL) -> String {
        let renditions = parseAudioRenditions(in: text, baseURL: base)
        let variants = parseVariants(in: text, baseURL: base)
        guard !variants.isEmpty || !renditions.isEmpty else { return "0 -" }
        if let chosen = chooseAudioPlaylist(variants: variants, renditions: renditions) {
            return "1 \(chosen.absoluteString)"
        }
        return "2 -"
    }
}
