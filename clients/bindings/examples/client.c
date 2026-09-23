#include "tidkod-client.h"
#include "check.h"
#include <stdio.h>
int main(void) {
    TKResultCore created = tk_core_new();
    CHECK(created.status == 0);
    TKCore core = created.value;
    TKResultReading read = tk_core_read(core, 123);
    CHECK(read.status == 0 && read.value.fps_numerator == 30);
    TKResultTimecodeSnapshot captured = tk_core_snapshot(core);
    CHECK(!captured.status);
    CHECK(!tk_core_snapshot_into(core, captured.value).status);
    TKResultTimecodeSnapshot copied = tk_timecode_snapshot_copy(captured.value);
    CHECK(!copied.status);
    CHECK(tk_timecode_snapshot_read(copied.value, 123).value.accepted_observations == 0);
    const uint8_t bytes[] = {255};
    const TKBytes malformed = {bytes, 1};
    TKResultUnit bad = tk_core_state(core, malformed, 123);
    CHECK(bad.status != 0 && bad.error);
    tk_buffer_dispose(&bad.error);
    tk_core_dispose(&core);
    CHECK(tk_timecode_snapshot_read(copied.value, 123).value.frames == 0);
    tk_timecode_snapshot_dispose(&copied.value);
#ifdef TIDKOD_NATIVE
    TKResultEngine started = tk_engine_new();
    if (started.status) { tk_buffer_dispose(&started.error); return 1; }
    TKEngine engine = started.value;
    TKResultLeaderOptions config = tk_leader_options_new();
    CHECK(!config.status);
    TKResultEndpoint endpoint = tk_endpoint_loopback(0);
    CHECK(!endpoint.status);
    CHECK(!tk_leader_options_bind_endpoint(config.value, endpoint.value).status);
    tk_endpoint_dispose(&endpoint.value);
    CHECK(!tk_leader_options_advertise(config.value, false).status);
    CHECK(!tk_leader_options_session_id(config.value, 0x1234, 0x5678).status);
    TKResultLeader created_leader = tk_engine_leader(engine, config.value);
    CHECK(!created_leader.status);
    TKLeader leader = created_leader.value;
    CHECK(!tk_leader_seek(leader, -7, 0x80000000u).status);
    TKResultReader reader = tk_leader_reader(leader);
    CHECK(!reader.status);
    CHECK(tk_reader_read(reader.value).value.frames == -7);
    Reading part = tk_reader_read(reader.value).value;
    CHECK(part.has_session_id && part.session_id_high == 0x1234 && part.session_id_low == 0x5678);
    TKResultSessionId rotated = tk_leader_rotate_session_id(leader);
    CHECK(!rotated.status);
    CHECK(tk_reader_read(reader.value).value.session_id_high == rotated.value.high);
    CHECK(!tk_leader_set_session_id(leader, 0x8765, 0x4321).status);
    CHECK(tk_reader_read(reader.value).value.session_id_low == 0x4321);
    CHECK(!tk_reader_snapshot_into(reader.value, captured.value).status);
    TKResultEndpointList endpoints = tk_leader_local_endpoints(leader);
    CHECK(!endpoints.status);
    CHECK(tk_endpoint_list_count(endpoints.value).value == 1);
    TKResultEndpoint first_endpoint = tk_endpoint_list_get(endpoints.value, 0);
    CHECK(!first_endpoint.status);
    tk_endpoint_dispose(&first_endpoint.value);
    tk_endpoint_list_dispose(&endpoints.value);
    TKResultEndpoint address = tk_leader_endpoint(leader);
    CHECK(!address.status);
    TKResultFollowerOptions follow_options = tk_follower_options_endpoint(address.value);
    CHECK(!follow_options.status);
    TKResultString fingerprint = tk_leader_fingerprint(leader);
    CHECK(!fingerprint.status);
    TKBuffer *pin_error = NULL;
    CHECK(!tidkod_follower_options_pin(follow_options.value.raw,
        tidkod_buffer_data(fingerprint.value), tidkod_buffer_len(fingerprint.value), &pin_error));
    tk_buffer_dispose(&fingerprint.value);
    TKResultFollower follower = tk_engine_follower(engine, follow_options.value);
    CHECK(!follower.status);
    TKResultReader remote = tk_follower_reader(follower.value);
    CHECK(!remote.status);
    unsigned attempts = 0;
    TKResultReading synced;
    do {
        synced = tk_reader_read(remote.value);
        CHECK(!synced.status);
        if (synced.value.synchronization == 2) break;
        CHECK(++attempts < 1000);
        smoke_sleep();
    } while (1);
    CHECK(synced.value.frames == -7 && synced.value.subframe == 0x80000000u);
    CHECK(synced.value.accepted_observations >= 12);
    CHECK(!tk_follower_shutdown(follower.value).status);
    tk_reader_dispose(&remote.value);
    tk_follower_dispose(&follower.value);
    tk_follower_options_dispose(&follow_options.value);
    tk_endpoint_dispose(&address.value);
    tk_reader_dispose(&reader.value);
    tk_leader_dispose(&leader);
    tk_leader_options_dispose(&config.value);
    CHECK(!tk_engine_shutdown(engine).status);
    tk_engine_dispose(&engine);
    CHECK(tk_timecode_snapshot_read(captured.value, 123).value.frames == -7);
#endif
    tk_timecode_snapshot_dispose(&captured.value);
    puts("C client passed");
}
