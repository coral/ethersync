private func require(_ condition: Bool, _ message: String = "", line: UInt = #line) {
    if !condition {
        FileHandle.standardError.write(Data("Bonjour test failed at line \(line): \(message)\n".utf8))
        fatalError(message)
    }
}
// Compiled together with the generated convenience layer to exercise its private
// OS boundary without making test injection part of the public SDK.
private final class FakeBonjour {
    var next = 100
    var live = Set<DNSServiceRef>()
    var queues: [DNSServiceRef: DispatchQueue] = [:]
    var browses: [(DNSServiceRef, DNSServiceBrowseReply, UnsafeMutableRawPointer?)] = []
    var resolves: [(DNSServiceRef, DNSServiceResolveReply, UnsafeMutableRawPointer?)] = []
    var lookups: [(DNSServiceRef, DNSServiceGetAddrInfoReply, UnsafeMutableRawPointer?)] = []
    var registrations: [(DNSServiceRef, DNSServiceRegisterReply, UnsafeMutableRawPointer?)] = []
    var records: [(DNSServiceRef, DNSRecordRef, DNSServiceRegisterRecordReply, UnsafeMutableRawPointer?)] = []
    var scheduleError: Int32 = 0
    var registrationError: Int32 = 0
    func allocate(_ pointer: UnsafeMutablePointer<DNSServiceRef?>?) -> DNSServiceRef {
        next += 1
        let ref = DNSServiceRef(bitPattern: next)!
        pointer!.pointee = ref; live.insert(ref)
        return ref
    }
    func dns() -> BonjourDNS {
        var dns = BonjourDNS()
        dns.browse = { ref, _, _, type, domain, callback, box in
            require(String(cString: type!) == "_tidkod._udp" && String(cString: domain!) == "local.")
            self.browses.append((self.allocate(ref), callback!, box)); return 0
        }
        dns.resolve = { ref, _, _, _, _, _, callback, box in
            self.resolves.append((self.allocate(ref), callback!, box)); return 0
        }
        dns.addresses = { ref, _, _, _, _, callback, box in
            self.lookups.append((self.allocate(ref), callback!, box)); return 0
        }
        dns.connection = { ref in _ = self.allocate(ref); return 0 }
        dns.record = { ref, record, _, _, _, _, _, _, _, _, callback, box in
            self.next += 1
            let value = DNSRecordRef(bitPattern: self.next)!
            record!.pointee = value
            self.records.append((ref!, value, callback!, box)); return 0
        }
        dns.register = { ref, _, _, _, _, _, host, port, _, _, callback, box in
            require(String(cString: host!).hasPrefix("tidkod-") && port != 0)
            if self.registrationError != 0 { return self.registrationError }
            self.registrations.append((self.allocate(ref), callback!, box)); return 0
        }
        dns.schedule = { ref, queue in self.queues[ref!] = queue!; return self.scheduleError }
        dns.deallocate = { ref in
            if let queue = self.queues.removeValue(forKey: ref!) { dispatchPrecondition(condition: .onQueue(queue)) }
            require(self.live.remove(ref!) != nil, "double deallocation")
        }
        return dns
    }
    func browse(_ index: Int = 0, added: Bool = true, interface: UInt32 = 4, error: Int32 = 0) {
        let (ref, callback, box) = browses[index]
        callback(ref, added ? UInt32(kDNSServiceFlagsAdd) : 0, interface, error, "Test", "_tidkod._udp.", "local.", box)
    }
    func resolve(_ index: Int = 0, txt: Data) {
        let (ref, callback, box) = resolves[index]
        txt.withUnsafeBytes { callback(ref, 0, 4, 0, "Test._tidkod._udp.local.", "test.local.", UInt16(4443).bigEndian, UInt16($0.count), $0.bindMemory(to: UInt8.self).baseAddress, box) }
    }
    func address(_ index: Int = 0, added: Bool = true, v6: Bool = false, error: Int32 = 0) {
        let (ref, callback, box) = lookups[index]
        var a = sockaddr_in(), b = sockaddr_in6()
        a.sin_family = sa_family_t(AF_INET); a.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        b.sin6_family = sa_family_t(AF_INET6); b.sin6_len = UInt8(MemoryLayout<sockaddr_in6>.size)
        _ = inet_pton(AF_INET, "192.0.2.1", &a.sin_addr)
        _ = inet_pton(AF_INET6, "fe80::1", &b.sin6_addr)
        let flags = added ? UInt32(kDNSServiceFlagsAdd) : 0
        if v6 {
            withUnsafePointer(to: &b) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { callback(ref, flags, 4, error, "test.local.", $0, 120, box) } }
        } else {
            withUnsafePointer(to: &a) { $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { callback(ref, flags, 4, error, "test.local.", $0, 120, box) } }
        }
    }
    func ready() {
        for (ref, record, callback, box) in records where live.contains(ref) { callback(ref, record, 0, 0, box) }
        for (ref, callback, box) in registrations where live.contains(ref) { callback(ref, UInt32(kDNSServiceFlagsAdd), 0, "Test", "_tidkod._udp", "local.", box) }
    }
}

