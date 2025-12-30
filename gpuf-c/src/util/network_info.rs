use std::time::{Duration, Instant};

#[cfg(not(target_os = "android"))]
use std::net::UdpSocket;

#[cfg(not(target_os = "android"))]
use sysinfo::Networks;

#[cfg(not(target_os = "android"))]
pub struct SessionNetworkMonitor {
    networks: Networks,
    interface_name: String,
    session_total_rx: u64,
    session_total_tx: u64,
    last_rx: u64,
    last_tx: u64,
    start_time: Instant,
}

#[cfg(target_os = "android")]
pub struct SessionNetworkMonitor {
    interface_name: String,
    session_total_rx: u64,
    session_total_tx: u64,
    start_time: Instant,
}

#[cfg(not(target_os = "android"))]
fn detect_default_interface() -> Option<String> {
    // Try to detect via UDP socket connection
    if let Some(iface) = detect_via_udp_socket() {
        return Some(iface);
    }

    // Fallback: find the first non-loopback interface with an IP
    detect_first_active_interface()
}

#[cfg(not(target_os = "android"))]
fn detect_via_udp_socket() -> Option<String> {
    // Create a UDP socket and connect to an external address
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:53").ok()?;

    // Get the local address of the socket
    let local_addr = socket.local_addr().ok()?;

    // Iterate over all network interfaces to find the one containing this IP
    let networks = Networks::new_with_refreshed_list();
    for (iface_name, network_data) in &networks {
        for ip in network_data.ip_networks() {
            if ip.addr == local_addr.ip() {
                return Some(iface_name.clone());
            }
        }
    }
    None
}

#[cfg(not(target_os = "android"))]
fn detect_first_active_interface() -> Option<String> {
    use std::net::IpAddr;

    let networks = Networks::new_with_refreshed_list();
    for (iface_name, network_data) in &networks {
        // Skip loopback interfaces
        if iface_name.starts_with("lo") || iface_name == "Loopback Pseudo-Interface 1" {
            continue;
        }

        // Find first interface with a non-loopback IP
        for ip in network_data.ip_networks() {
            match ip.addr {
                IpAddr::V4(ipv4) if !ipv4.is_loopback() && !ipv4.is_link_local() => {
                    return Some(iface_name.clone());
                }
                IpAddr::V6(ipv6) if !ipv6.is_loopback() => {
                    return Some(iface_name.clone());
                }
                _ => continue,
            }
        }
    }
    None
}

#[cfg(not(target_os = "android"))]
impl SessionNetworkMonitor {
    pub fn new(interface_name: Option<&str>) -> Option<Self> {
        let mut networks = Networks::new_with_refreshed_list();
        networks.refresh(true);

        // Determine which interface to use
        let interface = if let Some(name) = interface_name {
            name.to_string()
        } else {
            // Try to detect default interface, return None if detection fails
            detect_default_interface()?
        };

        if let Some(network_data) = networks.get(&interface) {
            let initial_rx = network_data.total_received();
            let initial_tx = network_data.total_transmitted();

            Some(Self {
                networks,
                interface_name: interface.to_string(),
                session_total_rx: 0,
                session_total_tx: 0,
                last_rx: initial_rx,
                last_tx: initial_tx,
                start_time: Instant::now(),
            })
        } else {
            None
        }
    }

    pub fn refresh(&mut self) -> Option<(u64, u64)> {
        self.networks.refresh(true);

        if let Some(network_data) = self.networks.get(&self.interface_name) {
            let current_rx = network_data.total_received();
            let current_tx = network_data.total_transmitted();

            // Calculate the amount of data in this interval (handle counter reset)
            let rx_delta = if current_rx >= self.last_rx {
                current_rx - self.last_rx
            } else {
                // Counter reset, only calculate the part after the reset
                current_rx
            };

            let tx_delta = if current_tx >= self.last_tx {
                current_tx - self.last_tx
            } else {
                current_tx
            };

            // Update the session total values
            self.session_total_rx += rx_delta;
            self.session_total_tx += tx_delta;

            // Update the last values
            self.last_rx = current_rx;
            self.last_tx = current_tx;

            Some((rx_delta, tx_delta))
        } else {
            None
        }
    }

    pub fn get_session_stats(&self) -> (u64, u64, Duration) {
        (
            self.session_total_rx,
            self.session_total_tx,
            self.start_time.elapsed(),
        )
    }

    #[allow(dead_code)]
    pub fn reset_session(&mut self) {
        self.session_total_rx = 0;
        self.session_total_tx = 0;
        self.start_time = Instant::now();

        // Refresh the network data to get the current system counter baseline
        self.networks.refresh(true);

        if let Some(network_data) = self.networks.get(&self.interface_name) {
            self.last_rx = network_data.total_received();
            self.last_tx = network_data.total_transmitted();
        }
    }
}

#[cfg(target_os = "android")]
impl SessionNetworkMonitor {
    pub fn new(interface_name: Option<&str>) -> Option<Self> {
        let interface = interface_name.unwrap_or("android").to_string();

        Some(Self {
            interface_name: interface,
            session_total_rx: 0,
            session_total_tx: 0,
            start_time: Instant::now(),
        })
    }

    pub fn refresh(&mut self) -> Option<(u64, u64)> {
        // Android network monitoring not implemented - return zero values
        Some((0, 0))
    }

    pub fn get_session_stats(&self) -> (u64, u64, Duration) {
        (
            self.session_total_rx,
            self.session_total_tx,
            self.start_time.elapsed(),
        )
    }

    #[allow(dead_code)]
    pub fn reset_session(&mut self) {
        self.session_total_rx = 0;
        self.session_total_tx = 0;
        self.start_time = Instant::now();
    }
}

#[cfg(not(target_os = "android"))]
#[test]
fn test_detect_default_interface() {
    let interface = detect_default_interface();
    println!("interface: {:?}", interface);
    assert!(interface.is_some());
}
