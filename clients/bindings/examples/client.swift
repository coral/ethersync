import Foundation
#if ETHERSYNC_NATIVE
import Network
#endif

let core = Core()
precondition(core.read(nowNs: 123).fpsNumerator == 30)
do {
    try core.state(bytes: [255], nowNs: 123)
    fatalError("malformed input accepted")
} catch let error as EthersyncError {
    precondition(!error.description.isEmpty)
}
#if ETHERSYNC_NATIVE
let v6 = try Endpoint.ipv6(IPv6Address("fe80::1")!, port: 4443, scopeID: 7)
precondition(v6.address() == "[fe80::1%7]:4443")
let v4 = try Endpoint.ipv4(.loopback, port: 4443)
precondition(v4.address() == "127.0.0.1:4443")
do {
    _ = try FollowerOptions(endpoint: .loopback(port: .any))
    fatalError("zero destination port accepted")
} catch is EthersyncError { }
let engine = try Engine()
let options = LeaderOptions()
options.advertise(enabled: false)
options.bind(to: try .loopback(port: .any))
let leader = try engine.leader(options: options)
let reader = try leader.reader()
try leader.seek(frames: -7, subframe: 0x80000000)
precondition(reader.read().frames == -7)
let follow = try FollowerOptions(endpoint: leader.endpoint())
follow.pin(fingerprint: leader.fingerprint())
let follower = try engine.follower(options: follow)
let remote = try follower.reader()
let deadline = Date().addingTimeInterval(5)
while remote.read().synchronization != .synchronized {
    precondition(Date() < deadline)
    Thread.sleep(forTimeInterval: 0.01)
}
precondition(remote.read().subframe == 0x80000000)
print(remote.read().timecode)
try follower.shutdown()
try engine.shutdown()
#endif
print("Swift client passed")