@main private struct BonjourTests {
    static let fields = ["id": "one", "name": "Test 🎬", "fp": String(repeating: "ab", count: 32), "v": "1", "transport": "moq-lite-05"]
    static func expectFailure(_ body: () throws -> Void) {
        do { try body(); fatalError("expected failure") } catch { }
    }
    static func wait(_ message: String, _ predicate: () throws -> Bool) throws {
        let deadline = Date().addingTimeInterval(18)
        while try !predicate() {
            guard Date() < deadline else { throw TidkodError(description: "Timed out: \(message)") }
            Thread.sleep(forTimeInterval: 0.05)
        }
    }
    static func main() {
        do { try run() }
        catch {
            FileHandle.standardError.write(Data("Bonjour tests failed: \(error)\n".utf8))
            exit(1)
        }
    }
    static func run() throws {
        let txt = try BonjourTXT.encode(fields)
        require(BonjourTXT.decode(txt) == fields)
        require(BonjourTXT.decode(Data([20, 1])) == nil)
        require(BonjourTXT.decode(Data([4]) + Data("ID=a".utf8) + Data([4]) + Data("id=b".utf8)) == nil)
        require(BonjourTXT.decode(Data([2]) + Data("id".utf8) + Data([4]) + Data("id=b".utf8)) == nil)
        expectFailure { _ = try BonjourTXT.encode(["name": String(repeating: "🎬", count: 64)]) }
        var bad = fields; bad["fp"] = "invalid"
        require(BonjourEntry(txt: try BonjourTXT.encode(bad), fallbackName: "", addresses: []) == nil)
        bad = fields; bad["transport"] = "other"
        require(BonjourEntry(txt: try BonjourTXT.encode(bad), fallbackName: "", addresses: []) == nil)
        let entry = BonjourEntry(txt: txt, fallbackName: "", addresses: ["192.0.2.1:4443"])!
        var another = entry; another.addresses = ["[fe80::1%4]:4443"]
        var cache = BonjourCache(services: ["a": entry, "b": another])
        require(cache.snapshot().count == 1 && cache.snapshot()[0].addresses.count == 2)
        bad = fields; bad["id"] = "two"
        cache.services["c"] = BonjourEntry(txt: try BonjourTXT.encode(bad), fallbackName: "", addresses: ["192.0.2.2:4443"])
        require(cache.snapshot().count == 2, "same name must not merge different identities")

        let engine = try Engine()
        let fake = FakeBonjour(), context: BonjourContext
        context = BonjourContext(dns: fake.dns())
        let discovery = try Discovery(context: context, engine: engine.raw, interfaces: [])
        try context.sync {
            fake.browse(); fake.resolve(txt: Data())
            require(discovery.poll() == 0 && discovery.status == .ready, "empty TXT must be ignored")
            fake.resolve(txt: txt); fake.address(); fake.address(v6: true)
            fake.address(error: Int32(kDNSServiceErr_NoSuchRecord))
            require(discovery.poll() == 1 && discovery.status == .ready)
            require(try discovery.address(index: 0, addressIndex: 1) == "[fe80::1%4]:4443")
            _ = try FollowerOptions.discovered(discovery: discovery, index: 0, addressIndex: 1)
            fake.browse(added: false)
            require(try discovery.name(index: 0) == "Test 🎬", "indices must remain stable until poll")
            require(discovery.poll() == 0)
            require(fake.live.count == 1, "removed service must cancel both resolve handles")
            fake.browse(error: Int32(kDNSServiceErr_PolicyDenied))
            if case .failed(let error) = discovery.status { require(error.isPermissionDenied) } else { fatalError("permission error hidden") }
            require(fake.live.isEmpty)
        }
        try discovery.shutdown(); context.shutdown()
        expectFailure { _ = try Discovery(context: context, engine: engine.raw, interfaces: []) }
        let failed = FakeBonjour(); failed.scheduleError = Int32(kDNSServiceErr_BadParam)
        expectFailure { _ = try Discovery(context: BonjourContext(dns: failed.dns()), engine: engine.raw, interfaces: []) }
        require(failed.live.isEmpty, "partial setup leaked")
        expectFailure { _ = try Discovery(context: BonjourContext(), engine: engine.raw, interfaces: ["tidkod-no-such-interface"]) }

        let options = LeaderOptions()
        options.name(name: "Bonjour test")
        let raw = try TidkodSys.engine_leader_external_discovery(engine.raw, options.raw)
        let leader = Leader(raw: raw)
        let metadata = try BonjourAdvertisementMetadata(leader.advertisement())
        let local: [BonjourLocalAddress] = [
            .init(name: "en0", index: 4, ip: BonjourIP("192.0.2.1")!),
            .init(name: "en0", index: 4, ip: BonjourIP("fe80::1")!),
            .init(name: "lo0", index: 1, ip: BonjourIP("127.0.0.1")!)]
        require(metadata.records(local)[4] == [BonjourIP("192.0.2.1")!], "IPv4 listener advertised IPv6")
        let constrainedOptions = LeaderOptions()
        constrainedOptions.interface(name: "en0")
        try constrainedOptions.address(address: "192.0.2.9")
        let constrained = Leader(raw: try TidkodSys.engine_leader_external_discovery(engine.raw, constrainedOptions.raw))
        let constrainedMetadata = try BonjourAdvertisementMetadata(constrained.advertisement())
        require(constrainedMetadata.records(local) == [4: [BonjourIP("192.0.2.9")!]])
        let explicitOptions = LeaderOptions()
        try explicitOptions.address(address: "192.0.2.9")
        let explicit = Leader(raw: try TidkodSys.engine_leader_external_discovery(engine.raw, explicitOptions.raw))
        require(try BonjourAdvertisementMetadata(explicit.advertisement()).records(local).count == 1,
                "explicit physical address must not create an empty localhost service")
        try explicitOptions.bind(address: "127.0.0.1:0")
        let mismatch = Leader(raw: try TidkodSys.engine_leader_external_discovery(engine.raw, explicitOptions.raw))
        expectFailure { _ = try BonjourAdvertisementMetadata(mismatch.advertisement()) }
        let adFake = FakeBonjour()
        let adContext = BonjourContext(dns: adFake.dns(), localAddresses: { local })
        var ad = try adContext.advertise(leader.advertisement())
        require(ad!.status == .starting)
        adContext.sync { adFake.ready() }
        require(ad!.status == .ready)
        adContext.shutdown()
        require(ad!.status == .stopped && adFake.live.isEmpty)
        ad = nil
        let partial = FakeBonjour(); partial.registrationError = Int32(kDNSServiceErr_PolicyDenied)
        let partialContext = BonjourContext(dns: partial.dns(), localAddresses: { local })
        expectFailure { _ = try partialContext.advertise(leader.advertisement()) }
        require(partial.live.isEmpty, "address registration leaked after service setup failed")
        let dropping = FakeBonjour()
        let dropContext = BonjourContext(dns: dropping.dns(), localAddresses: { local })
        ad = try dropContext.advertise(leader.advertisement())
        require(!dropping.live.isEmpty)
        ad = nil
        require(dropping.live.isEmpty, "advertisement release must cancel timer and DNS refs")
        let asyncFailure = FakeBonjour()
        let asyncContext = BonjourContext(dns: asyncFailure.dns(), localAddresses: { local })
        ad = try asyncContext.advertise(leader.advertisement())
        asyncContext.sync {
            let (ref, callback, box) = asyncFailure.registrations[0]
            callback(ref, 0, Int32(kDNSServiceErr_PolicyDenied), nil, nil, nil, box)
        }
        if case .failed(let error) = ad!.status { require(error.isPermissionDenied) }
        else { fatalError("asynchronous advertisement error hidden") }
        require(asyncFailure.live.isEmpty)
        // An advertisement failure must not stop the Rust unicast leader.
        _ = try leader.reader()
        ad = nil
        let browserDrop = FakeBonjour()
        let browserContext = BonjourContext(dns: browserDrop.dns())
        var released: Discovery? = try Discovery(context: browserContext, engine: engine.raw, interfaces: [])
        require(released!.status == .ready)
        released = nil
        require(browserDrop.live.isEmpty)
        let hotplug = FakeBonjour()
        var currentAddresses = local
        let hotplugContext = BonjourContext(dns: hotplug.dns(), localAddresses: { currentAddresses })
        let moving = try hotplugContext.advertise(leader.advertisement())!
        hotplugContext.sync { hotplug.ready(); currentAddresses = [] }
        try wait("removed interfaces withdraw all records") {
            hotplugContext.sync { moving.status == .starting && hotplug.live.isEmpty }
        }
        hotplugContext.sync { currentAddresses = [local[0]] }
        try wait("new interface registers again") { hotplugContext.sync { !hotplug.live.isEmpty } }
        hotplugContext.sync { hotplug.ready() }
        require(moving.status == .ready)
        hotplugContext.shutdown()
        require(hotplug.live.isEmpty)
        options.advertise(enabled: false)
        let disabled = try engine.leader(options: options)
        require(disabled.advertisementStatus == .disabled)
        try engine.shutdown()
        print("Swift Bonjour deterministic tests passed")
        if let mode = ProcessInfo.processInfo.environment["TIDKOD_TEST_BONJOUR"], mode == "1" || mode == "apple" {
            try systemBonjour()
            if mode == "1" { try interoperability() }
        }
    }

