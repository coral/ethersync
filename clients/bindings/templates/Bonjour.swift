// Apple system integration. Generated into the native convenience SDK only.
import Darwin
import Dispatch
import dnssd

public struct BonjourError: Error, Equatable, Sendable, CustomStringConvertible {
    public let operation: String
    public let code: Int32
    public var isPermissionDenied: Bool { code == kDNSServiceErr_PolicyDenied }
    public var description: String {
        isPermissionDenied ? "Local network permission denied (\(operation))." : "Bonjour \(operation) failed (\(code))."
    }
}
public enum BonjourStatus: Equatable, Sendable {
    case disabled, starting, ready, stopped
    case failed(BonjourError)
}

// Injectable OS boundary; production always uses Apple's system daemon.
struct BonjourDNS {
    var browse = DNSServiceBrowse
    var resolve = DNSServiceResolve
    var addresses = DNSServiceGetAddrInfo
    var register = DNSServiceRegister
    var connection = DNSServiceCreateConnection
    var record = DNSServiceRegisterRecord
    var schedule = DNSServiceSetDispatchQueue
    var deallocate = DNSServiceRefDeallocate
}
private final class BonjourCallback<T: AnyObject> {
    weak var owner: T?
    init(_ owner: T) { self.owner = owner }
    static func owner(_ pointer: UnsafeMutableRawPointer?) -> T? {
        guard let pointer else { return nil }
        return Unmanaged<BonjourCallback<T>>.fromOpaque(pointer).takeUnretainedValue().owner
    }
}
private final class BonjourHandle {
    let ref: DNSServiceRef
    let context: BonjourContext
    // Retain the callback box until reference destruction has completed on its queue.
    let callback: AnyObject?
    init(_ ref: DNSServiceRef, _ context: BonjourContext, _ callback: AnyObject? = nil) {
        self.ref = ref; self.context = context; self.callback = callback
    }
    deinit {
        let ref = ref, context = context, callback = callback
        context.sync { context.dns.deallocate(ref); withExtendedLifetime(callback) {} }
    }
}
private protocol BonjourOperation: AnyObject { func stopOnQueue() }
private final class BonjourWeakOperation {
    weak var value: (any BonjourOperation)?
    init(_ value: any BonjourOperation) { self.value = value }
}
final class BonjourContext {
    let queue = DispatchQueue(label: "se.tidkod.bonjour")
    private let key = DispatchSpecificKey<Bool>()
    let dns: BonjourDNS
    let localAddresses: () throws -> [BonjourLocalAddress]
    private var stopped = false
    private var operations: [BonjourWeakOperation] = []
    init(dns: BonjourDNS = BonjourDNS(), localAddresses: @escaping () throws -> [BonjourLocalAddress] = BonjourLocalAddress.current) {
        self.dns = dns; self.localAddresses = localAddresses
        queue.setSpecific(key: key, value: true)
    }
    func sync<T>(_ body: () throws -> T) rethrows -> T {
        if DispatchQueue.getSpecific(key: key) == true { return try body() }
        return try queue.sync(execute: body)
    }
    func checkRunning() throws {
        try sync { if stopped { throw TidkodError(description: "Engine is shut down") } }
    }
    fileprivate func own(_ operation: any BonjourOperation) throws {
        try checkRunning()
        operations.removeAll { $0.value == nil }
        operations.append(BonjourWeakOperation(operation))
    }
    func shutdown() {
        sync {
            stopped = true
            for operation in operations { operation.value?.stopOnQueue() }
            operations.removeAll()
        }
    }
    fileprivate func handle<T: AnyObject>(_ owner: T, operation: String,
        create: (UnsafeMutablePointer<DNSServiceRef?>, UnsafeMutableRawPointer) -> DNSServiceErrorType) throws -> BonjourHandle {
        let box = BonjourCallback(owner)
        var ref: DNSServiceRef?
        let code = create(&ref, Unmanaged.passUnretained(box).toOpaque())
        let handle = ref.map { BonjourHandle($0, self, box) }
        guard code == 0, let handle else { throw BonjourError(operation: operation, code: code == 0 ? Int32(kDNSServiceErr_BadReference) : code) }
        let result = dns.schedule(handle.ref, queue)
        guard result == 0 else { throw BonjourError(operation: "schedule \(operation)", code: result) }
        return handle
    }
    func advertise(_ info: LeaderAdvertisement) throws -> BonjourAdvertisement? {
        guard info.enabled() else { return nil }
        let metadata = try BonjourAdvertisementMetadata(info)
        return try sync {
            try checkRunning()
            let advertisement = BonjourAdvertisement(context: self, metadata: metadata)
            try own(advertisement)
            try advertisement.start()
            return advertisement
        }
    }
}

