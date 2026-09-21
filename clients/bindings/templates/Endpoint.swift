// Platform adapter only; validation and socket-address semantics live in Rust.
import Network

public extension Endpoint {
    static func ipv4(_ address: IPv4Address, port: NWEndpoint.Port) throws -> Endpoint {
        try ipv4(bytes: Array(address.rawValue), port: UInt32(port.rawValue))
    }
    static func ipv6(_ address: IPv6Address, port: NWEndpoint.Port, scopeID: UInt32? = nil) throws -> Endpoint {
        guard let inheritedScope = UInt32(exactly: address.interface?.index ?? 0) else {
            throw EthersyncError(description: "invalid IPv6 interface index")
        }
        return try ipv6(bytes: Array(address.rawValue), port: UInt32(port.rawValue), scopeId: scopeID ?? inheritedScope)
    }
    static func loopback(port: NWEndpoint.Port) throws -> Endpoint {
        try loopback(port: UInt32(port.rawValue))
    }
    static func anyIPv4(port: NWEndpoint.Port) throws -> Endpoint {
        try anyIpv4(port: UInt32(port.rawValue))
    }
}
public extension LeaderOptions {
    func bind(to endpoint: Endpoint) { bindEndpoint(endpoint: endpoint) }
}
public extension FollowerOptions {
    convenience init(endpoint: Endpoint) throws {
        self.init(raw: try checked { try EthersyncSys.follower_options_endpoint(endpoint.raw) })
    }
    func bind(to endpoint: Endpoint) { bindEndpoint(endpoint: endpoint) }
}