    static func systemBonjour() throws {
        let engine = try Engine()
        defer { try? engine.shutdown() }
        let discovery = try engine.discovery()
        let options = LeaderOptions()
        let identity = "apple-" + UUID().uuidString
        options.identity(identity: identity)
        var leader: Leader? = try engine.leader(options: options)
        try wait("system Bonjour registration") {
            if case .failed(let error) = leader!.advertisementStatus { throw error }
            return leader!.advertisementStatus == .ready
        }
        var index: UInt32 = 0
        try wait("system Bonjour discovery") {
            if case .failed(let error) = discovery.status { throw error }
            for i in 0..<discovery.poll() {
                if try discovery.identity(index: i) == identity { index = i; return true }
            }
            return false
        }
        for i in 0..<(try discovery.addressCount(index: index)) {
            require(try !discovery.address(index: index, addressIndex: i).hasPrefix("["))
        }
        let follower = try engine.follower(options: FollowerOptions.discovered(discovery: discovery, index: index, addressIndex: 0))
        let reader = try follower.reader()
        try wait("system-discovered pinned QUIC follower") { reader.read().synchronization == .synchronized }
        leader = nil
        try wait("release withdraws system Bonjour registration") {
            for i in 0..<discovery.poll() { if try discovery.identity(index: i) == identity { return false } }
            return true
        }
        // Local-only IPv6 includes both ::1 and a scoped link-local address on
        // macOS, exercising multiple records in the same address RRset.
        var owner: Engine? = try Engine()
        let v6Options = LeaderOptions()
        try v6Options.bind(address: "[::]:0"); v6Options.interface(name: "lo0")
        let v6 = try owner!.leader(options: v6Options)
        let browseOptions = DiscoveryOptions(); browseOptions.interface(name: "lo0")
        let v6Discovery = try owner!.discoveryConfigured(options: browseOptions)
        owner = nil
        try wait("retained children continue advertising after Engine wrapper release") {
            if case .failed(let error) = v6.advertisementStatus { throw error }
            return v6.advertisementStatus == .ready
        }
        try wait("local-only IPv6 discovery") {
            for i in 0..<v6Discovery.poll() {
                if try v6Discovery.identity(index: i) == v6.advertisement().identity() { index = i; return true }
            }
            return false
        }
        let v6Follow = try engine.follower(options: FollowerOptions.discovered(discovery: v6Discovery, index: index, addressIndex: 0))
        let v6Reader = try v6Follow.reader()
        try wait("system-discovered IPv6 QUIC follower") { v6Reader.read().synchronization == .synchronized }
        try v6.shutdown(); try v6Discovery.shutdown()
        try engine.shutdown()
        require(discovery.status == .stopped)
        print("System Bonjour discovery, IPv4/IPv6 pinned QUIC, ownership, and withdrawal passed")
        fflush(stdout)
    }