enum BonjourTXT {
    static let service = "_tidkod._udp"
    static func encode(_ fields: [String: String]) throws -> Data {
        var result = Data()
        for (key, value) in fields.sorted(by: { $0.key < $1.key }) {
            let bytes = Data("\(key)=\(value)".utf8)
            guard bytes.count <= 255 else { throw TidkodError(description: "Bonjour TXT field exceeds 255 bytes") }
            result.append(UInt8(bytes.count)); result.append(bytes)
        }
        guard result.count <= Int(UInt16.max) else { throw TidkodError(description: "Bonjour TXT record too long") }
        return result
    }
    static func decode(_ data: Data) -> [String: String]? {
        let bytes = Array(data)
        var i = 0, fields: [String: String] = [:]
        while i < bytes.count {
            let length = Int(bytes[i]); i += 1
            guard i + length <= bytes.count else { return nil }
            defer { i += length }
            guard length > 0, let text = String(bytes: bytes[i..<i+length], encoding: .utf8) else { continue }
            let equals = text.firstIndex(of: "=")
            let key = String(text[..<(equals ?? text.endIndex)]).lowercased()
            guard !key.isEmpty, fields[key] == nil else { return nil }
            fields[key] = equals.map { String(text[text.index(after: $0)...]) } ?? ""
        }
        return fields
    }
}
struct BonjourEntry: Equatable {
    let identity: String, name: String, fingerprint: String
    let version: UInt32
    var addresses: [String]
    init?(txt: Data, fallbackName: String, addresses: [String]) {
        guard let fields = BonjourTXT.decode(txt), let id = fields["id"], !id.isEmpty,
            let fp = fields["fp"], fp.utf8.count == 64,
            fp.utf8.allSatisfy({ (48...57).contains($0) || (65...70).contains($0) || (97...102).contains($0) }),
            fields["transport"] == "moq-lite-05", let v = fields["v"].flatMap(UInt32.init)
        else { return nil }
        identity = id; name = fields["name"] ?? fallbackName; fingerprint = fp.lowercased(); version = v
        self.addresses = addresses
    }
    static func addressOrder(_ a: String, _ b: String) -> Bool {
        if a.hasPrefix("[") != b.hasPrefix("[") { return !a.hasPrefix("[") }
        return a < b
    }
}
struct BonjourCache {
    var services: [String: BonjourEntry] = [:]
    func snapshot() -> [BonjourEntry] {
        // Merge observations of the same service identity, never display names.
        struct Key: Hashable { let identity: String; let fingerprint: String; let version: UInt32 }
        var merged: [Key: BonjourEntry] = [:]
        for entry in services.sorted(by: { $0.key < $1.key }).map(\.value) where !entry.addresses.isEmpty {
            let key = Key(identity: entry.identity, fingerprint: entry.fingerprint, version: entry.version)
            if var previous = merged[key] {
                previous.addresses = Array(Set(previous.addresses + entry.addresses)).sorted(by: BonjourEntry.addressOrder)
                merged[key] = previous
            } else { merged[key] = entry }
        }
        return merged.values.sorted { ($0.name, $0.identity, $0.fingerprint, $0.version) < ($1.name, $1.identity, $1.fingerprint, $1.version) }
    }
}

