#include "ethersync-client.hpp"
#include "check.h"
#include <iostream>
int main() {
    namespace es = ethersync::client;
    auto core = es::Core::create();
    CHECK(core.read(123).fps_numerator == 30);
    auto snapshot = es::TimecodeSnapshot::create();
    core.snapshot_into(snapshot);
    const auto frozen = snapshot.copy();
    CHECK(frozen.read(123).accepted_observations == 0);
    CHECK(!frozen.next_boundary(123).valid);
    CHECK(frozen.read_for_presentation(123, 1));
    auto bad = core.state({255}, 123);
    CHECK(!bad && !bad.error().empty());
#ifdef ETHERSYNC_NATIVE
    auto created = es::Engine::create();
    if (!created) { std::cerr << created.error(); return 1; }
    auto engine = created.take();
    auto options = es::LeaderOptions::create();
    options.advertise(false);
    auto endpoint = es::Endpoint::loopback(0);
    CHECK(endpoint);
    options.bind_endpoint(endpoint.value());
    auto started = engine.leader(options);
    CHECK(started);
    auto leader = started.take();
    auto reading = leader.reader();
    CHECK(reading);
    auto reader = reading.take();
    CHECK(leader.seek(-7, 0x80000000u));
    CHECK(reader.read().frames == -7);
    CHECK(reader.read().subframe == 0x80000000u);
    reader.snapshot_into(snapshot);
    const auto leader_snapshot = reader.snapshot();
    auto endpoints = leader.local_endpoints();
    CHECK(endpoints && endpoints.value().count() == 1);
    CHECK(endpoints.value().get(0));
    CHECK(!endpoints.value().get(1));
    auto address = leader.endpoint();
    auto follow_options = es::FollowerOptions::endpoint(address);
    CHECK(follow_options);
    follow_options.value().pin(leader.fingerprint());
    auto started_follower = engine.follower(follow_options.value());
    CHECK(started_follower);
    auto follower = started_follower.take();
    auto remote_result = follower.reader();
    CHECK(remote_result);
    auto remote = remote_result.take();
    unsigned attempts = 0;
    while (remote.read().synchronization != 2) { CHECK(++attempts < 1000); smoke_sleep(); }
    auto synced = remote.read();
    CHECK(synced.frames == -7 && synced.subframe == 0x80000000u);
    CHECK(synced.accepted_observations >= 12);
    CHECK(follower.shutdown());
    CHECK(engine.shutdown());
    CHECK(leader_snapshot.read(123).frames == -7);
#endif
    std::cout << "C++ client passed\n";
}
