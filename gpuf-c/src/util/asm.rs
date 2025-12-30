#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
#[allow(dead_code)]
pub fn pci_config_read(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    // On ARM, PCI configuration space is memory-mapped
    // The base address is platform-specific, here we use a common address for ARM
    // You may need to adjust this base address based on your specific ARM platform
    const PCI_CONFIG_BASE: u64 = 0x3F00_0000; // Example base address, adjust as needed

    // Calculate the offset in the configuration space
    let offset = ((bus as u32) << 16)
        | (((device as u32) & 0x1F) << 11)
        | (((func as u32) & 0x07) << 8)
        | ((offset as u32) & 0xFC);

    // Calculate the full address
    let address = (PCI_CONFIG_BASE + offset as u64) as *const u32;

    // Read the value using volatile read for MMIO
    unsafe { address.read_volatile() }
}

#[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
pub fn pci_config_read(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    use core::arch::asm;
    let address: u32 = 0x80000000
        | ((bus as u32) << 16)
        | (((device as u32) & 0x1F) << 11)
        | (((func as u32) & 0x07) << 8)
        | ((offset as u32) & 0xFC);

    let mut value: u32;

    unsafe {
        asm!(
            "out dx, eax",
            in("dx") 0xCF8u16,
            in("eax") address,
            options(nostack, preserves_flags)
        );

        // Read configuration data
        asm!(
            "in eax, dx",
            in("dx") 0xCFCu16,
            out("eax") value,
            options(nostack, preserves_flags)
        );
    }

    value
}

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
#[allow(dead_code)]
pub fn get_pci_ids(_bus: u8, device_index: u8, _func: u8) -> Option<(u16, u16)> {
    let bus = 0;
    let func = 0;

    // On ARM, we use the same function signature but the implementation
    // will use the ARM-specific pci_config_read
    let value = pci_config_read(bus, device_index, func, 0x00);
    let vendor_id = (value & 0xFFFF) as u16;
    let device_id = (value >> 16) as u16;

    // 0xFFFF is an invalid vendor ID, so we can use it to detect non-existent devices
    if vendor_id != 0xFFFF {
        Some((vendor_id, device_id))
    } else {
        None
    }
}