/// Own and access this handle on one execution context, just like other SDK handles.
/// Bonjour runs independently on a private serial queue; no main run loop is needed.
public final class Discovery {
    private let browser: BonjourBrowser
    // Match the foreign ABI's child ownership: dropping the Engine wrapper does not
    // shut down an engine still retained by a discovery handle.
    private let engine: TidkodSys.Engine
    private var entries: [BonjourEntry] = []
    fileprivate init(context: BonjourContext, engine: TidkodSys.Engine, interfaces: [String]) throws {
        self.engine = engine
        browser = try context.sync {
            try context.checkRunning()
            let browser = BonjourBrowser(context: context, interfaces: interfaces)
            try context.own(browser)
            try browser.start()
            return browser
        }
    }
    public var status: BonjourStatus { browser.context.sync { browser.state } }
    public func poll() -> UInt32 {
        entries = browser.context.sync { browser.cache.snapshot() }
        return UInt32(entries.count)
    }
    private func entry(_ index: UInt32) throws -> BonjourEntry {
        guard Int(index) < entries.count else { throw TidkodError(description: "discovery index") }
        return entries[Int(index)]
    }
    public func name(index: UInt32) throws -> String { try entry(index).name }
    public func identity(index: UInt32) throws -> String { try entry(index).identity }
    public func fingerprint(index: UInt32) throws -> String { try entry(index).fingerprint }
    public func version(index: UInt32) throws -> UInt32 { try entry(index).version }
    public func addressCount(index: UInt32) throws -> UInt32 { UInt32(try entry(index).addresses.count) }
    public func address(index: UInt32, addressIndex: UInt32) throws -> String {
        let item = try entry(index)
        guard Int(addressIndex) < item.addresses.count else { throw TidkodError(description: "address index") }
        return item.addresses[Int(addressIndex)]
    }
    fileprivate func resolvedOptions(index: UInt32, addressIndex: UInt32) throws -> FollowerOptions {
        let item = try entry(index)
        return try FollowerOptions.resolved(address: address(index: index, addressIndex: addressIndex), fingerprint: item.fingerprint, version: item.version)
    }
    public func shutdown() throws { browser.context.sync { browser.stopOnQueue() }; entries.removeAll() }
}
private final class BonjourBrowser: BonjourOperation {
    let context: BonjourContext
    let interfaces: [String]
    var state: BonjourStatus = .starting
    var cache = BonjourCache()
    private var handles: [BonjourHandle] = []
    private var resolvers: [String: BonjourResolver] = [:]
    init(context: BonjourContext, interfaces: [String]) { self.context = context; self.interfaces = interfaces }
    deinit { context.sync { stopOnQueue() } }
    func start() throws {
        do {
            for index in try BonjourLocalAddress.interfaceIndices(interfaces) {
                let handle = try context.handle(self, operation: "browse") { ref, box in
                    context.dns.browse(ref, 0, index, BonjourTXT.service, "local.", { _, flags, interface, code, name, type, domain, box in
                        guard let owner = BonjourCallback<BonjourBrowser>.owner(box), owner.state != .stopped else { return }
                        guard code == 0, let name, let type, let domain else { owner.fail(BonjourError(operation: "browse", code: code)); return }
                        owner.found(name: String(cString: name), type: String(cString: type), domain: String(cString: domain), interface: interface, added: flags & UInt32(kDNSServiceFlagsAdd) != 0)
                    }, box)
                }
                handles.append(handle)
            }
            state = .ready
        } catch { stopOnQueue(); throw error }
    }
    func stopOnQueue() { handles.removeAll(); resolvers.removeAll(); cache.services.removeAll(); state = .stopped }
    func fail(_ error: BonjourError) { stopOnQueue(); state = .failed(error) }
    private func found(name: String, type: String, domain: String, interface: UInt32, added: Bool) {
        let key = "\(name).\(type)\(domain)|\(interface)"
        if !added { resolvers.removeValue(forKey: key); cache.services.removeValue(forKey: key); return }
        guard resolvers[key] == nil else { return }
        let resolver = BonjourResolver(context: context, name: name, type: type, domain: domain, interface: interface)
        resolver.update = { [weak self] in self?.cache.services[key] = $0 }
        resolver.failure = { [weak self] in self?.fail($0) }
        resolvers[key] = resolver
        do { try resolver.start() } catch { fail(error as? BonjourError ?? BonjourError(operation: "resolve", code: Int32(kDNSServiceErr_Unknown))) }
    }
}
private final class BonjourResolver {
    let context: BonjourContext
    let name: String, type: String, domain: String
    let interface: UInt32
    var update: ((BonjourEntry?) -> Void)?
    var failure: ((BonjourError) -> Void)?
    private var resolve: BonjourHandle?, address: BonjourHandle?
    private var txt = Data(), port: UInt16 = 0
    private var endpoints = Set<String>()
    init(context: BonjourContext, name: String, type: String, domain: String, interface: UInt32) {
        self.context = context; self.name = name; self.type = type; self.domain = domain; self.interface = interface
    }
    deinit { context.sync { resolve = nil; address = nil } }
    func start() throws {
        resolve = try context.handle(self, operation: "resolve") { ref, box in
            context.dns.resolve(ref, 0, interface, name, type, domain, { _, _, interface, code, _, host, port, length, txt, box in
                guard let owner = BonjourCallback<BonjourResolver>.owner(box) else { return }
                guard code == 0, let host else { owner.failure?(BonjourError(operation: "resolve", code: code)); return }
                let data = txt.map { Data(bytes: $0, count: Int(length)) } ?? Data()
                owner.resolved(host: String(cString: host), port: UInt16(bigEndian: port), txt: data, interface: interface)
            }, box)
        }
    }
    private func resolved(host: String, port: UInt16, txt: Data, interface: UInt32) {
        address = nil; endpoints.removeAll(); update?(nil)
        guard port != 0, BonjourEntry(txt: txt, fallbackName: name, addresses: []) != nil else { return }
        self.txt = txt; self.port = port
        do {
            address = try context.handle(self, operation: "resolve addresses") { ref, box in
                context.dns.addresses(ref, 0, interface, UInt32(kDNSServiceProtocol_IPv4 | kDNSServiceProtocol_IPv6), host, { _, flags, interface, code, _, address, _, box in
                    guard let owner = BonjourCallback<BonjourResolver>.owner(box) else { return }
                    // An IPv4-only target can have no AAAA record (and vice versa).
                    // Keep the continuous query alive for the supported family.
                    if code == kDNSServiceErr_NoSuchRecord { return }
                    guard code == 0, let address else { owner.failure?(BonjourError(operation: "resolve addresses", code: code)); return }
                    guard let ip = BonjourIP(address) else { return }
                    let scope = ip.linkLocal ? (BonjourIP.scope(address) == 0 ? interface : BonjourIP.scope(address)) : 0
                    let endpoint = ip.endpoint(port: owner.port, scope: scope)
                    if flags & UInt32(kDNSServiceFlagsAdd) != 0 { owner.endpoints.insert(endpoint) } else { owner.endpoints.remove(endpoint) }
                    owner.update?(owner.endpoints.isEmpty ? nil : BonjourEntry(txt: owner.txt, fallbackName: owner.name, addresses: owner.endpoints.sorted(by: BonjourEntry.addressOrder)))
                }, box)
            }
        } catch { failure?(error as? BonjourError ?? BonjourError(operation: "resolve addresses", code: Int32(kDNSServiceErr_Unknown))) }
    }
}

