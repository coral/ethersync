import Foundation
let core = core_new()
let initial = core_read(core, 123)
precondition(initial.frames == 0 && initial.fps_numerator == 30)
let bytes: [UInt8] = [255]
do {
    try bytes.withUnsafeBufferPointer { try core_state(core, $0, 123) }
    fatalError("malformed input accepted")
} catch { }
#if ETHERSYNC_NATIVE
let engine = try engine_new()
let options = leader_options_new()
leader_options_advertise(options, false)
try leader_options_bind(options, "127.0.0.1:0")
let leader = try engine_leader(engine, options)
let reader = try leader_reader(leader)
try leader_seek(leader, -7, 0x80000000)
let reading = reader_read(reader)
precondition(reading.frames == -7 && reading.subframe == 0x80000000)
do { _ = try follower_options_new("invalid address"); fatalError("invalid address accepted") } catch { }
let followerOptions = try follower_options_new(leader_address(leader))
follower_options_pin(followerOptions, leader_fingerprint(leader))
let follower = try engine_follower(engine, followerOptions)
let followerReader = try follower_reader(follower)
let limit = Date().addingTimeInterval(5)
while reader_read(followerReader).synchronization != 2 {
    precondition(Date() < limit, "follower did not synchronize")
    Thread.sleep(forTimeInterval: 0.01)
}
let followed = reader_read(followerReader)
precondition(followed.frames == -7 && followed.subframe == 0x80000000)
try follower_reconnect(follower)
try follower_shutdown(follower)
let discovery = try engine_discovery(engine)
_ = discovery_poll(discovery)
try discovery_shutdown(discovery)
try engine_shutdown(engine)
#endif
print("Swift bindings passed")
