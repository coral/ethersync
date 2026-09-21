import Foundation
#if ETHERSYNC_NATIVE
import Network
#endif

let core = Core()
precondition(core.read(nowNs: 123).fpsNumerator == 30)
let snapshot = TimecodeSnapshot()
core.snapshotInto(snapshot: snapshot)
let frozen = snapshot.copy()
precondition(frozen.read(nowNs: 123).acceptedObservations == 0)
precondition(!frozen.nextBoundary(nowNs: 123).valid)
let predicted = try frozen.readForPresentation(nowNs: 123, delayNs: 1)
precondition(predicted.frames == 0)
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
reader.snapshotInto(snapshot: snapshot)
let endpoints = try leader.localEndpoints()
precondition(endpoints.count() == 1)
let firstEndpoint = try endpoints.get(index: 0)
precondition(firstEndpoint.address() == leader.endpoint().address())
do { _ = try endpoints.get(index: 1); fatalError("out-of-range endpoint accepted") }
catch is EthersyncError { }
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
precondition(remote.read().acceptedObservations >= 12)
print(remote.read().timecode)
try follower.shutdown()
try engine.shutdown()
precondition(snapshot.read(nowNs: 123).frames == -7)
#endif
print("Swift client passed")
