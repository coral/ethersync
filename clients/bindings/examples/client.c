#include "ethersync-client.h"
#include "check.h"
#include <stdio.h>
int main(void) {
    EsResultCore created = es_core_new();
    CHECK(created.status == 0);
    EsCore core = created.value;
    EsResultReading read = es_core_read(core, 123);
    CHECK(read.status == 0 && read.value.fps_numerator == 30);
    EsResultTimecodeSnapshot captured = es_core_snapshot(core);
    CHECK(!captured.status);
    CHECK(!es_core_snapshot_into(core, captured.value).status);
    EsResultTimecodeSnapshot copied = es_timecode_snapshot_copy(captured.value);
    CHECK(!copied.status);
    CHECK(es_timecode_snapshot_read(copied.value, 123).value.accepted_observations == 0);
    const uint8_t bytes[] = {255};
    const EsBytes malformed = {bytes, 1};
    EsResultUnit bad = es_core_state(core, malformed, 123);
    CHECK(bad.status != 0 && bad.error);
    es_buffer_dispose(&bad.error);
    es_core_dispose(&core);
    CHECK(es_timecode_snapshot_read(copied.value, 123).value.frames == 0);
    es_timecode_snapshot_dispose(&copied.value);
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
    CHECK(!es_reader_snapshot_into(reader.value, captured.value).status);
    EsResultEndpointList endpoints = es_leader_local_endpoints(leader);
    CHECK(!endpoints.status);
    CHECK(es_endpoint_list_count(endpoints.value).value == 1);
    EsResultEndpoint first_endpoint = es_endpoint_list_get(endpoints.value, 0);
    CHECK(!first_endpoint.status);
    es_endpoint_dispose(&first_endpoint.value);
    es_endpoint_list_dispose(&endpoints.value);
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
    CHECK(synced.value.accepted_observations >= 12);
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
    CHECK(es_timecode_snapshot_read(captured.value, 123).value.frames == -7);
#endif
    es_timecode_snapshot_dispose(&captured.value);
    puts("C client passed");
}
