#include "tidkod.h"
#include "check.h"
#include <stdio.h>
#include <string.h>
int main(void) {
    Core *core = NULL;
    TKBuffer *error = NULL;
    CHECK(tidkod_core_new(&core, &error) == 0);
    Reading value;
    CHECK(tidkod_core_read(core, 123, &value, &error) == 0);
    CHECK(value.frames == 0 && value.fps_numerator == 30);
    TimecodeSnapshot *snapshot = NULL, *copy = NULL;
    CHECK(tidkod_core_snapshot(core, &snapshot, &error) == 0);
    CHECK(tidkod_timecode_snapshot_copy(snapshot, &copy, &error) == 0);
    CHECK(tidkod_core_snapshot_into(core, snapshot, &error) == 0);
    CHECK(tidkod_timecode_snapshot_read(snapshot, 123, &value, &error) == 0);
    CHECK(value.accepted_observations == 0);
    Boundary boundary;
    CHECK(tidkod_timecode_snapshot_next_boundary(snapshot, 123, &boundary, &error) == 0);
    CHECK(!boundary.valid);
    CHECK(tidkod_timecode_snapshot_read_for_presentation(snapshot, 123, 1, &value, &error) == 0);
    const uint8_t bad[] = {255};
    CHECK(tidkod_core_state(core, bad, sizeof(bad), 123, &error) != 0);
    CHECK(error && tidkod_buffer_len(error) > 0);
    tidkod_buffer_free(error);
    error = NULL;
    tidkod_core_free(core);
    CHECK(tidkod_timecode_snapshot_read(copy, 123, &value, &error) == 0);
    tidkod_timecodesnapshot_free(copy);
#ifdef TIDKOD_NATIVE
    Engine *engine = NULL;
    LeaderOptions *options = NULL;
    Leader *leader = NULL;
    Reader *reader = NULL;
    CHECK(tidkod_engine_new(&engine, &error) == 0);
    CHECK(tidkod_leader_options_new(&options, &error) == 0);
    CHECK(tidkod_leader_options_advertise(options, false, &error) == 0);
    const uint8_t bind_address[] = "127.0.0.1:0";
    CHECK(tidkod_leader_options_bind(options, bind_address, sizeof(bind_address)-1, &error) == 0);
    CHECK(tidkod_engine_leader(engine, options, &leader, &error) == 0);
    CHECK(tidkod_leader_reader(leader, &reader, &error) == 0);
    CHECK(tidkod_leader_seek(leader, -7, 0x80000000u, &error) == 0);
    CHECK(tidkod_reader_read(reader, &value, &error) == 0);
    CHECK(value.frames == -7 && value.subframe == 0x80000000u);
    CHECK(tidkod_reader_snapshot_into(reader, snapshot, &error) == 0);
    EndpointList *endpoints = NULL;
    Endpoint *endpoint = NULL;
    uint32_t count = 0;
    CHECK(tidkod_leader_local_endpoints(leader, &endpoints, &error) == 0);
    CHECK(tidkod_endpoint_list_count(endpoints, &count, &error) == 0 && count == 1);
    CHECK(tidkod_endpoint_list_get(endpoints, 0, &endpoint, &error) == 0);
    tidkod_endpoint_free(endpoint);
    CHECK(tidkod_endpoint_list_get(endpoints, count, &endpoint, &error) != 0);
    tidkod_buffer_free(error); error = NULL;
    tidkod_endpointlist_free(endpoints);
    CHECK(tidkod_engine_shutdown(engine, &error) == 0);
    tidkod_reader_free(reader);
    tidkod_leader_free(leader);
    tidkod_leaderoptions_free(options);
    tidkod_engine_free(engine);
    CHECK(tidkod_timecode_snapshot_read(snapshot, 123, &value, &error) == 0);
    CHECK(value.frames == -7);
#endif
    tidkod_timecodesnapshot_free(snapshot);
    puts("C bindings passed");
}
