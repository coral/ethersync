#include "ethersync.h"
#include "check.h"
#include <stdio.h>
#include <string.h>
int main(void) {
    Core *core = NULL;
    EsBuffer *error = NULL;
    CHECK(ethersync_core_new(&core, &error) == 0);
    Reading value;
    CHECK(ethersync_core_read(core, 123, &value, &error) == 0);
    CHECK(value.frames == 0 && value.fps_numerator == 30);
    const uint8_t bad[] = {255};
    CHECK(ethersync_core_state(core, bad, sizeof(bad), 123, &error) != 0);
    CHECK(error && ethersync_buffer_len(error) > 0);
    ethersync_buffer_free(error);
    ethersync_core_free(core);
#ifdef ETHERSYNC_NATIVE
    Engine *engine = NULL;
    LeaderOptions *options = NULL;
    Leader *leader = NULL;
    Reader *reader = NULL;
    CHECK(ethersync_engine_new(&engine, &error) == 0);
    CHECK(ethersync_leader_options_new(&options, &error) == 0);
    CHECK(ethersync_leader_options_advertise(options, false, &error) == 0);
    const uint8_t bind_address[] = "127.0.0.1:0";
    CHECK(ethersync_leader_options_bind(options, bind_address, sizeof(bind_address)-1, &error) == 0);
    CHECK(ethersync_engine_leader(engine, options, &leader, &error) == 0);
    CHECK(ethersync_leader_reader(leader, &reader, &error) == 0);
    CHECK(ethersync_leader_seek(leader, -7, 0x80000000u, &error) == 0);
    CHECK(ethersync_reader_read(reader, &value, &error) == 0);
    CHECK(value.frames == -7 && value.subframe == 0x80000000u);
    CHECK(ethersync_engine_shutdown(engine, &error) == 0);
    ethersync_reader_free(reader);
    ethersync_leader_free(leader);
    ethersync_leaderoptions_free(options);
    ethersync_engine_free(engine);
#endif
    puts("C bindings passed");
}
