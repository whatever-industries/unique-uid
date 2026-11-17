import Foundation

public enum AvidKind: String, Sendable {
    case bin = "avb"
    case script = "avc"

    public init(url: URL) throws {
        guard url.isFileURL, let kind = Self(rawValue: url.pathExtension.lowercased()) else {
            throw AvidFormatError.unsupportedFile
        }
        self = kind
    }
}

enum AvidFormatError: LocalizedError {
    case unsupportedFile
    case notRegularFile
    case invalidContainer
    case unexpectedRoot
    case unsupportedRootLayout
    case truncated

    var errorDescription: String? {
        switch self {
        case .unsupportedFile: "Choose an Avid bin (.avb) or script (.avc)."
        case .notRegularFile: "Choose a regular file."
        case .invalidContainer: "The file does not have a supported Avid document header."
        case .unexpectedRoot: "The document’s root object does not match its file extension."
        case .unsupportedRootLayout: "The Avid root object uses an unsupported layout."
        case .truncated: "The file has an incomplete or invalid Avid object."
        }
    }
}

public struct AvidUID: Hashable, Sendable, CustomStringConvertible {
    public let value: UInt64

    public init(_ value: UInt64) {
        self.value = value
    }

    public var description: String {
        String(format: "%016llX", value)
    }
}

public struct AvidDocument: Sendable {
    public let uid: AvidUID
    public let kind: AvidKind
    let uidOffset: Int
    private let byteOrder: ByteOrder

    public static func read(from url: URL) throws -> Self {
        try snapshot(from: url).document
    }

    static func snapshot(from url: URL) throws -> (document: Self, data: Data) {
        let kind = try AvidKind(url: url)
        let file = url.resolvingSymlinksInPath()
        let values = try file.resourceValues(forKeys: [.isRegularFileKey])
        guard values.isRegularFile == true else { throw AvidFormatError.notRegularFile }
        let data = try Data(contentsOf: file)
        return (try Self(data: data, kind: kind), data)
    }

    init(data: Data, kind: AvidKind) throws {
        var container = try Reader(data: data)
        guard try container.literal("Domain"),
              try container.tag("OBJD"),
              try container.prefixString("AObjDoc"),
              try container.number(1) == 4 else {
            throw AvidFormatError.invalidContainer
        }

        try container.skipString() // Save date.
        let objectCount = Int(try container.number(4))
        let rootIndex = Int(try container.number(4))
        try container.skip(20)
        try container.skipString() // Creator.
        try container.skip(16)
        guard rootIndex > 0, rootIndex <= objectCount, objectCount <= container.remaining / 8 else {
            throw AvidFormatError.truncated
        }

        var root: Reader?
        for index in 1...objectCount {
            let tag = try container.take(4)
            let size = Int(try container.number(4))
            let payloadStart = container.offset
            try container.skip(size)
            if index == rootIndex {
                let expectedTags = kind == .bin ? ["ABIN", "BINF"] : ["Scpt"]
                guard expectedTags.contains(where: { container.byteOrder.matches($0, tag: tag) }) else {
                    throw AvidFormatError.unexpectedRoot
                }
                guard size >= 3, data[container.offset - 1] == 3 else {
                    throw AvidFormatError.truncated
                }
                root = Reader(data: data, byteOrder: container.byteOrder,
                              offset: payloadStart, end: container.offset - 1)
            }
        }
        guard container.remaining == 0, var root else {
            throw AvidFormatError.truncated
        }

        guard try root.number(1) == 2 else { throw AvidFormatError.unsupportedRootLayout }
        let version = try root.number(1)
        switch kind {
        case .bin:
            // Bin view reference is immediately before the eight-byte UID.
            guard version == 0x0e || version == 0x0f else { throw AvidFormatError.unsupportedRootLayout }
            try root.skip(4)
        case .script:
            guard version == 2 || version == 3 else { throw AvidFormatError.unsupportedRootLayout }
            // Text reference, three font fields, versioned rectangle, scroll offsets and slate height.
            try root.skip(4 + 6)
            guard try root.number(2) == 1 else { throw AvidFormatError.unsupportedRootLayout }
            try root.skip(8 + 8 + 2)
            let slateCount = Int(try root.number(4))
            try root.skip(slateCount * 4)
        }

        uidOffset = root.offset
        self.kind = kind
        byteOrder = root.byteOrder
        switch kind {
        case .bin:
            uid = AvidUID(try root.number(8))
        case .script:
            let high = try root.number(4)
            let low = try root.number(4)
            uid = AvidUID((high << 32) | low)
        }
    }

    func replaceUID(in data: inout Data, with uid: AvidUID) {
        switch kind {
        case .bin:
            data.replaceSubrange(uidOffset..<uidOffset + 8, with: byteOrder.bytes(uid.value, count: 8))
        case .script:
            let high = byteOrder.bytes(uid.value >> 32, count: 4)
            let low = byteOrder.bytes(uid.value & 0xffff_ffff, count: 4)
            data.replaceSubrange(uidOffset..<uidOffset + 8, with: high + low)
        }
    }
}

private enum ByteOrder: Sendable {
    case little, big

    func matches(_ tagName: String, tag: Data.SubSequence) -> Bool {
        let bytes = Array(tagName.utf8)
        return tag.elementsEqual(self == .little ? Array(bytes.reversed()) : bytes)
    }

    func bytes(_ value: UInt64, count: Int) -> [UInt8] {
        let littleEndianBytes = (0..<count).map { UInt8(truncatingIfNeeded: value >> ($0 * 8)) }
        return self == .little ? littleEndianBytes : Array(littleEndianBytes.reversed())
    }
}

private struct Reader {
    let data: Data
    let byteOrder: ByteOrder
    private(set) var offset: Int
    let end: Int
    var remaining: Int { end - offset }

    init(data: Data) throws {
        guard data.count >= 2 else { throw AvidFormatError.invalidContainer }
        let byteOrder: ByteOrder
        switch (data[0], data[1]) {
        case (6, 0): byteOrder = .little
        case (0, 6): byteOrder = .big
        default: throw AvidFormatError.invalidContainer
        }
        self.init(data: data, byteOrder: byteOrder, offset: 2, end: data.endIndex)
    }

    init(data: Data, byteOrder: ByteOrder, offset: Int, end: Int) {
        self.data = data
        self.byteOrder = byteOrder
        self.offset = offset
        self.end = end
    }

    mutating func skip(_ count: Int) throws {
        guard count >= 0, count <= remaining else { throw AvidFormatError.truncated }
        offset += count
    }

    mutating func take(_ count: Int) throws -> Data.SubSequence {
        let start = offset
        try skip(count)
        return data[start..<offset]
    }

    mutating func number(_ count: Int) throws -> UInt64 {
        let start = offset
        try skip(count)
        var value: UInt64 = 0
        for index in 0..<count {
            let sourceIndex = byteOrder == .little ? offset - 1 - index : start + index
            value = (value << 8) | UInt64(data[sourceIndex])
        }
        return value
    }

    mutating func literal(_ text: String) throws -> Bool {
        try take(text.utf8.count).elementsEqual(text.utf8)
    }

    mutating func tag(_ text: String) throws -> Bool {
        byteOrder.matches(text, tag: try take(4))
    }

    mutating func prefixString(_ text: String) throws -> Bool {
        let count = Int(try number(2))
        return try take(count).elementsEqual(text.utf8)
    }

    mutating func skipString() throws {
        let count = Int(try number(2))
        try skip(count)
    }
}
