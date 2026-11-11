//! The one simulated host the demo apps share.
//!
//! Every fixture seeds its domain from these tables, so the Files tree, the
//! Network view, Storage, Accounts and Home describe the same machine instead
//! of five unrelated fixtures.

pub const HOSTNAME: &str = "anthracite-demo";

pub struct Interface {
    pub name: &'static str,
    pub mac: &'static str,
    pub mtu: u32,
    pub up: bool,
    pub addresses: &'static [(&'static str, &'static str, u8)],
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

pub struct Mount {
    pub device: &'static str,
    pub mountpoint: &'static str,
    pub filesystem: &'static str,
    pub used_bytes: u64,
    pub total_bytes: u64,
}

pub struct Device {
    pub name: &'static str,
    pub path: &'static str,
    pub size_bytes: u64,
    pub model: &'static str,
    pub removable: bool,
    pub mounted: bool,
}

pub struct User {
    pub name: &'static str,
    pub uid: u32,
    pub gid: u32,
    pub home: &'static str,
    pub shell: &'static str,
    pub system: bool,
    pub locked: bool,
}

pub struct Group {
    pub name: &'static str,
    pub gid: u32,
    pub members: &'static [&'static str],
}

pub fn interfaces() -> &'static [Interface] {
    &[
        Interface {
            name: "lo",
            mac: "00:00:00:00:00:00",
            mtu: 65536,
            up: true,
            addresses: &[("ipv4", "127.0.0.1", 8), ("ipv6", "::1", 128)],
            rx_bytes: 1_048_576,
            tx_bytes: 1_048_576,
        },
        Interface {
            name: "eth0",
            mac: "02:42:ac:11:00:02",
            mtu: 1500,
            up: true,
            addresses: &[
                ("ipv4", "10.0.0.2", 24),
                ("ipv6", "fe80::42:acff:fe11:2", 64),
            ],
            rx_bytes: 12_582_912,
            tx_bytes: 3_145_728,
        },
        Interface {
            name: "wlan0",
            mac: "02:42:ac:11:00:03",
            mtu: 1500,
            up: false,
            addresses: &[("ipv4", "192.168.1.40", 24)],
            rx_bytes: 0,
            tx_bytes: 0,
        },
    ]
}

pub fn mounts() -> &'static [Mount] {
    &[
        Mount {
            device: "/dev/nvme0n1p1",
            mountpoint: "/boot/efi",
            filesystem: "vfat",
            used_bytes: 67_108_864,
            total_bytes: 536_870_912,
        },
        Mount {
            device: "/dev/nvme0n1p2",
            mountpoint: "/",
            filesystem: "ext4",
            used_bytes: 19_327_352_832,
            total_bytes: 51_539_607_552,
        },
        Mount {
            device: "/dev/sdb1",
            mountpoint: "/media/demo",
            filesystem: "exfat",
            used_bytes: 12_884_901_888,
            total_bytes: 68_719_476_736,
        },
    ]
}

pub fn devices() -> &'static [Device] {
    &[
        Device {
            name: "nvme0n1p1",
            path: "/dev/nvme0n1p1",
            size_bytes: 536_870_912,
            model: "Demo NVMe",
            removable: false,
            mounted: true,
        },
        Device {
            name: "nvme0n1p2",
            path: "/dev/nvme0n1p2",
            size_bytes: 51_539_607_552,
            model: "Demo NVMe",
            removable: false,
            mounted: true,
        },
        Device {
            name: "sdb1",
            path: "/dev/sdb1",
            size_bytes: 68_719_476_736,
            model: "Demo USB",
            removable: true,
            mounted: true,
        },
        Device {
            name: "sdc",
            path: "/dev/sdc",
            size_bytes: 15_931_064_320,
            model: "Demo SD",
            removable: true,
            mounted: false,
        },
    ]
}

pub fn users() -> &'static [User] {
    &[
        User {
            name: "root",
            uid: 0,
            gid: 0,
            home: "/root",
            shell: "/bin/sh",
            system: true,
            locked: false,
        },
        User {
            name: "daemon",
            uid: 1,
            gid: 1,
            home: "/usr/sbin",
            shell: "/usr/sbin/nologin",
            system: true,
            locked: true,
        },
        User {
            name: "operator",
            uid: 1000,
            gid: 1000,
            home: "/home/operator",
            shell: "/bin/sh",
            system: false,
            locked: false,
        },
        User {
            name: "backup",
            uid: 34,
            gid: 34,
            home: "/var/backups",
            shell: "/usr/sbin/nologin",
            system: true,
            locked: true,
        },
        User {
            name: "postgres",
            uid: 999,
            gid: 999,
            home: "/var/lib/postgresql",
            shell: "/bin/sh",
            system: false,
            locked: false,
        },
        User {
            name: "www-data",
            uid: 33,
            gid: 33,
            home: "/var/www",
            shell: "/usr/sbin/nologin",
            system: true,
            locked: true,
        },
    ]
}

pub fn groups() -> &'static [Group] {
    &[
        Group {
            name: "root",
            gid: 0,
            members: &[],
        },
        Group {
            name: "operator",
            gid: 1000,
            members: &["operator"],
        },
        Group {
            name: "sudo",
            gid: 27,
            members: &["operator"],
        },
        Group {
            name: "docker",
            gid: 998,
            members: &["operator"],
        },
        Group {
            name: "postgres",
            gid: 999,
            members: &["postgres"],
        },
        Group {
            name: "www-data",
            gid: 33,
            members: &["www-data"],
        },
    ]
}

/// Directories every stateful app should see, including the mountpoints and
/// the home directories owned by the accounts fixture.
pub fn directories() -> &'static [&'static str] {
    &[
        "/boot",
        "/boot/efi",
        "/media",
        "/media/demo",
        "/media/sd",
        "/root",
        "/usr",
        "/usr/sbin",
        "/var/backups",
        "/var/lib",
        "/var/lib/postgresql",
        "/var/www",
    ]
}
