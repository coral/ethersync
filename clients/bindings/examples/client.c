#include "ethersync-client.h"
#include <assert.h>
#include <stdio.h>
int main(void) {
    EsResultCore created = es_core_new();
    assert(created.status == 0);
    EsCore core = created.value;
    EsResultReading read = es_core_read(core, 123);
    assert(read.status == 0 && read.value.fps_numerator == 30);
    const uint8_t bytes[] = {255};
    EsResultUnit bad = es_core_state(core, (EsBytes){bytes, 1}, 123);
    assert(bad.status != 0 && bad.error);
    es_buffer_dispose(&bad.error);
    es_core_dispose(&core);
#ifdef ETHERSYNC_NATIVE
    EsResultEngine started = es_engine_new();
    if (started.status) { es_buffer_dispose(&started.error); return 1; }
    EsEngine engine = started.value;
    EsResultLeaderOptions config = es_leader_options_new();
    assert(!config.status);
    EsResultEndpoint endpoint = es_endpoint_loopback(0);
    assert(!endpoint.status);
    assert(!es_leader_options_bind_endpoint(config.value, endpoint.value).status);
    es_endpoint_dispose(&endpoint.value);
    assert(!es_leader_options_advertise(config.value, false).status);
    EsResultLeader created_leader = es_engine_leader(engine, config.value);
    assert(!created_leader.status);
    EsLeader leader = created_leader.value;
    assert(!es_leader_seek(leader, -7, 0x80000000u).status);
    EsResultReader reader = es_leader_reader(leader);
    assert(!reader.status);
    assert(es_reader_read(reader.value).value.frames == -7);
    es_reader_dispose(&reader.value);
    es_leader_dispose(&leader);
    es_leader_options_dispose(&config.value);
    assert(!es_engine_shutdown(engine).status);
    es_engine_dispose(&engine);
#endif
    puts("C client passed");
}