    static func interoperability() throws {
        let engine = try Engine()
        defer { try? engine.shutdown() }
        let swiftOptions = LeaderOptions()
        let swiftID = "swift-" + UUID().uuidString
        swiftOptions.identity(identity: swiftID); swiftOptions.name(name: "Bonjour interop")
        let swiftLeader = try engine.leader(options: swiftOptions)
        let rustDiscovery = try TidkodSys.engine_discovery(engine.raw)
        let swiftDiscovery = try engine.discovery()
        let rustOptions = LeaderOptions()
        let rustID = "rust-" + UUID().uuidString
        rustOptions.identity(identity: rustID); rustOptions.name(name: "Bonjour interop")
        let rustLeader = try TidkodSys.engine_leader(engine.raw, rustOptions.raw)
        try wait("Swift advertisement ready") {
            if case .failed(let error) = swiftLeader.advertisementStatus { throw error }
            return swiftLeader.advertisementStatus == .ready
        }
        var rustIndex: UInt32 = 0, swiftIndex: UInt32 = 0
        try wait("Rust discovers Swift") {
            for i in 0..<TidkodSys.discovery_poll(rustDiscovery) {
                if try TidkodSys.discovery_identity(rustDiscovery, i).toString() == swiftID { rustIndex = i; return true }
            }
            return false
        }
        for i in 0..<(try TidkodSys.discovery_address_count(rustDiscovery, rustIndex)) {
            let address = try TidkodSys.discovery_address(rustDiscovery, rustIndex, i).toString()
            require(!address.hasPrefix("["), "IPv4-only listener published IPv6: \(address)")
        }
        let rustFollow = try TidkodSys.follower_options_discovered(rustDiscovery, rustIndex, 0)
        let followerA = Follower(raw: try TidkodSys.engine_follower(engine.raw, rustFollow))
        let readerA = try followerA.reader()
        try wait("Swift discovers Rust") {
            for i in 0..<swiftDiscovery.poll() {
                if try swiftDiscovery.identity(index: i) == rustID { swiftIndex = i; return true }
            }
            return false
        }
        let followerB = try engine.follower(options: FollowerOptions.discovered(discovery: swiftDiscovery, index: swiftIndex, addressIndex: 0))
        let readerB = try followerB.reader()
        try wait("both discovered followers synchronize") { readerA.read().synchronization == .synchronized && readerB.read().synchronization == .synchronized }
        try swiftLeader.shutdown()
        try wait("Swift advertisement withdrawn from Rust") {
            for i in 0..<TidkodSys.discovery_poll(rustDiscovery) {
                if try TidkodSys.discovery_identity(rustDiscovery, i).toString() == swiftID { return false }
            }
            return true
        }
        try TidkodSys.leader_shutdown(rustLeader)
        try wait("Rust advertisement withdrawn from Swift") {
            for i in 0..<swiftDiscovery.poll() { if try swiftDiscovery.identity(index: i) == rustID { return false } }
            return true
        }
        try engine.shutdown()
        require(swiftDiscovery.status == .stopped)
        print("Swift ↔ Rust Bonjour discovery, pinned QUIC synchronization, and withdrawal passed")
    }
}
