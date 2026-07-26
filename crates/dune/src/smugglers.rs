//! The smuggler inventories — the seg001:10d8 table of six 0x11-byte records
//! (struct Smuggler): region, haggling attitude, two state bytes, five stock
//! counts and their prices. The new-day hook (events.rs) restocks empty
//! slots whose price byte has bit 7; the trade UI that spends them is not
//! yet ported.

/// = one 0x11-byte record of the smugglers table (seg001:10d8).
#[derive(Clone, Copy)]
pub(crate) struct Smuggler {
    pub(crate) region: u8,
    pub(crate) willingness_to_haggle: u8,
    /// +2 — state flags; bit 3 arms the daily restock (seg000:1cae).
    pub(crate) field_2: u8,
    pub(crate) field_3: u8,
    /// +4..+8 — the five stock counts: harvesters, ornithopters, krys
    /// knives, laser guns, weirding modules.
    pub(crate) stock: [u8; 5],
    /// +9..+0xd — the matching prices; bit 7 marks a slot the daily restock
    /// may refill.
    pub(crate) prices: [u8; 5],
    /// +0xe..+0x10 — carried verbatim (the DOS record tail).
    pub(crate) not_just_padding: [u8; 3],
}

const fn sm(region: u8, willingness_to_haggle: u8, stock: [u8; 5], prices: [u8; 5]) -> Smuggler {
    Smuggler {
        region,
        willingness_to_haggle,
        field_2: 0,
        field_3: 0,
        stock,
        prices,
        not_just_padding: [0; 3],
    }
}

/// = seg001:10d8 smugglers — the static initializer, extracted verbatim from
/// DNCDPRG.EXE (six records; the byte after the table is the 0xff region
/// terminator the walk at seg000:1cd4 stops on).
pub(crate) const SMUGGLERS: [Smuggler; 6] = [
    sm(0x01, 0x00, [1, 2, 0, 2, 2], [0x9e, 0xcb, 0x0a, 0xa8, 0xfd]),
    sm(0x03, 0x01, [1, 2, 0, 2, 1], [0x9e, 0xcb, 0x0a, 0xa8, 0xe4]),
    sm(0x05, 0x03, [1, 1, 0, 0, 1], [0xb2, 0xe3, 0x0a, 0x28, 0xe4]),
    sm(0x06, 0x02, [0, 2, 3, 2, 2], [0x28, 0xd0, 0x8f, 0xb2, 0xfd]),
    sm(0x09, 0x03, [2, 1, 0, 0, 1], [0xb2, 0xd0, 0x0a, 0x28, 0xee]),
    sm(0x0b, 0x06, [1, 1, 2, 1, 0], [0xbc, 0xda, 0x8a, 0xa8, 0x64]),
];
