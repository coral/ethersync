#include "ethersync-client.hpp"
#include "check.h"
#include <iostream>
int main() {
    namespace es = ethersync::client;
    auto core = es::Core::create();
    CHECK(core.read(123).fps_numerator == 30);
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
    CHECK(follower.shutdown());
    CHECK(engine.shutdown());
#endif
    std::cout << "C++ client passed\n";
}
