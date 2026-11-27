//! The computer's LAN IPv4 addresses: the certificate's names and the
//! addresses a phone can type or scan.
use std::net::Ipv4Addr;

/// Interface name prefixes of container and VM bridges: listed last, because
/// a phone on the LAN cannot reach them.
const VIRTUAL_PREFIXES: &[&str] = &[
    "docker", "br-", "veth", "virbr", "vmnet", "vboxnet", "lxc", "lxd", "podman", "cni", "flannel",
    "kube",
];

/// Every IPv4 address of an interface that is up, except loopback,
/// link-local, multicast and broadcast; private LAN addresses first.
pub(super) fn lan_ipv4() -> Vec<Ipv4Addr> {
    let mut found: Vec<(String, Ipv4Addr)> = interface_addresses()
        .into_iter()
        .filter(|(_, address)| usable(*address))
        .collect();
    found.sort_by_key(|(name, address)| {
        let is_virtual = VIRTUAL_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix));
        (is_virtual, !address.is_private(), *address)
    });
    let mut addresses: Vec<Ipv4Addr> = Vec::with_capacity(found.len());
    for (_, address) in found {
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    addresses
}

fn usable(address: Ipv4Addr) -> bool {
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_link_local()
        || address.is_multicast()
        || address.is_broadcast())
}

/// (interface name, address) of every IPv4 address on an interface that is up.
#[cfg(target_os = "linux")]
fn interface_addresses() -> Vec<(String, Ipv4Addr)> {
    use std::ffi::CStr;

    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `head` is a valid out-pointer; on success getifaddrs stores a
    // list that stays valid until the single freeifaddrs call below.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Vec::new();
    }
    let mut addresses = Vec::new();
    let mut cursor = head;
    while !cursor.is_null() {
        // SAFETY: `cursor` is a non-null node of the list getifaddrs returned,
        // which is not freed until after this loop.
        let entry = unsafe { &*cursor };
        cursor = entry.ifa_next;
        let up = entry.ifa_flags & (libc::IFF_UP as libc::c_uint) != 0;
        if !up || entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
            continue;
        }
        // SAFETY: a non-null `ifa_addr` points to a socket address whose
        // family field is always readable.
        let family = unsafe { (*entry.ifa_addr).sa_family };
        if libc::c_int::from(family) != libc::AF_INET {
            continue;
        }
        // SAFETY: the family is AF_INET, so the address is a `sockaddr_in`.
        let socket_address = unsafe { &*entry.ifa_addr.cast::<libc::sockaddr_in>() };
        let address = Ipv4Addr::from(u32::from_be(socket_address.sin_addr.s_addr));
        // SAFETY: `ifa_name` is a non-null, NUL-terminated string owned by the list.
        let name = unsafe { CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        addresses.push((name, address));
    }
    // SAFETY: `head` came from a successful getifaddrs and is freed exactly once.
    unsafe { libc::freeifaddrs(head) };
    addresses
}

#[cfg(not(target_os = "linux"))]
fn interface_addresses() -> Vec<(String, Ipv4Addr)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::{lan_ipv4, usable};
    use std::net::Ipv4Addr;

    #[test]
    fn never_lists_loopback_or_link_local_addresses() {
        assert!(!usable(Ipv4Addr::LOCALHOST));
        assert!(!usable(Ipv4Addr::new(169, 254, 3, 4)));
        assert!(!usable(Ipv4Addr::UNSPECIFIED));
        assert!(usable(Ipv4Addr::new(192, 168, 1, 20)));
        assert!(usable(Ipv4Addr::new(100, 101, 1, 2)));
        for address in lan_ipv4() {
            assert!(usable(address), "{address}");
        }
    }
}
