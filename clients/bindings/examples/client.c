#include "ethersync-client.h"
#include "check.h"
#include <stdio.h>
int main(void) {
    EsResultCore created = es_core_new();
    CHECK(created.status == 0);
    EsCore core = created.value;
    EsResultReading read = es_core_read(core, 123);
    CHECK(read.status == 0 && read.value.fps_numerator == 30);
    const uint8_t bytes[] = {255};
    EsResultUnit bad = es_core_state(core, (EsBytes){bytes, 1}, 123);
    CHECK(bad.status != 0 && bad.error);
    es_buffer_dispose(&bad.error);
    es_core_dispose(&core);
#ifdef ETHERSYNC_NATIVE
    EsResultEngine started = es_engine_new();
    if (started.status) { es_buffer_dispose(&started.error); return 1; }
    EsEngine engine = started.value;
    EsResultLeaderOptions config = es_leader_options_new();
    CHECK(!config.status);
    EsResultEndpoint endpoint = es_endpoint_loopback(0);
    CHECK(!endpoint.status);
    CHECK(!es_leader_options_bind_endpoint(config.value, endpoint.value).status);
    es_endpoint_dispose(&endpoint.value);
    CHECK(!es_leader_options_advertise(config.value, false).status);
    EsResultLeader created_leader = es_engine_leader(engine, config.value);
    CHECK(!created_leader.status);
    EsLeader leader = created_leader.value;
    CHECK(!es_leader_seek(leader, -7, 0x80000000u).status);
    EsResultReader reader = es_leader_reader(leader);
    CHECK(!reader.status);
    CHECK(es_reader_read(reader.value).value.frames == -7);
    EsResultEndpoint address = es_leader_endpoint(leader);
    CHECK(!address.status);
    EsResultFollowerOptions follow_options = es_follower_options_endpoint(address.value);
    CHECK(!follow_options.status);
    EsResultString fingerprint = es_leader_fingerprint(leader);
    CHECK(!fingerprint.status);
    EsBuffer *pin_error = NULL;
    CHECK(!ethersync_follower_options_pin(follow_options.value.raw,
        ethersync_buffer_data(fingerprint.value), ethersync_buffer_len(fingerprint.value), &pin_error));
    es_buffer_dispose(&fingerprint.value);
    EsResultFollower follower = es_engine_follower(engine, follow_options.value);
    CHECK(!follower.status);
    EsResultReader remote = es_follower_reader(follower.value);
    CHECK(!remote.status);
    unsigned attempts = 0;
    EsResultReading synced;
    do {
        synced = es_reader_read(remote.value);
        CHECK(!synced.status);
        if (synced.value.synchronization == 2) break;
        CHECK(++attempts < 1000);
        smoke_sleep();
    } while (1);
    CHECK(synced.value.frames == -7 && synced.value.subframe == 0x80000000u);
    CHECK(!es_follower_shutdown(follower.value).status);
    es_reader_dispose(&remote.value);
    es_follower_dispose(&follower.value);
    es_follower_options_dispose(&follow_options.value);
    es_endpoint_dispose(&address.value);
    es_reader_dispose(&reader.value);
    es_leader_dispose(&leader);
    es_leader_options_dispose(&config.value);
    CHECK(!es_engine_shutdown(engine).status);
    es_engine_dispose(&engine);
#endif
    puts("C client passed");
}
