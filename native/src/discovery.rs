use crate::{Error, LeaderConfig, LeaderInfo, Result};
use std::{
    collections::BTreeMap,
    net::{IpAddr, SocketAddr, SocketAddrV6},
    time::Duration,
};
pub const SERVICE_TYPE: &str = "_ethersync._udp.local.";
#[derive(Clone, Debug, Default)]
pub struct DiscoveryConfig {
    /// Empty selects all interfaces.
    pub interfaces: Vec<String>,
    /// Empty advertises current interface addresses automatically.
    pub addresses: Vec<IpAddr>,
}
#[derive(Clone, Debug)]
pub struct DiscoveredLeader {
    pub service_name: String,
    pub identity: String,
    pub name: String,
    pub protocol_version: u32,
    pub addresses: Vec<SocketAddr>,
    pub fingerprint: String,
}
/// Poll returns a current owned list; removals/expiry replace prior entries by full service name.
pub struct Discovery {
    pub(crate) daemon: mdns_sd::ServiceDaemon,
    events: mdns_sd::Receiver<mdns_sd::ServiceEvent>,
    leaders: BTreeMap<String, DiscoveredLeader>,
}
// Tear down the daemon on any setup error; ServiceDaemon handles alone do not join it.
struct SetupGuard(Option<mdns_sd::ServiceDaemon>);
impl Drop for SetupGuard {
    fn drop(&mut self) {
        if let Some(d) = self.0.take() {
            let _ = d.shutdown();
        }
    }
}
fn daemon(c: &DiscoveryConfig) -> Result<mdns_sd::ServiceDaemon> {
    let d = mdns_sd::ServiceDaemon::new().map_err(err)?;
    let mut guard = SetupGuard(Some(d.clone()));
    d.set_ip_check_interval(2).map_err(err)?;
    if !c.interfaces.is_empty() {
        d.disable_interface(mdns_sd::IfKind::All).map_err(err)?;
        for name in &c.interfaces {
            d.enable_interface(mdns_sd::IfKind::Name(name.clone()))
                .map_err(err)?;
        }
    }
    guard.0 = None;
    Ok(d)
}
fn err(e: mdns_sd::Error) -> Error {
    Error::Discovery(e.to_string())
}
impl Discovery {
    pub fn new(config: DiscoveryConfig) -> Result<Self> {
        let daemon = daemon(&config)?;
        let mut guard = SetupGuard(Some(daemon.clone()));
        let events = daemon.browse(SERVICE_TYPE).map_err(err)?;
        guard.0 = None;
        Ok(Self {
            daemon,
            events,
            leaders: BTreeMap::new(),
        })
    }
    pub fn poll(&mut self) -> Vec<DiscoveredLeader> {
        while let Ok(event) = self.events.try_recv() {
            match event {
                mdns_sd::ServiceEvent::ServiceResolved(info) => {
                    let Some(identity) = info.get_property_val_str("id") else {
                        continue;
                    };
                    let Some(fingerprint) = info.get_property_val_str("fp") else {
                        continue;
                    };
                    if crate::transport::tls::validate_pin(fingerprint).is_err() {
                        continue;
                    }
                    let version = info
                        .get_property_val_str("v")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let addresses = info
                        .get_addresses()
                        .iter()
                        .filter_map(|ip| match ip {
                            mdns_sd::ScopedIp::V4(v) => {
                                Some(SocketAddr::new(IpAddr::V4(*v.addr()), info.get_port()))
                            }
                            mdns_sd::ScopedIp::V6(v) => Some(SocketAddr::V6(SocketAddrV6::new(
                                *v.addr(),
                                info.get_port(),
                                0,
                                v.scope_id().index,
                            ))),
                            _ => None,
                        })
                        .collect();
                    let entry = DiscoveredLeader {
                        service_name: info.get_fullname().into(),
                        identity: identity.into(),
                        name: info.get_property_val_str("name").unwrap_or(identity).into(),
                        protocol_version: version,
                        addresses,
                        fingerprint: fingerprint.into(),
                    };
                    self.leaders.insert(entry.service_name.clone(), entry);
                }
                mdns_sd::ServiceEvent::ServiceRemoved(_, name) => {
                    self.leaders.remove(&name);
                }
                _ => {}
            }
        }
        self.leaders.values().cloned().collect()
    }
    pub fn shutdown(&mut self) -> Result<()> {
        self.daemon.stop_browse(SERVICE_TYPE).map_err(err)?;
        self.leaders.clear();
        let rx = self.daemon.shutdown().map_err(err)?;
        rx.recv_timeout(Duration::from_secs(1))
            .map_err(|e| Error::Discovery(e.to_string()))?;
        Ok(())
    }
}
impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}
pub(crate) struct Advertisement {
    daemon: mdns_sd::ServiceDaemon,
    name: String,
}
impl Advertisement {
    pub fn new(c: &LeaderConfig, info: &LeaderInfo) -> Result<Self> {
        let daemon = daemon(&c.discovery)?;
        let mut guard = SetupGuard(Some(daemon.clone()));
        if let SocketAddr::V6(addr) = c.bind
            && addr.scope_id() != 0
        {
            daemon
                .disable_interface(mdns_sd::IfKind::All)
                .map_err(err)?;
            daemon
                .enable_interface(mdns_sd::IfKind::IndexV6(addr.scope_id()))
                .map_err(err)?;
        }
        let mut addresses = c.discovery.addresses.clone();
        if addresses.is_empty() && !c.bind.ip().is_unspecified() {
            addresses.push(c.bind.ip());
        }
        let props = [
            ("id", c.identity.as_str()),
            ("name", c.name.as_str()),
            ("v", "1"),
            ("fp", info.fingerprint.as_str()),
            ("transport", "moq-lite-05"),
        ];
        // Unique stable identity suffix permits duplicate display names without conflating leaders.
        let host = format!("ethersync-{}.local.", uuid::Uuid::from_bytes(info.session));
        let mut display_name = c.name.clone();
        while display_name.len() > 54 {
            display_name.pop();
        }
        let instance = format!(
            "{}-{}",
            display_name,
            &uuid::Uuid::from_bytes(info.session).to_string()[..8]
        );
        let mut service = mdns_sd::ServiceInfo::new(
            SERVICE_TYPE,
            &instance,
            &host,
            addresses.as_slice(),
            info.address.port(),
            &props[..],
        )
        .map_err(err)?;
        if addresses.is_empty() {
            service = service.enable_addr_auto();
            // An IPv4 wildcard listens on every IPv4 interface, not on IPv6.
            // Keep automatic publication in sync with the listener while still
            // tracking both Ethernet and Wi-Fi (and later interface changes).
            if c.bind.is_ipv4() {
                service.set_interfaces(vec![mdns_sd::IfKind::IPv4]);
            }
        }
        let name = service.get_fullname().to_owned();
        daemon.register(service).map_err(err)?;
        guard.0 = None;
        Ok(Self { daemon, name })
    }
}
impl Drop for Advertisement {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.name);
        let _ = self.daemon.shutdown();
    }
}
