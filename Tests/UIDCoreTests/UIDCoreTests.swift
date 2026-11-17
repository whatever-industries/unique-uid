import Foundation
import Testing
@testable import UIDCore

private let original = AvidUID(0x1234_abcd_7654_fedc)
private let replacement = AvidUID(0xdefa_9876_4567_9abc)

@Test(arguments: [AvidKind.bin, .script], [false, true])
func patchesOnlyRootUID(kind: AvidKind, bigEndian: Bool) throws {
    let fixture = makeFixture(kind, bigEndian: bigEndian)
    let document = try AvidDocument(data: fixture.data, kind: kind)
    #expect(document.uid == original)
    #expect(document.uidOffset == fixture.offset)

    var patched = fixture.data
    document.replaceUID(in: &patched, with: replacement)
    let range = fixture.offset..<fixture.offset + 8
    let expected: [UInt8]
    switch (kind, bigEndian) {
    case (_, true): expected = [0xde, 0xfa, 0x98, 0x76, 0x45, 0x67, 0x9a, 0xbc]
    case (.bin, false): expected = [0xbc, 0x9a, 0x67, 0x45, 0x76, 0x98, 0xfa, 0xde]
    case (.script, false): expected = [0x76, 0x98, 0xfa, 0xde, 0xbc, 0x9a, 0x67, 0x45]
    }
    #expect(Array(patched[range]) == expected)
    #expect(patched[..<range.lowerBound] == fixture.data[..<range.lowerBound])
    #expect(patched[range.upperBound...] == fixture.data[range.upperBound...])
    #expect(try AvidDocument(data: patched, kind: kind).uid == replacement)
}

@Test
func rejectsMismatchedAndMalformedFiles() throws {
    let fixture = makeFixture(.script, bigEndian: false)
    #expect(throws: AvidFormatError.unexpectedRoot) {
        try AvidDocument(data: fixture.data, kind: .bin)
    }
    #expect(throws: AvidFormatError.truncated) {
        try AvidDocument(data: fixture.data.dropLast(), kind: .script)
    }
    var badSlates = fixture.data
    let slateCountOffset = fixture.offset - 12
    badSlates.replaceSubrange(slateCountOffset..<slateCountOffset + 4, with: [255, 255, 255, 255])
    #expect(throws: AvidFormatError.truncated) {
        try AvidDocument(data: badSlates, kind: .script)
    }
    var unknownRectangle = fixture.data
    let rectangleOffset = fixture.offset - 32
    unknownRectangle[rectangleOffset] = 7
    #expect(throws: AvidFormatError.unsupportedRootLayout) {
        try AvidDocument(data: unknownRectangle, kind: .script)
    }
}

@Test(arguments: [AvidKind.bin, .script])
func batchSkipsExistingNamesAndKeepsOriginal(kind: AvidKind) throws {
    try withScratchFolder { folder in
        let fixture = makeFixture(kind, bigEndian: false)
        let ext = kind.rawValue.uppercased()
        let source = folder.appendingPathComponent("Source.\(ext)")
        try fixture.data.write(to: source)
        let existing = folder.appendingPathComponent("Source Copy.\(ext)")
        let existingBytes = Data("existing copy".utf8)
        try existingBytes.write(to: existing)

        let batch = try AvidCopies.create(from: source, in: folder, count: 3, excluding: [replacement])
        #expect(batch.errorMessage == nil)
        #expect(batch.copies.map(\.url.lastPathComponent) == (2...4).map { "Source Copy \($0).\(ext)" })
        let assigned = Set(batch.copies.map(\.uid))
        #expect(assigned.count == 3)
        #expect(assigned.isDisjoint(with: [original, replacement, AvidUID(0)]))
        #expect(try Data(contentsOf: source) == fixture.data)
        #expect(try Data(contentsOf: existing) == existingBytes)
        for copy in batch.copies {
            let data = try Data(contentsOf: copy.url)
            #expect(try AvidDocument.read(from: copy.url).uid == copy.uid)
            #expect(data[..<fixture.offset] == fixture.data[..<fixture.offset])
            #expect(data[(fixture.offset + 8)...] == fixture.data[(fixture.offset + 8)...])
        }
        let names = try FileManager.default.contentsOfDirectory(atPath: folder.path)
        #expect(names.count == 5)
    }
}

private func makeFixture(_ kind: AvidKind, bigEndian: Bool) -> (data: Data, offset: Int) {
    func tag(_ value: String) -> [UInt8] {
        bigEndian ? Array(value.utf8) : Array(value.utf8.reversed())
    }

    func number(_ value: UInt64, width: Int) -> [UInt8] {
        let bytes = (0..<width).map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) }
        return bigEndian ? Array(bytes.reversed()) : bytes
    }

    let rootTag = kind == .bin ? "ABIN" : "Scpt"
    var data = Data(number(6, width: 2))
    data.append(contentsOf: "Domain".utf8)
    data.append(contentsOf: tag("OBJD"))
    data.append(contentsOf: number(7, width: 2))
    data.append(contentsOf: "AObjDoc".utf8)
    data.append(4)
    data.append(contentsOf: number(0, width: 2)) // Save date.
    data.append(contentsOf: number(3, width: 4))
    data.append(contentsOf: number(2, width: 4)) // Object 2 is the root.
    data.append(Data(repeating: 0, count: 38))

    func appendChunk(_ name: String, _ bytes: [UInt8]) {
        data.append(contentsOf: tag(name))
        data.append(contentsOf: number(UInt64(bytes.count), width: 4))
        data.append(contentsOf: bytes)
    }

    appendChunk(rootTag, [0xa0, 0xa1, 0xa2])
    let payloadStart = data.count + 8
    var payload: [UInt8]
    let offset: Int
    if kind == .bin {
        payload = [2, 14]
        payload += number(1, width: 4)
        offset = payloadStart + 6
        payload += number(original.value, width: 8)
    } else {
        payload = [2, 3]
        payload += number(1, width: 4) // Text.
        payload += Array(repeating: 0, count: 6) // Font.
        payload += number(1, width: 2) // Rectangle version.
        payload += Array(repeating: 0, count: 18) // Coordinates, scroll offsets and slate height.
        payload += number(2, width: 4)
        payload += number(1, width: 4)
        payload += number(3, width: 4)
        offset = payloadStart + 44
        payload += number(original.value >> 32, width: 4)
        payload += number(original.value & 0xffff_ffff, width: 4)
    }
    payload += [0xa3, 0xa4, 0x03]
    appendChunk(rootTag, payload)
    appendChunk("ATTR", [0xb0, 0xb1, 0xb2, 0xb3])
    return (data, offset)
}

private func withScratchFolder(_ action: (URL) throws -> Void) throws {
    let manager = FileManager.default
    let folder = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
    try manager.createDirectory(at: folder, withIntermediateDirectories: false)
    defer { try? manager.removeItem(at: folder) }
    try action(folder)
}
