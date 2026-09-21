# Leaders with Ethernet and Wi-Fi

Use `--bind 0.0.0.0:4443` for the Rust leader example, or the default
`LeaderConfig` with the desired port. The listener accepts simultaneous clients
through all local IPv4 addresses. Binding a specific interface address restricts
which destination IPs it accepts. `0.0.0.0` is a listen address, not a follower destination.

`leader.info().address` remains the actual bind endpoint, including wildcard IPs. Use
`leader.info().local_endpoints()?` for concrete local candidates with the listener's port.
A specific bind returns that endpoint. A wildcard enumerates active local unicast addresses
of the same IP family, sorted and deduplicated. Loopback and virtual interfaces are included;
loopback addresses are useful only on the same computer. IPv6 link-local addresses retain
their local interface scope and are omitted when a usable scope index is unavailable. A
remote follower must select its own local interface scope rather than copying the leader's
numeric scope ID. An IPv6 listener's possible platform-specific IPv4 dual-stack behavior is
not used to generate IPv4 candidates.

Each call performs a fresh OS enumeration and can fail; an empty list is also possible.
The result does not promise firewall/routing reachability, continued address assignment, or
listener liveness after shutdown. Discovery interface/address settings control advertisement
and do not filter the socket's local candidates. Use mDNS's resolved addresses for discovery
connections; use this helper for manual connection choices or listener diagnostics.

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