struct BonjourIP: Hashable {
    let family: Int32
    let bytes: Data
    init?(_ text: String) {
        var v4 = in_addr(), v6 = in6_addr()
        if inet_pton(AF_INET, text, &v4) == 1 {
            family = AF_INET; bytes = withUnsafeBytes(of: v4) { Data($0) }
        } else if inet_pton(AF_INET6, text, &v6) == 1 {
            family = AF_INET6; bytes = withUnsafeBytes(of: v6) { Data($0) }
        } else { return nil }
    }
    init?(_ address: UnsafePointer<sockaddr>) {
        if address.pointee.sa_family == sa_family_t(AF_INET) {
            family = AF_INET
            bytes = address.withMemoryRebound(to: sockaddr_in.self, capacity: 1) { ptr in
                withUnsafeBytes(of: ptr.pointee.sin_addr) { Data($0) }
            }
        } else if address.pointee.sa_family == sa_family_t(AF_INET6) {
            family = AF_INET6
            bytes = address.withMemoryRebound(to: sockaddr_in6.self, capacity: 1) { ptr in
                withUnsafeBytes(of: ptr.pointee.sin6_addr) { Data($0) }
            }
        } else { return nil }
    }
    var host: String {
        var buffer = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
        bytes.withUnsafeBytes { _ = inet_ntop(family, $0.baseAddress, &buffer, socklen_t(buffer.count)) }
        return String(decoding: buffer.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }, as: UTF8.self)
    }
    var any: Bool { bytes.allSatisfy { $0 == 0 } }
    var linkLocal: Bool { family == AF_INET6 && bytes[0] == 0xfe && bytes[1] & 0xc0 == 0x80 }
    var loopback: Bool { family == AF_INET ? bytes[0] == 127 : bytes.dropLast().allSatisfy { $0 == 0 } && bytes.last == 1 }
    var multicast: Bool { family == AF_INET ? (224...239).contains(bytes[0]) : bytes[0] == 0xff }
    static func scope(_ address: UnsafePointer<sockaddr>) -> UInt32 {
        guard address.pointee.sa_family == sa_family_t(AF_INET6) else { return 0 }
        return address.withMemoryRebound(to: sockaddr_in6.self, capacity: 1) { $0.pointee.sin6_scope_id }
    }
    func endpoint(port: UInt16, scope: UInt32 = 0) -> String {
        family == AF_INET ? "\(host):\(port)" : "[\(host)\(scope == 0 ? "" : "%\(scope)")]:\(port)"
    }
}
struct BonjourLocalAddress: Hashable {
    let name: String
    let index: UInt32
    let ip: BonjourIP
    static func interfaceIndices(_ names: [String]) throws -> [UInt32] {
        if names.isEmpty { return [0] }
        return try Set(names).sorted().map {
            let index = if_nametoindex($0)
            guard index != 0 else { throw BonjourError(operation: "interface \($0)", code: Int32(kDNSServiceErr_BadInterfaceIndex)) }
            return $0 == "lo0" ? UInt32(kDNSServiceInterfaceIndexLocalOnly) : index
        }
    }
    static func current() throws -> [BonjourLocalAddress] {
        var list: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0 else { throw BonjourError(operation: "enumerate interfaces", code: Int32(kDNSServiceErr_Unknown)) }
        defer { freeifaddrs(list) }
        var result: [BonjourLocalAddress] = []
        var cursor = list
        while let item = cursor {
            defer { cursor = item.pointee.ifa_next }
            guard item.pointee.ifa_flags & UInt32(IFF_UP) != 0,
                let address = item.pointee.ifa_addr, let ip = BonjourIP(address), !ip.any, !ip.multicast,
                !(ip.family == AF_INET && ip.bytes.allSatisfy { $0 == 255 }) else { continue }
            let name = String(cString: item.pointee.ifa_name)
            result.append(.init(name: name, index: if_nametoindex(name), ip: ip))
        }
        return Array(Set(result))
    }
}
struct BonjourAdvertisementMetadata {
    let instance: String, hostname: String
    let txt: Data
    let bind: BonjourIP, scope: UInt32, port: UInt16
    let interfaces: [String], addresses: [BonjourIP]
    init(_ info: LeaderAdvertisement) throws {
        instance = info.instance(); hostname = info.hostname()
        txt = try BonjourTXT.encode(["name": info.name(), "id": info.identity(), "fp": info.fingerprint(), "v": "1", "transport": "moq-lite-05"])
        let endpoint = info.bindAddress()
        // Socket-address syntax was already validated by Rust; preserve its scope.
        let colon = endpoint.lastIndex(of: ":")!
        let host = String(endpoint[..<colon]).trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
        let parts = host.split(separator: "%")
        guard let bind = BonjourIP(String(parts[0])), let port = UInt16(endpoint[endpoint.index(after: colon)...]) else {
            throw TidkodError(description: "Invalid listener endpoint")
        }
        self.bind = bind; self.port = port; scope = parts.count == 2 ? UInt32(parts[1])! : 0
        interfaces = try (0..<info.interfaceCount()).map { try info.interface(index: $0) }
        addresses = try (0..<info.addressCount()).map {
            guard let ip = BonjourIP(try info.address(index: $0)), !ip.any, !ip.multicast,
                ip.family == bind.family, bind.any || ip == bind else {
                throw TidkodError(description: "Advertised address does not match the listener")
            }
            return ip
        }
    }
    func records(_ local: [BonjourLocalAddress]) -> [UInt32: Set<BonjourIP>] {
        var result: [UInt32: Set<BonjourIP>] = [:]
        for item in local {
            guard interfaces.isEmpty || interfaces.contains(item.name),
                scope == 0 || scope == item.index, item.ip.family == bind.family else { continue }
            let loopback = item.ip.loopback
            let index = loopback ? UInt32(kDNSServiceInterfaceIndexLocalOnly) : item.index
            if addresses.isEmpty {
                if bind.any || item.ip == bind { result[index, default: []].insert(item.ip) }
            } else {
                // Explicit addresses are caller supplied; a specific bind was
                // checked above. Never leak localhost onto a physical interface.
                let selected = addresses.filter { $0.loopback == loopback }
                if !selected.isEmpty { result[index, default: []].formUnion(selected) }
            }
        }
        return result
    }
}
final class BonjourAdvertisement: BonjourOperation {
    let context: BonjourContext
    let metadata: BonjourAdvertisementMetadata
    private var state: BonjourStatus = .starting
    private var timer: DispatchSourceTimer?
    private var groups: [UInt32: BonjourRegistration] = [:]
    var status: BonjourStatus { context.sync { state } }
    init(context: BonjourContext, metadata: BonjourAdvertisementMetadata) { self.context = context; self.metadata = metadata }
    deinit { stop() }
    func stop() { context.sync { stopOnQueue() } }
    func stopOnQueue() { timer?.cancel(); timer = nil; groups.removeAll(); state = .stopped }
    func start() throws {
        do {
            _ = try BonjourLocalAddress.interfaceIndices(metadata.interfaces)
            try refresh()
            let timer = DispatchSource.makeTimerSource(queue: context.queue)
            timer.schedule(deadline: .now() + 2, repeating: 2)
            timer.setEventHandler { [weak self] in
                guard let self else { return }
                do { try self.refresh() }
                catch { self.fail(error as? BonjourError ?? BonjourError(operation: "refresh addresses", code: Int32(kDNSServiceErr_Unknown))) }
            }
            self.timer = timer; timer.resume()
        } catch { stopOnQueue(); throw error }
    }
    private func refresh() throws {
        let wanted = metadata.records(try context.localAddresses())
        for index in Array(groups.keys) where wanted[index] != groups[index]?.ips { groups.removeValue(forKey: index) }
        for (index, ips) in wanted where groups[index] == nil {
            let group = BonjourRegistration(context: context, metadata: metadata, index: index, ips: ips)
            group.changed = { [weak self] in self?.updateStatus() }
            group.failure = { [weak self] in self?.fail($0) }
            try group.start()
            groups[index] = group
        }
        updateStatus()
    }
    private func updateStatus() { state = !groups.isEmpty && groups.values.allSatisfy(\.ready) ? .ready : .starting }
    private func fail(_ error: BonjourError) { stopOnQueue(); state = .failed(error) }
}
private final class BonjourRegistration {
    let context: BonjourContext, metadata: BonjourAdvertisementMetadata
    let index: UInt32, ips: Set<BonjourIP>
    var changed: (() -> Void)?, failure: ((BonjourError) -> Void)?
    private var connection: BonjourHandle?, service: BonjourHandle?
    private var accepted = Set<DNSRecordRef>()
    private var serviceReady = false
    var ready: Bool { serviceReady && accepted.count == ips.count }
    // Local-only records are also visible to this host's resolver. Give each
    // interface its own target so its address RRset cannot conflict with another
    // interface or leak localhost addresses into a physical-interface result.
    private var hostname: String { String(metadata.hostname.dropLast(".local.".count)) + "-\(index).local." }
    init(context: BonjourContext, metadata: BonjourAdvertisementMetadata, index: UInt32, ips: Set<BonjourIP>) {
        self.context = context; self.metadata = metadata; self.index = index; self.ips = ips
    }
    deinit { context.sync { service = nil; connection = nil } }
    func start() throws {
        // Use a per-leader SRV target, not the system hostname (which may publish
        // IPv6 for an IPv4-only QUIC listener). Apple owns all mDNS traffic.
        connection = try context.handle(self, operation: "create address registration") { ref, _ in context.dns.connection(ref) }
        // The connection's callback box has exactly this type and owner, and is
        // retained until every record on the connection has been deallocated.
        let pointer = Unmanaged.passUnretained(connection!.callback as! BonjourCallback<BonjourRegistration>).toOpaque()
        for ip in ips {
            var record: DNSRecordRef?
            let code = ip.bytes.withUnsafeBytes { bytes in
                context.dns.record(connection!.ref, &record, UInt32(kDNSServiceFlagsUnique), index, hostname,
                    UInt16(ip.family == AF_INET ? kDNSServiceType_A : kDNSServiceType_AAAA), UInt16(kDNSServiceClass_IN), UInt16(bytes.count), bytes.baseAddress, 0,
                    { _, record, _, code, box in
                        guard let owner = BonjourCallback<BonjourRegistration>.owner(box) else { return }
                        guard code == 0, let record else { owner.failure?(BonjourError(operation: "register address", code: code)); return }
                        owner.accepted.insert(record); owner.changed?()
                    }, pointer)
            }
            guard code == 0 else { throw BonjourError(operation: "register address", code: code) }
        }
        service = try context.handle(self, operation: "register service") { ref, box in
            metadata.txt.withUnsafeBytes { bytes in
                context.dns.register(ref, 0, index, metadata.instance, BonjourTXT.service, "local.", hostname, metadata.port.bigEndian, UInt16(bytes.count), bytes.baseAddress,
                    { _, flags, code, _, _, _, box in
                        guard let owner = BonjourCallback<BonjourRegistration>.owner(box) else { return }
                        guard code == 0 else { owner.failure?(BonjourError(operation: "register service", code: code)); return }
                        owner.serviceReady = flags & UInt32(kDNSServiceFlagsAdd) != 0
                        owner.changed?()
                    }, box)
            }
        }
    }
}
