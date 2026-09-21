#include "ethersync-client.hpp"
#include <cassert>
#include <iostream>
int main() {
    using namespace ethersync::client;
    auto core = Core::create();
    assert(core.read(123).fps_numerator == 30);
    auto bad = core.state({255}, 123);
    assert(!bad && !bad.error().empty());
#ifdef ETHERSYNC_NATIVE
    auto created = Engine::create();
    if (!created) { std::cerr << created.error(); return 1; }
    auto engine = created.take();
    auto options = LeaderOptions::create();
    options.advertise(false);
    auto endpoint = Endpoint::loopback(0);
    assert(endpoint);
    options.bind_endpoint(endpoint.value());
    auto started = engine.leader(options);
    assert(started);
    auto leader = started.take();
    auto reading = leader.reader();
    assert(reading);
    auto reader = reading.take();
    assert(leader.seek(-7, 0x80000000u));
    assert(reader.read().frames == -7);
    assert(engine.shutdown());
#endif
    std::cout << "C++ client passed\n";
}
