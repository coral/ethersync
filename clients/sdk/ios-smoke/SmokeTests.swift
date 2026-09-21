import Ethersync
import Network
import XCTest

final class SmokeTests: XCTestCase {
    func testPackagedSDK() throws {
        let core = Core()
        XCTAssertEqual(core.read(nowNs: 123).fpsNumerator, 30)
        XCTAssertThrowsError(try core.state(bytes: [255], nowNs: 123))
        let engine = try Engine()
        defer { try? engine.shutdown() }
        let options = LeaderOptions()
        options.advertise(enabled: false)
        options.bind(to: try .loopback(port: .any))
        let leader = try engine.leader(options: options)
        try leader.seek(frames: -7, subframe: 0x80000000)
        let follow = try FollowerOptions(endpoint: leader.endpoint())
        follow.pin(fingerprint: leader.fingerprint())
        let follower = try engine.follower(options: follow)
        defer { try? follower.shutdown() }
        let reader = try follower.reader()
        let deadline = Date().addingTimeInterval(10)
        while reader.read().synchronization != .synchronized {
            if Date() >= deadline { XCTFail("Follower did not synchronize"); return }
            Thread.sleep(forTimeInterval: 0.01)
        }
        let reading = reader.read()
        XCTAssertEqual(reading.frames, -7)
        XCTAssertEqual(reading.subframe, 0x80000000)
    }
}
