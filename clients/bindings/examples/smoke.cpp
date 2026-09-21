#include "ethersync.hpp"
#include <cassert>
#include <iostream>
int main() {
    using namespace ethersync;
    auto core = core_new();
    auto reading = core_read(*core, 123);
    assert(reading.frames == 0 && reading.fps_numerator == 30);
    const std::uint8_t bad[] = {255};
    auto error = core_state(*core, rust::Slice<const std::uint8_t>(bad, 1), 123);
    assert(!OutcomeUnit_ok(*error));
    assert(!OutcomeUnit_error(*error).empty());
#ifdef ETHERSYNC_NATIVE
    auto created = engine_new(); assert(OutcomeEngine_ok(*created));
    auto engine = OutcomeEngine_take(*created);
    auto options = leader_options_new();
    leader_options_advertise(*options, false);
    auto started = engine_leader(*engine, *options); assert(OutcomeLeader_ok(*started));
    auto leader = OutcomeLeader_take(*started);
    auto created_reader = leader_reader(*leader); assert(OutcomeReader_ok(*created_reader));
    auto reader = OutcomeReader_take(*created_reader);
    assert(OutcomeUnit_ok(*leader_seek(*leader, -7, 0x80000000u)));
    reading = reader_read(*reader);
    assert(reading.frames == -7 && reading.subframe == 0x80000000u);
    assert(OutcomeUnit_ok(*engine_shutdown(*engine)));
#endif
    std::cout << "C++ bindings passed (exceptions disabled)\n";
}
