# Leaders with Ethernet and Wi-Fi

Use `--bind 0.0.0.0:4443` for the Rust leader example, or the default
`LeaderConfig` with the desired port. The listener accepts simultaneous clients
through all local IPv4 addresses. Binding a specific interface address restricts
which destination IPs it accepts. `0.0.0.0` is a listen address, not a follower destination.

The QUIC socket preserves the destination IP on received packets and passes the
corresponding source IP on replies. Automatic discovery for an IPv4 wildcard
publishes IPv4 addresses across enabled interfaces and follows interface changes.
Explicit discovery addresses remain caller-controlled. IPv6 requires an IPv6 listener.

Discovered leaders can have multiple endpoints. Clients should try alternative
endpoints when one is unavailable, retaining the discovered certificate pin on
every attempt. The Swift app does this during initial connection and displays all
resolved addresses. An established follower retries its selected endpoint; this
does not promise seamless migration when an interface disappears.

For local multi-address regression testing, substitute two addresses assigned to
the test machine (Ethernet and Wi-Fi, or loopback and one LAN address):

```sh
ETHERSYNC_TEST_LOCAL_IPS=10.0.1.5,10.0.1.47 cargo test -p libethersync --locked --test loopback wildcard_listener_accepts_multiple_local_addresses -- --ignored --nocapture
cargo test -p libethersync --locked --test mdns -- --ignored --nocapture
```

The test connects concurrent pinned followers through each address to one
wildcard listener, checks synchronization and a shared transport update, then
reconnects them. Local testing verifies destination/source address handling but
does not validate firewall rules or routing between separate physical machines.
