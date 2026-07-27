// = seg001:08aa troops — one entry of the 68-troop table (27-byte stride).
// The troop list per location is a linked list chained via
// Location::troop_id (head) -> Troop::next_troop_id, both 1-based
// (0 = end of list), indexed as troops[id - 1].
#[derive(Clone, Copy)]
pub struct Troop {
    pub troop_id: u8,
    pub next_troop_id: u8,
    pub position: u8,
    pub occupation: u8,
    pub offset_of_location: u16,
    pub gps_coordinates_1: u16,
    pub gps_coordinates_2: u16,
    pub time_period_of_ralliement: u16,
    // = troop +0x0c/+0x0e troop_occupation_dependent_C/E — the two
    // accumulators the current occupation writes, cleared whenever it changes
    // (troop_set_occupation). For spice mining, the ported case, they are this
    // period's harvest rate and the running total behind the info panel's
    // "Current:" / "Average: N kgs/h". Other occupations reuse the same two
    // words for their own tallies — an attacking troop keeps its losses and
    // kills here (the "Fremen lost / Harkonnen killed" lines read ds:44/46),
    // and an ecology troop its covered area — so read the names as "what the
    // occupation is accumulating", not as spice specifically. A moving troop
    // (occupation bit 6) keeps its home location ptr in +0x0c and the
    // equipment-to-fetch word in +0x0e (slot index low, equipment bit high;
    // the turn-around arrival at seg000:841f consumes them).
    pub harvest_rate: u16,
    pub harvest_total: u16,
    pub bitfield_10: u16,
    pub dissatisfaction_and_speech: u16,
    pub game_day_of_ralliement: u8,
    pub motivation: u8,
    pub spice_skill: u8,
    pub army_skill: u8,
    pub ecology_skill: u8,
    // = the held-equipment bitmask, bit 7 = harvesters .. bit 1 = bulbs.
    pub equipment: u8,
    pub population: u8,
}

// = seg001:08aa troops — the 68-entry troop table's static initializer.
pub(crate) const TROOPS: [Troop; 68] = [
    // [0]
    Troop {
        troop_id: 1,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0x40,
        game_day_of_ralliement: 0,
        motivation: 28,
        spice_skill: 10,
        army_skill: 10,
        ecology_skill: 0,
        equipment: 0b00000000,
        population: 190,
    },
    // [1]
    Troop {
        troop_id: 2,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 26,
        spice_skill: 20,
        army_skill: 60,
        ecology_skill: 0,
        equipment: 0b00000000,
        population: 208,
    },
    // [2]
    Troop {
        troop_id: 3,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 16,
        spice_skill: 14,
        army_skill: 0,
        ecology_skill: 0,
        equipment: 0b00000000,
        population: 40,
    },
    // [3]
    Troop {
        troop_id: 4,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 40,
        spice_skill: 12,
        army_skill: 10,
        ecology_skill: 22,
        equipment: 0b00000000,
        population: 243,
    },
    // [4]
    Troop {
        troop_id: 5,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 30,
        spice_skill: 14,
        army_skill: 10,
        ecology_skill: 26,
        equipment: 0b00000000,
        population: 174,
    },
    // [5]
    Troop {
        troop_id: 6,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 26,
        spice_skill: 20,
        army_skill: 10,
        ecology_skill: 30,
        equipment: 0b00000000,
        population: 150,
    },
    // [6]
    Troop {
        troop_id: 7,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 22,
        spice_skill: 0,
        army_skill: 10,
        ecology_skill: 20,
        equipment: 0b00000000,
        population: 201,
    },
    // [7]
    Troop {
        troop_id: 8,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 31,
        spice_skill: 11,
        army_skill: 22,
        ecology_skill: 24,
        equipment: 0b00000000,
        population: 136,
    },
    // [8]
    Troop {
        troop_id: 9,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 25,
        spice_skill: 4,
        army_skill: 11,
        ecology_skill: 26,
        equipment: 0b00000000,
        population: 235,
    },
    // [9]
    Troop {
        troop_id: 10,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 23,
        spice_skill: 12,
        army_skill: 17,
        ecology_skill: 17,
        equipment: 0b00000000,
        population: 252,
    },
    // [10]
    Troop {
        troop_id: 11,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 19,
        spice_skill: 12,
        army_skill: 31,
        ecology_skill: 8,
        equipment: 0b00000000,
        population: 241,
    },
    // [11]
    Troop {
        troop_id: 12,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 12,
        spice_skill: 5,
        army_skill: 1,
        ecology_skill: 29,
        equipment: 0b00000000,
        population: 134,
    },
    // [12]
    Troop {
        troop_id: 13,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0x40,
        game_day_of_ralliement: 0,
        motivation: 23,
        spice_skill: 22,
        army_skill: 11,
        ecology_skill: 22,
        equipment: 0b00000000,
        population: 213,
    },
    // [13]
    Troop {
        troop_id: 14,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0x40,
        game_day_of_ralliement: 0,
        motivation: 21,
        spice_skill: 12,
        army_skill: 29,
        ecology_skill: 22,
        equipment: 0b00000000,
        population: 96,
    },
    // [14]
    Troop {
        troop_id: 15,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0x40,
        game_day_of_ralliement: 0,
        motivation: 20,
        spice_skill: 28,
        army_skill: 11,
        ecology_skill: 3,
        equipment: 0b00000000,
        population: 235,
    },
    // [15]
    Troop {
        troop_id: 16,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 31,
        spice_skill: 11,
        army_skill: 28,
        ecology_skill: 6,
        equipment: 0b00000000,
        population: 123,
    },
    // [16]
    Troop {
        troop_id: 17,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 39,
        spice_skill: 9,
        army_skill: 31,
        ecology_skill: 0,
        equipment: 0b00000000,
        population: 107,
    },
    // [17]
    Troop {
        troop_id: 18,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 17,
        spice_skill: 19,
        army_skill: 10,
        ecology_skill: 18,
        equipment: 0b00000000,
        population: 214,
    },
    // [18]
    Troop {
        troop_id: 19,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0x40,
        game_day_of_ralliement: 0,
        motivation: 23,
        spice_skill: 12,
        army_skill: 29,
        ecology_skill: 3,
        equipment: 0b00000000,
        population: 237,
    },
    // [19]
    Troop {
        troop_id: 20,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0x40,
        game_day_of_ralliement: 0,
        motivation: 22,
        spice_skill: 13,
        army_skill: 1,
        ecology_skill: 22,
        equipment: 0b00000000,
        population: 66,
    },
    // [20]
    Troop {
        troop_id: 21,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 6,
        spice_skill: 10,
        army_skill: 11,
        ecology_skill: 25,
        equipment: 0b00000000,
        population: 172,
    },
    // [21]
    Troop {
        troop_id: 22,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 15,
        spice_skill: 1,
        army_skill: 22,
        ecology_skill: 6,
        equipment: 0b00000000,
        population: 236,
    },
    // [22]
    Troop {
        troop_id: 23,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 21,
        spice_skill: 1,
        army_skill: 19,
        ecology_skill: 30,
        equipment: 0b00000000,
        population: 76,
    },
    // [23]
    Troop {
        troop_id: 24,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 8,
        spice_skill: 8,
        army_skill: 10,
        ecology_skill: 19,
        equipment: 0b00000000,
        population: 155,
    },
    // [24]
    Troop {
        troop_id: 25,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 16,
        spice_skill: 21,
        army_skill: 6,
        ecology_skill: 31,
        equipment: 0b00000000,
        population: 222,
    },
    // [25]
    Troop {
        troop_id: 26,
        next_troop_id: 0,
        position: 1,
        occupation: 0x80,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 31,
        spice_skill: 13,
        army_skill: 4,
        ecology_skill: 13,
        equipment: 0b00000000,
        population: 148,
    },
    // [26]
    Troop {
        troop_id: 27,
        next_troop_id: 28,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 6,
        equipment: 0b00111000,
        population: 180,
    },
    // [27]
    Troop {
        troop_id: 28,
        next_troop_id: 29,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 23,
        equipment: 0b00111000,
        population: 180,
    },
    // [28]
    Troop {
        troop_id: 29,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 11,
        equipment: 0b00111000,
        population: 180,
    },
    // [29]
    Troop {
        troop_id: 30,
        next_troop_id: 31,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 5,
        equipment: 0b00111100,
        population: 180,
    },
    // [30]
    Troop {
        troop_id: 31,
        next_troop_id: 32,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 2,
        equipment: 0b00111000,
        population: 180,
    },
    // [31]
    Troop {
        troop_id: 32,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 30,
        equipment: 0b00111100,
        population: 180,
    },
    // [32]
    Troop {
        troop_id: 33,
        next_troop_id: 34,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 24,
        equipment: 0b00111000,
        population: 180,
    },
    // [33]
    Troop {
        troop_id: 34,
        next_troop_id: 35,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 60,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 28,
        equipment: 0b00111000,
        population: 180,
    },
    // [34]
    Troop {
        troop_id: 35,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 15,
        equipment: 0b00110000,
        population: 180,
    },
    // [35]
    Troop {
        troop_id: 36,
        next_troop_id: 37,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 40,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 0,
        equipment: 0b00011000,
        population: 182,
    },
    // [36]
    Troop {
        troop_id: 37,
        next_troop_id: 38,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 40,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 2,
        equipment: 0b00010000,
        population: 182,
    },
    // [37]
    Troop {
        troop_id: 38,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 40,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 15,
        equipment: 0b00010000,
        population: 182,
    },
    // [38]
    Troop {
        troop_id: 39,
        next_troop_id: 40,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 60,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 30,
        equipment: 0b00011000,
        population: 185,
    },
    // [39]
    Troop {
        troop_id: 40,
        next_troop_id: 0,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 50,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 29,
        equipment: 0b00011000,
        population: 190,
    },
    // [40]
    Troop {
        troop_id: 41,
        next_troop_id: 42,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 10,
        equipment: 0b00111100,
        population: 190,
    },
    // [41]
    Troop {
        troop_id: 42,
        next_troop_id: 43,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 3,
        equipment: 0b00111000,
        population: 190,
    },
    // [42]
    Troop {
        troop_id: 43,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 27,
        equipment: 0b00111100,
        population: 190,
    },
    // [43]
    Troop {
        troop_id: 44,
        next_troop_id: 45,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 27,
        equipment: 0b00010000,
        population: 185,
    },
    // [44]
    Troop {
        troop_id: 45,
        next_troop_id: 46,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 19,
        equipment: 0b00010000,
        population: 185,
    },
    // [45]
    Troop {
        troop_id: 46,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 24,
        equipment: 0b00010000,
        population: 185,
    },
    // [46]
    Troop {
        troop_id: 47,
        next_troop_id: 48,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 10,
        spice_skill: 30,
        army_skill: 70,
        ecology_skill: 18,
        equipment: 0b00110000,
        population: 188,
    },
    // [47]
    Troop {
        troop_id: 48,
        next_troop_id: 49,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 17,
        spice_skill: 30,
        army_skill: 69,
        ecology_skill: 15,
        equipment: 0b00010000,
        population: 188,
    },
    // [48]
    Troop {
        troop_id: 49,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 20,
        spice_skill: 30,
        army_skill: 65,
        ecology_skill: 31,
        equipment: 0b00010000,
        population: 188,
    },
    // [49]
    Troop {
        troop_id: 50,
        next_troop_id: 51,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 25,
        spice_skill: 30,
        army_skill: 48,
        ecology_skill: 7,
        equipment: 0b00011000,
        population: 188,
    },
    // [50]
    Troop {
        troop_id: 51,
        next_troop_id: 0,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 25,
        spice_skill: 30,
        army_skill: 58,
        ecology_skill: 22,
        equipment: 0b00011000,
        population: 188,
    },
    // [51]
    Troop {
        troop_id: 52,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 4,
        equipment: 0b00111000,
        population: 185,
    },
    // [52]
    Troop {
        troop_id: 53,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 90,
        spice_skill: 30,
        army_skill: 90,
        ecology_skill: 9,
        equipment: 0b00011100,
        population: 180,
    },
    // [53]
    Troop {
        troop_id: 54,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 40,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 13,
        equipment: 0b00011000,
        population: 180,
    },
    // [54]
    Troop {
        troop_id: 55,
        next_troop_id: 56,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 12,
        spice_skill: 30,
        army_skill: 34,
        ecology_skill: 2,
        equipment: 0b00110000,
        population: 180,
    },
    // [55]
    Troop {
        troop_id: 56,
        next_troop_id: 0,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 12,
        spice_skill: 30,
        army_skill: 34,
        ecology_skill: 23,
        equipment: 0b00110000,
        population: 180,
    },
    // [56]
    Troop {
        troop_id: 57,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 10,
        spice_skill: 30,
        army_skill: 34,
        ecology_skill: 0,
        equipment: 0b00110000,
        population: 180,
    },
    // [57]
    Troop {
        troop_id: 58,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 10,
        spice_skill: 30,
        army_skill: 16,
        ecology_skill: 31,
        equipment: 0b00110000,
        population: 180,
    },
    // [58]
    Troop {
        troop_id: 59,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 10,
        spice_skill: 30,
        army_skill: 10,
        ecology_skill: 16,
        equipment: 0b00110000,
        population: 180,
    },
    // [59]
    Troop {
        troop_id: 60,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 80,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 15,
        equipment: 0b00010000,
        population: 180,
    },
    // [60]
    Troop {
        troop_id: 61,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 99,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 21,
        equipment: 0b00111100,
        population: 180,
    },
    // [61]
    Troop {
        troop_id: 62,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 60,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 22,
        equipment: 0b00011000,
        population: 180,
    },
    // [62]
    Troop {
        troop_id: 63,
        next_troop_id: 64,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 16,
        equipment: 0b00111000,
        population: 180,
    },
    // [63]
    Troop {
        troop_id: 64,
        next_troop_id: 65,
        position: 10,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 30,
        equipment: 0b00001000,
        population: 180,
    },
    // [64]
    Troop {
        troop_id: 65,
        next_troop_id: 0,
        position: 11,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 70,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 22,
        equipment: 0b00110000,
        population: 180,
    },
    // [65]
    Troop {
        troop_id: 66,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 50,
        spice_skill: 30,
        army_skill: 80,
        ecology_skill: 20,
        equipment: 0b00011000,
        population: 180,
    },
    // [66]
    Troop {
        troop_id: 67,
        next_troop_id: 0,
        position: 9,
        occupation: 0x8c,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000010010000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 8,
        spice_skill: 30,
        army_skill: 95,
        ecology_skill: 20,
        equipment: 0b00111000,
        population: 10,
    },
    // [67]
    Troop {
        troop_id: 0,
        next_troop_id: 0,
        position: 0,
        occupation: 0,
        offset_of_location: 0,
        gps_coordinates_1: 0,
        gps_coordinates_2: 0,
        time_period_of_ralliement: 0,
        harvest_rate: 0,
        harvest_total: 0,
        bitfield_10: 0b0000000000000000,
        dissatisfaction_and_speech: 0,
        game_day_of_ralliement: 0,
        motivation: 0,
        spice_skill: 0,
        army_skill: 0,
        ecology_skill: 0,
        equipment: 0b00000000,
        population: 0,
    },
];

use crate::{GameState, locations};

/// = the for_condit_troop_* staging block at seg001:002c..004b, filled by
/// troop_prepare_troop_data_for_condit for the troop behind the active
/// conversation; CONDIT operands read it through condit_ds_byte/word.
#[derive(Default, Clone, Copy)]
pub(crate) struct TroopCondit {
    /// = seg001:002c for_condit_troop_offset_of_location_ds_2c (the DOS
    /// seg001 pointer, so conditions comparing against location constants
    /// keep working).
    pub(crate) offset_of_location: u16,
    /// = seg001:002e for_condit_troop_troop_id_ds_2e.
    pub(crate) troop_id: u8,
    /// = seg001:002f for_condit_troop_occupation_and_0xf_ds_2f.
    pub(crate) occupation_low: u8,
    /// = seg001:0030 for_condit_troop_occupation_ds_30.
    pub(crate) occupation: u8,
    /// = seg001:0031 for_condit_troop_dissatisfactionAndSpeech_and_0xf_ds_31.
    pub(crate) dissatisfaction_low: u8,
    /// = seg001:0032 for_condit_troop_bitfield_10_ds_32.
    pub(crate) bitfield_10: u16,
    /// = seg001:0034 for_condit_troop_dissatisfactionAndSpeech_ds_34.
    pub(crate) dissatisfaction_and_speech: u16,
    /// = seg001:0036 for_condit_troop_ds_36 (the motivation modifier).
    pub(crate) motivation_modifier: u8,
    /// = seg001:0037 for_condit_troop_skill_in_current_occupation_ds_37.
    pub(crate) skill_in_occupation: u8,
    /// = seg001:0038..003a the three skills, 003b equipment, 003c population.
    pub(crate) spice_skill: u8,
    pub(crate) army_skill: u8,
    pub(crate) ecology_skill: u8,
    pub(crate) equipment: u8,
    pub(crate) population: u8,
    /// = seg001:0040 for_condit_troop_number_of_days_since_ralliement_ds_40.
    pub(crate) days_since_ralliement: u8,
    /// = seg001:0041 for_condit_game_days_since_ralliement_ds_41.
    pub(crate) game_days_since_ralliement: u8,
    /// = seg001:0042 for_condit_time_periods_since_ralliement_ds_42.
    pub(crate) time_periods_since_ralliement: u16,
    /// = seg001:0044/0046 for_condit_troop_field_C/field_E — the staged copy
    /// of the troop's two occupation accumulators (see Troop::harvest_rate).
    pub(crate) harvest_rate: u16,
    pub(crate) harvest_total: u16,
    /// = seg001:0048 for_condit_ds_48 — the troop's current spice production
    /// rate (troop_update_harvest_rate, the same figure the mining callback
    /// harvests that period), shown as "Current: N kgs/h"; and
    /// = seg001:004a for_condit_ds_4a — the running average beside it,
    /// harvest_total over the elapsed periods, shown as "Average: N kgs/h".
    pub(crate) ds_48: u16,
    pub(crate) ds_4a: u16,
}

/// = the for_condit location staging block at seg001:004d..005b, filled by
/// prepare_location_data_for_condit.
#[derive(Default, Clone, Copy)]
pub(crate) struct LocationCondit {
    /// = seg001:004d for_condit_location_appearance_ds_4d.
    pub(crate) appearance: u8,
    /// = seg001:004e for_condit_location_location_area_and_name_ds_4e
    /// ((first_name << 8) | last_name).
    pub(crate) area_and_name: u16,
    /// = seg001:0050 for_condit_likelihood_of_worm_related_spice_mining_
    /// troop_events_ds_50.
    pub(crate) worm_event_likelihood: u8,
    /// = seg001:0051 for_condit_location_status_ds_51.
    pub(crate) status: u8,
    /// = seg001:0052 for_condit_location_spice_density_ds_52.
    pub(crate) spice_density: u8,
    /// = seg001:0053 for_condit_related_to_unused_location_equipment_ds_53.
    pub(crate) unused_equipment: u8,
    /// = seg001:0054 for_condit_location_water_amount_or_wind_trap_assembly_
    /// progress_ds_54.
    pub(crate) water: u8,
    /// = seg001:0055 array_for_condit_location_equipment_ds_55 ([di+0x14..0x1b]).
    pub(crate) equipment: [u8; 7],
}

impl GameState {
    // = seg000:01c8 the startup loop over locations + callback_troop_location_
    // 001e0 — link every troop to its location: offset_of_location = the
    // location's seg001 pointer, the gps coordinates = the location's map
    // cell, and the dissatisfaction_and_speech low byte = the location's
    // voice-bank id ((first_name & 0xf) | (dissat & 0x70) | the alternate-bank
    // bit 0x80 for first_name 4..5 and >= 10).
    pub(crate) fn init_troop_locations(&mut self) {
        for loc_index in 0..self.locations.len() {
            let loc = self.locations[loc_index];
            // = seg000:01da the 0xff terminator entry ends the walk.
            if loc.first_name == 0xff {
                break;
            }
            self.for_each_troop_in_location(loc_index, |game, ti| {
                let t = &mut game.troops[ti];
                // = seg000:01e0..01e6.
                t.offset_of_location = locations::location_ptr(loc_index as u16);
                t.gps_coordinates_1 = loc.map_x as u16;
                t.gps_coordinates_2 = loc.map_y as u16;
                // = seg000:01e9..0208 the voice-bank low byte.
                let mut bank = loc.first_name & 0x0f;
                let mut high = (t.dissatisfaction_and_speech as u8) & 0x70;
                if loc.first_name > 3 {
                    high ^= 0x80;
                }
                if loc.first_name > 5 {
                    high ^= 0x80;
                }
                if loc.first_name > 9 {
                    high ^= 0x80;
                }
                bank |= high;
                t.dissatisfaction_and_speech =
                    (t.dissatisfaction_and_speech & 0xff00) | bank as u16;
            });
        }
    }

    // = seg000:6603 call_callback_on_all_troops_in_location — walk the
    // location's troop chain (Location.troop_id -> Troop.next_troop_id,
    // 1-based ids, 0 ends the list) calling `callback` with each troop's
    // table index. The next id is re-read after the callback (DOS reloads
    // si->next_troop_id at seg000:6616), so a callback may relink the chain.
    pub(crate) fn for_each_troop_in_location(
        &mut self,
        loc_index: usize,
        mut callback: impl FnMut(&mut Self, usize),
    ) {
        let mut id = self.locations[loc_index].troop_id;
        while id != 0 {
            let ti = (id - 1) as usize;
            callback(self, ti);
            id = self.troops[ti].next_troop_id;
        }
    }

    // = seg000:316e callback_troop_location_0316e — classify one troop of the
    // current location into a dynamic room-person slot for the room the
    // player just entered. The troop appears in room 2 (the audience room),
    // or room 3 when it has the no-more-orders bit in a Harkonnen fortress
    // (appearance >= 0x28), or room 1 during the night attack; a troop bound
    // for another room is skipped. A Harkonnen troop (bitfield_10 bit 7)
    // becomes the captain (room_persons[12]); a rallied troop (occupation
    // bit 7) the Fremen chief (room_persons[14]); anyone else joins the
    // Fremen-2 round-robin (room_persons[15], fremen2_troop_ptrs).
    fn classify_troop_for_room(&mut self, ti: usize) {
        let t = self.troops[ti];
        let loc_index = ((self.location_appearance >> 8) as usize).wrapping_sub(1);
        // = seg000:316e al = occupation; ah = 2.
        let mut room = 2u8;
        if t.occupation & 0x20 != 0 {
            // = seg000:3177 the no-more-orders troop sits in room 3 of a
            //   Harkonnen fortress/palace (appearance >= 0x28).
            if self.locations[loc_index].appearance >= 0x28 {
                room = 3;
            }
        } else {
            // = seg000:3181 a Harkonnen troop that still has orders stays
            //   out of sight.
            if t.bitfield_10 & 0x80 != 0 {
                return;
            }
            // = seg000:3187 during the night attack the troop mans room 1.
            if self.night_attack_stage != 0 {
                room = 1;
            }
        }
        // = seg000:3190 cmp ah, dl — only the troop bound for this room.
        if room as u16 != self.location_and_room & 0xff {
            return;
        }
        // = seg000:3194 the slot triage.
        let slot = if t.bitfield_10 & 0x80 != 0 {
            // = seg000:31c9 the Harkonnen captain: room_persons[12],
            //   harkonnen_captain_troop_ptr = the troop; flags bit 4 mirrors
            //   occupation bit 4 (the surrendered state), and the
            //   overpower-condit pair ds:ee/ed is seeded (0xff when
            //   surrendered, else the troop's motivation).
            self.harkonnen_captain_troop = Some(ti);
            let occ_bit4 = t.occupation & 0x10;
            let rp = &mut self.room_persons[12];
            rp.flags = (rp.flags & !0x10) | occ_bit4;
            self.data_000ee = 0;
            self.data_000ed = if occ_bit4 != 0 { 0xff } else { t.motivation };
            12
        } else if t.occupation >= 0x80 {
            // = seg000:31a0 jnb — the rallied troop's chief: Fremen 1.
            self.fremen1_troop = Some(ti);
            14
        } else {
            // = seg000:31a4 Fremen 2: the round-robin fremen2_troop_ptrs slot
            //   (data_0476a & 7, incremented per troop); the prospector troop
            //   (troops[2]) records its 1-based slot in data_0476b.
            self.data_0476a &= 7;
            let idx = self.data_0476a as usize;
            self.data_0476a += 1;
            if ti == 2 {
                self.data_0476b = idx as u8 + 1;
            }
            self.fremen2_troops[idx] = Some(ti);
            15
        };
        // = seg000:31ed the matched slot now matches the current room.
        self.room_persons[slot].location_and_room = self.location_and_room;
        self.room_persons[slot].location_appearance = self.location_appearance;
    }

    // = seg000:3140..316a the special-room half of init_room_persons: for a
    // room whose location_appearance low byte is 0x80, classify the
    // location's troops into the dynamic room-person slots, reveal the
    // smuggler in a smuggler den (appearance 0x21), and stage the location's
    // CONDIT block.
    pub(crate) fn init_room_persons_special(&mut self) {
        // = seg000:3140/3144 only rooms with the 0x80 appearance low byte.
        if self.location_appearance & 0xff != 0x80 {
            return;
        }
        // = seg000:3149 di = [current_location_ptr] — the port derives the
        //   location from the appearance high byte (locations[bh - 1]).
        let Some(loc_index) = ((self.location_appearance >> 8) as usize).checked_sub(1) else {
            return;
        };
        if loc_index >= self.locations.len() {
            return;
        }
        // = seg000:3151/3154 run the classification over the troop chain.
        self.for_each_troop_in_location(loc_index, Self::classify_troop_for_room);
        // = seg000:3157 a smuggler den (appearance 0x21) shows the SMUG
        //   person (room_persons[13]).
        if self.locations[loc_index].appearance == 0x21 {
            self.room_persons[13].location_and_room = self.location_and_room;
            self.room_persons[13].location_appearance = self.location_appearance;
            // = seg000:3166 call loc_02318 — the smuggler-encounter staging
            //   (the Smuggler table walk, days-since-encounter, the condit
            //   smuggler fields). TODO: the smuggler data model is not
            //   ported.
        }
        // = seg000:316a call prepare_location_data_for_condit.
        self.prepare_location_data_for_condit(loc_index);
    }

    // = seg000:331e prepare_location_data_for_condit — stage the location's
    // CONDIT block (ds:4d..5b) from its record. The derived pieces
    // (location_033be, sub_034a5, sub_03385, sub_05274 and the
    // compute_location_available_equipment mask at ds:53) are not yet ported.
    pub(crate) fn prepare_location_data_for_condit(&mut self, loc_index: usize) {
        // = seg000:331e mov [data_011ce], di — the staged location.
        self.condit_staged_location = loc_index;
        let loc = self.locations[loc_index];
        // = seg000:3324..3329 ds:4e = (first_name << 8) | last_name.
        self.location_condit.area_and_name = ((loc.first_name as u16) << 8) | loc.last_name as u16;
        // = seg000:332c xlat — ds:50 = the per-region worm-event likelihood
        //   (array indexed by last_name). TODO: the array is not modelled.
        self.location_condit.worm_event_likelihood = 0;
        // = seg000:3333..3348 the direct field copies.
        self.location_condit.status = loc.status;
        self.location_condit.spice_density = loc.spice_density;
        self.location_condit.water = loc.water;
        self.location_condit.appearance = loc.appearance;
        // = seg000:334d..3357 the 7 equipment bytes at [di+0x14].
        self.location_condit.equipment = [
            loc.equipment.harvesters,
            loc.equipment.ornithopters,
            loc.equipment.krys_knives,
            loc.equipment.laser_guns,
            loc.equipment.weirding_modules,
            loc.equipment.atomics,
            loc.equipment.bulbs,
        ];
        // = seg000:335a..3379 location_033be, sub_034a5, compute_location_
        //   available_equipment -> the ds:53 unused-equipment mask; 337d/3380
        //   sub_03385 / sub_05274. TODO: not yet ported.
        self.location_condit.unused_equipment = 0;
    }

    // = seg000:3310 get_command_string_index_from_troop_skill — the skill
    // caption's COMMAND id: the skill's high nibble picks one of "On trial",
    // "Novice", "Average", "Efficient", "Skilled", "Expert" (0xd1..0xd6).
    pub(crate) fn get_command_string_index_from_troop_skill(skill: u8) -> u16 {
        // = seg000:3310..331a xor ah,ah; shr ax,1 x4; add ax,0d1h.
        (skill >> 4) as u16 + 0xd1
    }

    // = seg000:31f6 troop_prepare_troop_data_for_condit — stage the troop's
    // CONDIT block (ds:2c..4b) before a troop-person conversation, then the
    // troop's location block. It also stages the string_subst_id_table entries
    // 3..8 and 0xa — the ids the 0x83..0x8a placeholders in the troop's
    // dialogue and info-panel strings expand to.
    pub(crate) fn troop_prepare_troop_data_for_condit(&mut self, ti: usize) {
        let t = self.troops[ti];
        // = seg000:31f9..320e the direct copies.
        self.troop_condit.offset_of_location = t.offset_of_location;
        self.troop_condit.troop_id = t.troop_id;
        self.troop_condit.occupation = t.occupation;
        self.troop_condit.occupation_low = t.occupation & 0x0f;
        // = seg000:3211/3214 subst_id_04 = (occupation & 0xf) + 0x18 — the
        //   placeholder 0x84 occupation caption (COMMAND 0x18.. "Spice
        //   Mining", ".."; map_draw_troop_info_panel_content bumps it by 0xc
        //   to the 0x24.. panel wording at seg000:78f7).
        self.string_subst_id_table[4] = (t.occupation & 0x0f) as u16 + 0x18;
        // = seg000:3217 call sub_032c7 — the ralliement clocks.
        // = seg000:32c7..32d6 ds:42 = game_time - time_period_of_ralliement;
        //   ds:41 = ds:42 >> 4.
        let periods = self.game_time.wrapping_sub(t.time_period_of_ralliement);
        self.troop_condit.time_periods_since_ralliement = periods;
        self.troop_condit.game_days_since_ralliement = (periods >> 4) as u8;
        // = seg000:32d9..330c subst_id_05 — the placeholder 0x85 duration
        //   phrase: "but our job is finished" (0x74) for occupation bit 4,
        //   else keyed on the periods since ralliement — "for a very short
        //   time" (< 3), "for a few hours" (< 0x10), "for 1 day" (< 0x20),
        //   else "for 12 days" (0x73) with the day count patched over its
        //   digits in the resource.
        self.string_subst_id_table[5] = if t.occupation & 0x10 != 0 {
            0x74
        } else if periods < 3 {
            0x70
        } else if periods < 0x10 {
            0x71
        } else if periods < 0x20 {
            0x72
        } else {
            // = seg000:32f7..330a get_phrase_or_command_string_si + the
            //   number replacement on the string itself.
            self.command_string_replace_number(0x73, periods >> 4);
            0x73
        };
        // = seg000:321a/321d ds:48 = sub_0329d (a mining troop's current
        //   rate; it may clear bitfield_10 bits 2..3 and update harvest_rate).
        self.troop_condit.ds_48 = self.troop_condit_stage_harvest_rates(ti);
        // = seg000:3220..322f — bitfield_10 is re-read AFTER sub_0329d.
        let t = self.troops[ti];
        self.troop_condit.bitfield_10 = t.bitfield_10;
        self.troop_condit.dissatisfaction_and_speech = t.dissatisfaction_and_speech;
        self.troop_condit.dissatisfaction_low = t.dissatisfaction_and_speech as u8 & 0x0f;
        // = seg000:3232/3235 subst_id_0a = the dissatisfaction low nibble —
        //   the placeholder 0x8a phrase.
        self.string_subst_id_table[0xa] = t.dissatisfaction_and_speech & 0x0f;
        // = seg000:3238/323b ds:36 = troop_compute_motivation_modifier.
        self.troop_condit.motivation_modifier = self.troop_compute_motivation_modifier(ti);
        // = seg000:323e..325f the three skills; each also feeds its skill-
        //   caption placeholder through get_command_string_index_from_troop_
        //   skill — 0x86 spice, 0x87 army, 0x88 ecology.
        self.troop_condit.spice_skill = t.spice_skill;
        self.string_subst_id_table[6] =
            Self::get_command_string_index_from_troop_skill(t.spice_skill);
        self.troop_condit.army_skill = t.army_skill;
        self.string_subst_id_table[7] =
            Self::get_command_string_index_from_troop_skill(t.army_skill);
        self.troop_condit.ecology_skill = t.ecology_skill;
        self.string_subst_id_table[8] =
            Self::get_command_string_index_from_troop_skill(t.ecology_skill);
        // = seg000:3262..3273 field_C / field_E (field_C after sub_0329d's
        //   update); subst_id_03 = field_E's low byte + 0xe8 — the placeholder
        //   0x83 equipment name (COMMAND 0xe8.. "a spice-harvester"..).
        self.troop_condit.harvest_rate = t.harvest_rate;
        self.troop_condit.harvest_total = t.harvest_total;
        self.string_subst_id_table[3] = (t.harvest_total & 0xff) + 0xe8;
        // = seg000:3276..327e ds:37 = the skill for the current occupation:
        //   [si + ((occupation & 0xf) >> 2) + 0x16] — spice/army/ecology (an
        //   occupation-bits value of 3 reads the equipment byte, as DOS does).
        let skill_index = ((t.occupation & 0x0f) >> 2) as usize;
        self.troop_condit.skill_in_occupation =
            [t.spice_skill, t.army_skill, t.ecology_skill, t.equipment][skill_index];
        // = seg000:3281..328a equipment + population.
        self.troop_condit.equipment = t.equipment;
        self.troop_condit.population = t.population;
        // = seg000:328d..3293 ds:40 = get_ingame_day - game_day_of_ralliement.
        self.troop_condit.days_since_ralliement =
            (self.get_ingame_day_in_ax() as u8).wrapping_sub(t.game_day_of_ralliement);
        // = seg000:3296 call prepare_location_data_for_condit on the troop's
        //   location (di still holds troop->offset_of_location).
        let loc_index = locations::location_index_from_ptr(t.offset_of_location);
        if loc_index < self.locations.len() {
            self.prepare_location_data_for_condit(loc_index);
        }
    }

    // = seg000:329d troop_prepare_troop_data_for_condit_sub_0329d — stage the
    // two rates the troop info panel shows. Only a spice-mining troop
    // (occupation 0) has them: ds:4a = harvest_total averaged over the time
    // periods since ralliement ("Average: N kgs/h"), then ds:48 = the current
    // rate from troop_update_harvest_rate ("Current:"). Any other occupation
    // clears the trend bits (bitfield_10 &= ~0x0c) and reads 0.
    fn troop_condit_stage_harvest_rates(&mut self, ti: usize) -> u16 {
        let t = self.troops[ti];
        if t.occupation != 0 {
            // = seg000:32a3/32a5.
            self.troops[ti].bitfield_10 &= 0xfff3;
            return 0;
        }
        // = seg000:32aa..32c1 ds:4a = round(field_E / ds:42).
        let periods = self.troop_condit.time_periods_since_ralliement;
        // = seg000:32aa..32be — field_E averaged over the periods, rounded up
        //   when the remainder is at least half the divisor (the shl dx,1;
        //   cmp cx,dx; adc ax,0 sequence); a zero period count reads 0.
        let avg = match t.harvest_total.checked_div(periods) {
            Some(q) => {
                q + if periods <= (t.harvest_total % periods) * 2 {
                    1
                } else {
                    0
                }
            }
            None => 0,
        };
        self.troop_condit.ds_4a = avg;
        // = seg000:32c4 jmp troop_update_harvest_rate.
        self.troop_update_harvest_rate(ti)
    }

    // = seg000:708a troop_update_harvest_rate — this time period's spice
    // harvest for the troop. The mining callback adds the returned value
    // straight into harvest_total, pays a tenth of it into spice_in_stock and
    // takes the matching bite out of the field (seg000:6fff). The condit
    // staging above tail-jumps here for the same figure, which is what makes it
    // the panel's "Current: N kgs/h".
    //
    // Folds the trend into bitfield_10: bits 2..3 become 8 when the rate ROSE
    // (DOS `sub dx,ax` borrows, so old < new) and 4 when it fell. The new value
    // replaces harvest_rate (troop_occupation_dependent_C) and is returned.
    fn troop_update_harvest_rate(&mut self, ti: usize) -> u16 {
        let t = self.troops[ti];
        // = seg000:708a..7093 al = motivation modifier + (spice_skill & 0xf0).
        let al = self
            .troop_compute_motivation_modifier(ti)
            .wrapping_add(t.spice_skill & 0xf0);
        // = seg000:7095 mul population.
        let mut ax = al as u16 * t.population as u16;
        // = seg000:7098..70a0 without a harvester (equipment bit 7) only a
        //   quarter of it.
        if t.equipment & 0x80 == 0 {
            ax >>= 2;
        }
        // = seg000:70a2..70a8 al = location spice_density & 0xf0; inc ax;
        //   mul ah — the byte ops leave ah = the previous product's high
        //   byte through the al reload.
        let loc_index = locations::location_index_from_ptr(t.offset_of_location);
        let density = if loc_index < self.locations.len() {
            self.locations[loc_index].spice_density & 0xf0
        } else {
            0
        };
        ax = (ax & 0xff00) | density as u16;
        ax = ax.wrapping_add(1);
        let product = (ax as u8 as u16) * (ax >> 8);
        // = seg000:70aa..70ae xchg al,ah; rol ax,1; and ah,1.
        let swapped = product.rotate_left(8);
        let rolled = swapped.rotate_left(1);
        let value = rolled & 0x01ff;
        // = seg000:70b1..70c8 the trend bits vs the previous harvest_rate.
        let old = self.troops[ti].harvest_rate;
        self.troops[ti].harvest_rate = value;
        if old != value {
            let trend = if old < value { 8 } else { 4 };
            let t = &mut self.troops[ti];
            t.bitfield_10 = (t.bitfield_10 & !0x0c) | trend;
        }
        value
    }

    // = seg000:6efd troop_compute_motivation_modifier — the troop's effective
    // motivation: +0x14 once vegetation has started on Dune; +0x1e for an
    // ecology troop (occupation 6) working the current location (skipping the
    // 0x64 cap); occupation 8..9 reads a flat 0x64; otherwise capped at 0x64.
    // In the endgame window (game_phase 0x64..0x67) everything drops by 0x28
    // with a floor of 10.
    pub(crate) fn troop_compute_motivation_modifier(&self, ti: usize) -> u8 {
        let t = &self.troops[ti];
        let occ = t.occupation & 0x0f;
        let mut al = t.motivation;
        // = seg000:6f06 the vegetation bonus.
        if self.vegetation_started_on_dune != 0 {
            al = al.wrapping_add(0x14);
        }
        let mut capped = true;
        if occ == 6 {
            // = seg000:6f14..6f21 the ecology troop at the current location.
            let here = ((self.location_appearance >> 8) as usize)
                .checked_sub(1)
                .map(|i| locations::location_ptr(i as u16))
                == Some(t.offset_of_location);
            if here {
                al = al.wrapping_add(0x1e);
            } else {
                // = seg000:6f1d jnz loc_06f31 — skips the cap.
                capped = false;
            }
        } else if occ & 0x0e == 8 {
            // = seg000:6f23..6f2f occupation 8..9 reads a flat 0x64.
            al = 0x64;
        }
        // = seg000:6f2b the 0x64 cap.
        if capped && al > 0x64 {
            al = 0x64;
        }
        // = seg000:6f31..6f45 the endgame malus.
        if (0x64..0x68).contains(&self.game_phase) {
            al = al.wrapping_sub(0x28);
            if (al as i8) < 10 {
                al = 10;
            }
        }
        al
    }

    // = seg000:1ebe game_phase_set_to_64_if_conditions_met — a Fremen-2 troop
    // whose dissatisfaction_and_speech carries bit 0x800, spoken to during
    // phases 0x60..0x63, advances the story to phase 0x64.
    pub(crate) fn game_phase_set_to_64_if_conditions_met(&mut self, ti: usize) {
        if self.troops[ti].dissatisfaction_and_speech & 0x800 == 0 {
            return;
        }
        if !(0x60..0x64).contains(&self.game_phase) {
            return;
        }
        self.set_game_phase_and_trigger_callbacks(0x64);
    }

    // = seg000:6c6f run_troop_occupation_events — the per-time-period walk
    // over every troop: run the callback for its occupation (the seg001 table
    // at 6c26), then age its skills. Called from
    // run_events_for_current_time_period (seg000:1b70).
    pub(crate) fn run_troop_occupation_events(&mut self) {
        // = seg000:6c6f..6c87 data_0473c = the location room_persons[4] stands
        //   in, then loc_06c46 — the per-period location/NPC bookkeeping. Not
        //   ported.
        // = seg000:6c8a data_04737 = 0 — the vegetation accumulator the walk
        //   rebuilds (copied into vegetation_started_on_Dune at 6ccc).
        // = seg000:6cc3..6cca add si,1bh; cmp si,troops[67]; jb — the walk
        //   stops at the table's last entry, which is the terminator (troop id
        //   0, no location), not a troop.
        for ti in 0..self.troops.len() - 1 {
            let t = self.troops[ti];
            // = seg000:6c92 test dissatisfaction_and_speech,430h; jnz
            //   loc_06cd3 — a troop with the ill bit or the just-reassigned
            //   speech bits is out of the normal walk: a moving one still
            //   takes its travel step, and once vegetation has started on
            //   Dune the speech bits 4-5 clear so a healthy troop rejoins
            //   the walk (the re-test at 6ceb).
            if t.dissatisfaction_and_speech & 0x430 != 0 {
                // = seg000:6cd3 test occupation,40h; jnz loc_06ced.
                if t.occupation & 0x40 != 0 {
                    self.troop_travel_step_unless_selected(ti);
                    continue;
                }
                // = seg000:6cd9..6ce9 the vegetation-started release.
                if self.vegetation_started_on_dune == 0 {
                    continue;
                }
                self.troops[ti].dissatisfaction_and_speech &= 0xffcf;
                if self.troops[ti].dissatisfaction_and_speech & 0x400 != 0 {
                    continue;
                }
            }
            // = seg000:6c99..6ca2 a troop under 20 strong may be folded into
            //   another at its location (troop_06d19). Not ported.
            // = seg000:6ca4..6ca9 test occupation,0a0h — captured (0x20) or
            //   unrallied (0x80) troops do nothing.
            let t = self.troops[ti];
            if t.occupation & 0xa0 != 0 {
                continue;
            }
            // = seg000:6cab test occupation,40h; jnz loc_06ced — a moving
            //   troop takes a travel step (troop_travel_step) instead.
            if t.occupation & 0x40 != 0 {
                self.troop_travel_step_unless_selected(ti);
                continue;
            }
            // = seg000:6caf..6cba call the occupation's callback with si = the
            //   troop and di = its location.
            self.run_troop_occupation_callback(ti);
            // = seg000:6cc0 call troop_usually_decrease_skills_every_4_days —
            //   the every-64-period skill decay. Not ported.
        }
    }

    // = seg000:6ced loc_06ced — the walk's moving-troop branch: the troop
    // whose contact UI is open (map_selected_troop_id) pauses in place, every
    // other moving troop takes its travel step.
    fn troop_travel_step_unless_selected(&mut self, ti: usize) {
        if self.troops[ti].troop_id == self.map_selected_troop_id {
            return;
        }
        self.troop_travel_step(ti);
    }

    // = seg000:6c26 array_callbacks_for_troop_occupation_06c26 — the per-period
    // callback for each occupation nibble. Only spice mining is ported; the
    // others (prospecting, military training, espionage, attacking, irrigation,
    // wind-trap assembly, bulb growing) are their own subsystems, and slots
    // 2/3/7/11..15 are nullsub_00f66.
    fn run_troop_occupation_callback(&mut self, ti: usize) {
        if self.troops[ti].occupation & 0x0f == 0 {
            self.troop_occupation_event_spice_mining(ti);
        }
    }

    // = seg000:6fe5 callback_troop_location_for_troop_occupation_spice_mining —
    // one time period of spice mining: work out what the troop pulled out of
    // the ground (troop_update_harvest_rate), add it to the troop's running
    // total, pay a tenth of it into the player's stock and take the matching
    // bite out of the location's spice density.
    fn troop_occupation_event_spice_mining(&mut self, ti: usize) {
        // = seg000:6fe5 call troop_location_do_stuff_upon_new_day_06e20 — the
        //   per-day location discovery countdown and motivation decay. Not
        //   ported.
        // = seg000:6fe8 test bitfield_10,200h; jnz — a damaged harvester mines
        //   nothing until it is repaired.
        if self.troops[ti].bitfield_10 & 0x200 != 0 {
            // = seg000:705c..7066 the repair comes up once a day, on the time
            //   period whose low nibble matches the troop's id.
            let due = (self.game_time & 0x0f) as u8 == (self.troops[ti].troop_id & 0x0f);
            if !due {
                // = seg000:7074/7079 harvest_rate = 0 (no rate to show) and the
                //   troop stops working.
                self.troops[ti].harvest_rate = 0;
                self.troop_make_stop_working(ti);
                return;
            }
            // = seg000:7068 the harvester is fixed.
            self.troops[ti].bitfield_10 &= 0xfdff;
            // = seg000:706d/7070 it still has to be worth mining here.
            if self.troop_spice_mining_not_viable(ti) {
                return;
            }
        } else {
            // = seg000:6fef/6ff2 the same viability test on the normal path;
            //   not viable clears BOTH accumulators (loc_0707b: no rate and no
            //   running total to average) and stops the troop.
            if self.troop_spice_mining_not_viable(ti) {
                self.troops[ti].harvest_rate = 0;
                self.troops[ti].harvest_total = 0;
                self.troop_make_stop_working(ti);
                return;
            }
            // = seg000:6ff7 call troop_location_events_for_spice_mining_troops_
            //   with_harvesters — the worm / saboteur events that damage or eat
            //   the harvester. Not ported (they queue messages).
        }
        // = seg000:6ffa or bitfield_10,100h — the troop is working.
        self.troops[ti].bitfield_10 |= 0x100;
        // = seg000:6fff call troop_update_harvest_rate — this period's
        //   harvest, which also refreshes harvest_rate (the "Current: N kgs/h"
        //   rate) and its trend.
        let harvested = self.troop_update_harvest_rate(ti);
        // = seg000:7003..7008 harvest_total += it — the running total the
        //   info panel averages into "Average: N kgs/h".
        let old_total = self.troops[ti].harvest_total;
        let total = old_total.wrapping_add(harvested);
        self.troops[ti].harvest_total = total;
        // = seg000:700b..7016 every time that total crosses a multiple of 128
        //   the troop gets better at mining (skill +1, marker 1).
        if (total ^ old_total) & 0xff80 != 0 {
            self.troop_increase_spice_skill(ti, 1, 0);
        }
        // = seg000:701b..702a the player's cut: a tenth of the harvest, the
        //   remainder carried to the next period.
        let pot = harvested.wrapping_add(self.spice_harvest_remainder);
        self.spice_harvest_remainder = pot % 10;
        self.spice_in_stock = self.spice_in_stock.wrapping_add(pot / 10);
        // = seg000:702f..703a the ground's cut: (harvest + the location's
        //   carry) / spice_amount is how much density goes, the remainder
        //   staying in field_13 towards the next period. spice_amount is the
        //   ground's yield per density point, so a rich field depletes slower.
        let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        let Some(loc) = self.locations.get(li).copied() else {
            return;
        };
        // = seg000:702f/7032 al = the harvest's low byte + field_13, carried
        //   into ah; 7038 div cl — an 8-bit divide, so a spice_amount of 0
        //   would trap in DOS and the quotient is capped at 0xff.
        let dividend = (harvested & 0xff) + loc.field_13 as u16 + (harvested & 0xff00);
        if loc.spice_amount == 0 {
            return;
        }
        let consumed = (dividend / loc.spice_amount as u16).min(0xff) as u8;
        self.locations[li].field_13 = (dividend % loc.spice_amount as u16) as u8;
        // = seg000:703d..704e with the spice-density overlay up, eating past
        //   the shade the overlay currently draws marks it for a repaint.
        if consumed > (loc.spice_density & 0x0f) && self.data_046eb & 0x40 != 0 {
            self.spice_density_overlay_dirty = self.spice_density_overlay_dirty.wrapping_add(1);
        }
        // = seg000:7052..705b the density drops, clamped at 0.
        self.locations[li].spice_density = loc.spice_density.saturating_sub(consumed);
    }

    // = seg000:6b96 troop_location_test_spice_mining_viable, called directly
    // (the occupation is known to be 0 here). True = NOT viable, DOS's carry.
    fn troop_spice_mining_not_viable(&mut self, ti: usize) -> bool {
        self.troop_occupation_not_viable(ti)
    }

    // = seg000:7085 callback_troop_make_troop_stop_working — occupation bit 4,
    // the "stopped" bit the icon script and the info panel read.
    pub(crate) fn troop_make_stop_working(&mut self, ti: usize) {
        self.troops[ti].occupation |= 0x10;
    }

    // = seg000:6edd troop_clamp_skill_and_do_something_else — raise the troop's
    // spice skill by `by`, capped at 0x5f. When the raise carries the skill
    // into a new rank (its high nibble changes) the rank-up is marked in
    // bitfield_10 bits 0-1 as `marker + 1`, which the troop's dialogue reads.
    fn troop_increase_spice_skill(&mut self, ti: usize, by: u8, marker: u8) {
        let before = self.troops[ti].spice_skill;
        let raised = before.wrapping_add(by).min(0x5f);
        self.troops[ti].spice_skill = raised;
        // = seg000:6eeb..6ef9 the rank test + the marker.
        if (raised ^ before) & 0xf0 != 0 {
            let t = &mut self.troops[ti];
            t.bitfield_10 = (t.bitfield_10 & 0xfffc) | (marker as u16 + 1);
        }
    }

    // = seg000:6acb troop_set_occupation — give the troop the occupation `new`
    // (the low nibble; the DOS store writes the whole byte, so the 0x10..0x80
    // bits go with it). Re-tests whether the troop can actually do the job
    // where it stands, refreshes its map icon and the order menu, and restarts
    // its occupation clocks. A no-op when the occupation is already `new`.
    pub(crate) fn troop_set_occupation(&mut self, ti: usize, new: u8) {
        // = seg000:6acb..6ad2 cmp occupation & 0xf, cl; jz ret.
        if self.troops[ti].occupation & 0x0f == new {
            return;
        }
        self.troop_reinit_occupation(ti, new);
    }

    // = seg000:6ad4 troop_06ad4 — the inner entry of troop_set_occupation
    // (no same-occupation early-out): the arrival paths re-enter it to
    // re-initialize an unchanged occupation after a move (troop_arrive_at_
    // destination / troop_settle_into_location), and troop_06ac5 (seg000:6ac5)
    // passes occupation & 0xfc through it to drop an espionage/attack
    // mission's low bits.
    pub(crate) fn troop_reinit_occupation(&mut self, ti: usize, new: u8) {
        let mut new = new;
        // = seg000:6ad4..6ae8 assigning ecology (8) at the one location whose
        //   record is seg001:07c8 while it holds no bulbs makes it bulb
        //   growing (0x0a) instead — that location grows the first bulbs.
        let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        if new == 8
            && self.troops[ti].offset_of_location == 0x07c8
            && self.locations[li].equipment.bulbs == 0
        {
            new = 0x0a;
        }
        // = seg000:6aea..6af1 the occupation byte, the speech bits 4-5 and
        //   bitfield_10 bit 8 (the "working" flag re-derived just below).
        self.troops[ti].occupation = new;
        self.troops[ti].dissatisfaction_and_speech &= 0xffcf;
        self.troops[ti].bitfield_10 &= 0xfeff;
        // = seg000:6af6..6afb the viability test sets the working flag.
        if !self.troop_occupation_not_viable(ti) {
            self.troops[ti].bitfield_10 |= 0x100;
        }
        // = seg000:6b00 call troop_refresh_icon — the icon sprite follows the
        //   occupation, so respawn it.
        self.troop_refresh_icon(ti);
        // = seg000:6b03 call troop_set_timePeriodOfRalliement_...
        self.troop_reset_occupation_clocks(ti);
        // = seg000:6b06..6b16 every occupation but "awaiting orders" (2) marks
        //   its class in the speech word (0x2000 << class), the flag the
        //   troop's dialogue reads to comment on its new job.
        if self.troops[ti].occupation != 2 {
            let class = self.troop_get_occupation_bits_2_and_3(ti);
            self.troops[ti].dissatisfaction_and_speech |= 0x2000u16 << class;
        }
        // = seg000:6b19..6b21 the contacted troop's order menu greys change
        //   with its occupation.
        if self.troops[ti].troop_id == self.map_selected_troop_id {
            self.map_setup_troop_dialog_menu(ti);
        }
    }

    // = seg000:6b25 troop_set_timePeriodOfRalliement_clear_troop_occupation_
    // dependent_fields — restart the troop's occupation clocks: the current
    // game_time as the new baseline, and the two accumulators (the spice /
    // battle totals the info panel reads) cleared.
    fn troop_reset_occupation_clocks(&mut self, ti: usize) {
        self.troops[ti].time_period_of_ralliement = self.game_time;
        self.troops[ti].harvest_rate = 0;
        self.troops[ti].harvest_total = 0;
    }

    // = seg000:693b troop_get_occupation_bits_2_and_3_0693b — the occupation
    // class: 0 spice, 1 army, 2 ecology.
    pub(crate) fn troop_get_occupation_bits_2_and_3(&self, ti: usize) -> u8 {
        (self.troops[ti].occupation & 0x0f) >> 2
    }

    // = seg000:6c15 troop_call_callback_according_to_occupation — can the troop
    // actually do its occupation where it stands? Dispatches on the occupation
    // nibble through array_callbacks_for_troop_occupation_06bf5; only spice
    // mining (0), spice prospecting (1) and irrigation (8) test anything, every
    // other occupation is nullsub_00f66 (always viable).
    //
    // Returns DOS's carry: true = NOT viable. The shared tail
    // (troop_sync_occupation_stopped_bit) syncs occupation bit 0x10 — the
    // "stopped working" bit the icon script reads — with the answer,
    // refreshing the icon and restarting the clocks when the troop starts
    // working again.
    fn troop_occupation_not_viable(&mut self, ti: usize) -> bool {
        let t = self.troops[ti];
        let li = locations::location_index_from_ptr(t.offset_of_location);
        let loc = self.locations[li];
        let not_viable = match t.occupation & 0x0f {
            // = seg000:6b96 troop_location_test_spice_mining_viable — spice mining needs an
            //   undamaged harvester (bitfield_10 bit 9 clear), a troop that is
            //   not sulking (speech bits 4-5 clear), spice in the ground, and
            //   an area that has been prospected but not exhausted (status bit
            //   6 set, bit 0 clear).
            0 => {
                t.bitfield_10 & 0x200 != 0
                    || t.dissatisfaction_and_speech & 0x30 != 0
                    || loc.spice_density < 1
                    || (loc.status ^ 0x40) & 0x41 != 0
            }
            // = seg000:6b8a troop_location_test_for_location_area_prospected —
            //   prospecting only pays where the area is not prospected yet.
            1 => loc.status & 0x41 != 0,
            // = seg000:6bd7 troop_location_test_irrigation_viable — irrigation needs a
            //   non-sulking troop, water at the location, its status bit 5, and
            //   the troop's own bulb equipment (mask 2).
            8 => {
                t.dissatisfaction_and_speech & 0x30 != 0
                    || loc.water < 1
                    || loc.status & 0x20 == 0
                    || t.equipment & 2 == 0
            }
            // = the nullsub_00f66 slots: nothing to test.
            _ => false,
        };
        // = seg000:6bb6..6bd6 troop_sync_occupation_stopped_bit — the answer
        //   lives in occupation bit 0x10; flipping it swaps the icon, and when the
        //   troop resumes working, restarts its clocks. The carry the caller
        //   reads is the viability answer, not the flip (DOS pushf/popf).
        let stopped = self.troops[ti].occupation & 0x10 != 0;
        if stopped != not_viable {
            self.troops[ti].occupation ^= 0x10;
            self.troop_refresh_icon(ti);
            // = seg000:6bce/6bd2 test al,10h; jz — only the working-again
            //   direction restarts the clocks.
            if !not_viable {
                self.troop_reset_occupation_clocks(ti);
            }
        }
        not_viable
    }

    // = seg000:913b char_to_sprite_walk_facing — the PERS sprite pair and the
    // idle expression for the walk/facing persons 0x0e..0x10: the sprite is
    // 0x0e + (troop_id % 3) (the three Fremen figures), the expression
    // (troop_id / 3) % (15 or 17) + 1. Person 0x0e reads fremen1_troop_ptr;
    // 0x0f/0x10 read fremen2_troop_ptrs[selected_fremen2_index] unless
    // game_phase == 0xc8 (then the raw id with expression 0).
    pub(crate) fn walk_facing_sprite(&self, id: u8) -> (u8, u8) {
        let troop = if id == 0x0e {
            self.fremen1_troop
        } else if self.game_phase == 0xc8 {
            // = seg000:9143/9148 jz char_to_sprite_store_expr.
            return (id, 0);
        } else {
            self.fremen2_troops[(self.selected_fremen2 & 7) as usize]
        };
        let Some(ti) = troop else {
            // No troop classified (an empty slot): DOS would chase a stale
            // pointer; keep the raw id.
            return (id, 0);
        };
        // = seg000:9155..916f al = troop_id; /3; the remainder picks the
        //   figure, the quotient (mod 15, or 17 when the remainder is 0)
        //   + 1 the expression.
        let tid = self.troops[ti].troop_id;
        let quot = tid / 3;
        let rem = tid % 3;
        let modulus = if rem == 0 { 0x0f } else { 0x11 };
        (0x0e + rem, quot % modulus + 1)
    }
}

// = troop movement (seg000:8308..8684): the per-period travel step of a
// moving troop (occupation bit 6), its arrival, and the settle/turn-around
// machinery behind it.
impl GameState {
    // = seg000:8308 troop_travel_step — one time period of a moving troop:
    // 4 sub-steps (8 with an ornithopter, equipment bit 6), each added to
    // the gps position; a zero delta is arrival. Afterwards the map icon
    // follows the new position: removed off-screen, recentred on-screen, or
    // spawned fresh from the moving-icon script.
    pub(crate) fn troop_travel_step(&mut self, ti: usize) {
        // = seg000:8308..8311 cx = 4, 8 with equipment bit 6.
        let steps = if self.troops[ti].equipment & 0x40 != 0 {
            8
        } else {
            4
        };
        self.troop_travel_substeps(ti, steps);
    }

    // = seg000:8313 loc_08313 — the sub-step loop and its icon-upkeep tail;
    // troop_issue_move_order enters here directly with cx = 7 (seg000:8518)
    // to give a freshly-ordered troop its head start.
    pub(crate) fn troop_travel_substeps(&mut self, ti: usize, steps: u16) {
        for _ in 0..steps {
            // = seg000:8314..831c a zero delta = arrival (troop_08357).
            let (dlng, dlat) = self.troop_travel_substep_delta(ti);
            if dlng == 0 && dlat == 0 {
                self.troop_arrive_at_destination(ti);
                return;
            }
            // = seg000:831e/8321 gps += the sub-step.
            let t = &mut self.troops[ti];
            t.gps_coordinates_1 = t.gps_coordinates_1.wrapping_add(dlng as u16);
            t.gps_coordinates_2 = t.gps_coordinates_2.wrapping_add(dlat as u16);
        }
        // = seg000:8326..8344 the icon upkeep.
        let pos = self.troop_icon_screen_pos(ti, None);
        let icon = self.troop_find_icon(ti);
        match (pos, icon) {
            // = seg000:8333/8338 scrolled off the window: drop the icon.
            (None, Some(i)) => self.troop_icon_remove(i),
            (None, None) => {}
            // = seg000:8330 -> loc_0c653: recentre the icon on the projected
            //   position (centre - the sprite half-size - the current rect).
            (Some((x, y)), Some(i)) => {
                self.open_onmap_spritesheet();
                let sprite = self.troop_icons[i].sprite;
                let (mut w, mut h) = (0i16, 0i16);
                self.with_active_bank_sheet(|_, sheet| {
                    if let Some(s) = sheet.get_sprite(sprite) {
                        w = s.width() as i16;
                        h = s.height() as i16;
                    }
                });
                let r = self.troop_icons[i].rect;
                self.troop_icon_move_and_redraw(i, x - w / 2 - r.x0, y - h / 2 - r.y0);
            }
            // = seg000:833c..8344 entered the window: spawn the moving icon
            //   and repaint its rect.
            (Some((x, y)), None) => {
                let script = self.troop_icon_pick_script_moving(ti);
                if script != 0 {
                    if let Some(i) = self.troop_icon_spawn_with_anim(script, x, y, ti) {
                        let r = self.troop_icons[i].rect;
                        self.troop_icons_update_dirty_rect(r);
                    }
                }
            }
        }
    }

    // = seg000:8604 troop_travel_substep_delta — one sub-step toward the
    // destination location: (dlng, dlat) to add to the gps position, (0, 0)
    // = arrived. The longitude gap is scaled to map cells by
    // lng_units_per_cell_table[|lat|]; arrival is a dominant-axis distance
    // below 7. Longitude-dominant: one cell toward the target (a sub-cell
    // remainder is snapped whole), latitude ±1 only on a set rolling rand
    // bit; latitude-dominant: ±1 row, longitude one cell only on a set rand
    // bit (a sub-cell longitude gap is snapped whole).
    fn troop_travel_substep_delta(&mut self, ti: usize) -> (i16, i16) {
        let t = self.troops[ti];
        let li = locations::location_index_from_ptr(t.offset_of_location);
        let loc = self.locations[li];
        // = seg000:860b..8616 bp = lng_units_per_cell_table[|lat|] (the port
        //   derives the same value through the tablat row length).
        let lat = t.gps_coordinates_2 as i16;
        let per_cell = self
            .tablat
            .as_ref()
            .expect("TABLAT.BIN not loaded")
            .lng_units_per_cell((lat + 98) as u16);
        // = seg000:861a..8628 the latitude gap and its sign.
        let dlat_total = loc.map_y.wrapping_sub(lat);
        let lat_sign: i16 = if dlat_total >= 0 { 1 } else { -1 };
        let alat = dlat_total.unsigned_abs();
        // = seg000:8628..8637 the longitude gap (the wrapping sub takes the
        //   short way around the planet) and its cell count.
        let dlng_total = (loc.map_x as u16).wrapping_sub(t.gps_coordinates_1) as i16;
        let cells = dlng_total.unsigned_abs() / per_cell;
        let lng_step = if dlng_total >= 0 {
            per_cell as i16
        } else {
            -(per_cell as i16)
        };
        // = seg000:863b/863d the dominant-axis split.
        if cells >= alat {
            // = seg000:863f the arrival radius.
            if cells < 7 {
                return (0, 0);
            }
            // = seg000:8644..8648 no latitude gap left: no latitude step.
            let dlat_step = if alat == 0 { 0 } else { lat_sign };
            // = seg000:8650/8652 the sub-cell snap (unreachable here: cells
            //   >= 7 survives the halving; kept for the DOS shape).
            if cells >> 1 == 0 {
                return (dlng_total, dlat_step);
            }
            // = seg000:8654..865c one cell across, diagonal on a rand bit.
            let diagonal = self.roll_rand_bit();
            (lng_step, if diagonal { dlat_step } else { 0 })
        } else {
            // = seg000:8666/8669 the arrival radius.
            if alat < 7 {
                return (0, 0);
            }
            // = seg000:866b/866d a sub-cell longitude gap snaps whole.
            if cells == 0 {
                return (dlng_total, lat_sign);
            }
            // = seg000:8675/8677 (unreachable: alat >= 7; the DOS shape).
            if alat >> 1 == 0 {
                return (dlng_total, lat_sign);
            }
            // = seg000:8679..8683 one row down/up, a cell across on a rand
            //   bit.
            let diagonal = self.roll_rand_bit();
            (if diagonal { lng_step } else { 0 }, lat_sign)
        }
    }

    // = seg000:8357 troop_arrive_at_destination — a moving troop reached its
    // destination: snap the gps onto the location, then settle or resolve
    // the arrival by owner and occupation.
    pub(crate) fn troop_arrive_at_destination(&mut self, ti: usize) {
        // = seg000:8357..8365 the prospector troop (troops[2]) arriving at
        //   the head of its destination queue shifts it.
        if ti == 2 && self.prospector_destinations[0] == self.troops[ti].offset_of_location {
            self.shift_prospector_destinations();
        }
        // = seg000:8368..836b al = the occupation nibble (read before the
        //   settle path re-initializes it).
        let occ_low = self.troops[ti].occupation & 0x0f;
        let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        // = seg000:836d..8379 gps = the location's coordinates.
        self.troops[ti].gps_coordinates_1 = self.locations[li].map_x as u16;
        self.troops[ti].gps_coordinates_2 = self.locations[li].map_y as u16;
        // = seg000:837c..8385 a battle-flagged (status bit 1) or Atreides
        //   location settles the troop.
        if self.locations[li].status & 2 != 0 || self.location_is_atreides(li) {
            self.troop_settle_into_location(ti);
            if occ_low == 5 {
                // = seg000:839a..83a4 an espionage arrival at a hidden
                //   location reveals it.
                if self.locations[li].status & 0x80 != 0 {
                    self.locations[li].status &= 0x7f;
                    self.location_mark_map_and_minimap_dirty(li);
                } else {
                    self.troop_location_notify_residents(li);
                }
            } else if self.troops[ti].troop_id == self.locations[li].troop_id {
                // = seg000:8390..8397 settling as the chain head is the
                //   battle won (troop_location_07429): the messages, the
                //   post-battle troop callbacks, the Harkonnen enslaving and
                //   the final-attack staging. Not ported. TODO.
                println!("troop_arrive_at_destination: battle won (seg000:7429) not ported");
            } else {
                self.troop_location_notify_residents(li);
            }
            return;
        }
        // = seg000:83a7 the non-Atreides arrival.
        let mut class = occ_low;
        if (5..=6).contains(&occ_low) {
            // = seg000:83af..83b4 espionage/attack drop the mission's low
            //   bits (troop_06ac5: occupation & 0xfc through the
            //   set-occupation reinit).
            let new = self.troops[ti].occupation & 0xfc;
            self.troop_reinit_occupation(ti, new);
            class = 0;
        }
        // = seg000:83b6..83ba occupation low bits 11b turn around for home;
        //   everyone else settles.
        if class & 3 == 3 {
            self.troop_turn_around_home(ti);
        } else {
            self.troop_settle_into_location(ti);
        }
    }

    // = seg000:8347 shift_prospector_troop_destinations_array — drop the
    // head of the 4-entry queue (the tail entry stays).
    fn shift_prospector_destinations(&mut self) {
        self.prospector_destinations.copy_within(1..4, 0);
    }

    // = seg000:83bc troop_location_finish_troop_motion_and_settle_it_into_
    // location — link the troop into its destination, clear the moving bit,
    // fold its equipment into the location and restart its occupation; an
    // arrival at the current location's audience room (room 1 during the
    // night attack) requests a room redraw.
    fn troop_settle_into_location(&mut self, ti: usize) {
        let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        // = seg000:83bc call troop_link_into_location_and_assign_position.
        self.troop_link_into_location(ti, li);
        // = seg000:83bf and occupation,0bfh — the moving bit ends here.
        self.troops[ti].occupation &= 0xbf;
        // = seg000:83c3 call troop_location_085cc.
        self.troop_fix_position_bank(ti, li);
        // = seg000:83c6 call troop_location_register_troop_equipment_...
        self.troop_register_equipment_into_location(ti, li);
        // = seg000:83c9..83ce an unrallied troop (occupation bit 7) is done.
        let occ = self.troops[ti].occupation;
        if occ & 0x80 != 0 {
            return;
        }
        // = seg000:83d0/83d3 re-init the occupation from its low nibble
        //   (troop_06ad4 — refreshes the working flag, the icon and the
        //   clocks).
        self.troop_reinit_occupation(ti, occ & 0x0f);
        // = seg000:83d6..83f7 arriving at the current location while the
        //   room verb menu is up, with the player in the audience room
        //   (room 1 during the night attack): request the room redraw.
        if li != self.current_location_index as usize {
            return;
        }
        if self.get_active_screen_element()
            != crate::room_game_screen::ScreenElement::RoomCommandMenu
        {
            return;
        }
        let room = if self.night_attack_stage == 0 { 2 } else { 1 };
        if self.current_room == room {
            self.room_redraw_request |= 1;
        }
    }

    // = seg000:851f troop_link_into_location_and_assign_position — append
    // the troop to the location's chain: an empty chain takes it as the head
    // at position 1 (9 for a Harkonnen troop, the mirrored slot bank);
    // otherwise it links after the tail and takes the first free position
    // from a 30-slot occupancy scan (Harkonnen troops scan from slot 9).
    fn troop_link_into_location(&mut self, ti: usize, li: usize) {
        let id = self.troops[ti].troop_id;
        let harkonnen = self.troops[ti].bitfield_10 & 0x80 != 0;
        // = seg000:8521..853f the empty chain.
        let head = self.locations[li].troop_id;
        if head == 0 {
            self.locations[li].troop_id = id;
            self.troops[ti].position = if harkonnen { 9 } else { 1 };
            return;
        }
        // = seg000:8540..8565 the occupancy bitmap over the chain, tracking
        //   the tail.
        let mut used = [false; 0x1e];
        let mut cur = head;
        let tail = loop {
            let t = &self.troops[(cur - 1) as usize];
            let p = t.position as usize;
            if (1..=0x1e).contains(&p) {
                used[p - 1] = true;
            }
            if t.next_troop_id == 0 {
                break cur;
            }
            cur = t.next_troop_id;
        };
        // = seg000:8567 link after the tail.
        self.troops[(tail - 1) as usize].next_troop_id = id;
        // = seg000:856a..8588 the first free slot; Harkonnen troops scan the
        //   mirrored bank (9..30). A full bank falls out at position 0x1e.
        let start = if harkonnen { 8 } else { 0 };
        let position = (start..0x1e)
            .find(|&k| !used[k])
            .map_or(0x1e, |k| k as u8 + 1);
        self.troops[ti].position = position;
    }

    // = seg000:85cc troop_location_085cc — a non-Harkonnen troop that landed
    // in the Harkonnen slot bank (position > 8) re-rolls: unlink it, remove
    // the chain's first captured non-Harkonnen troop from play to free a
    // slot, and assign a position again.
    fn troop_fix_position_bank(&mut self, ti: usize, li: usize) {
        // = seg000:85cc..85d7 the gates.
        if self.troops[ti].bitfield_10 & 0x80 != 0 || self.troops[ti].position <= 8 {
            return;
        }
        // = seg000:85d9 call troop_unlink_from_location_chain.
        self.troop_unlink_from_location_chain(ti);
        // = seg000:85dd..85fc the first non-Harkonnen captured troop
        //   (bitfield_10 bit 7 clear, occupation bit 5) leaves play.
        let mut id = self.locations[li].troop_id;
        while id != 0 {
            let tj = (id - 1) as usize;
            id = self.troops[tj].next_troop_id;
            if self.troops[tj].bitfield_10 & 0x80 == 0 && self.troops[tj].occupation & 0x20 != 0 {
                self.troop_remove_from_play(tj);
                break;
            }
        }
        // = seg000:8600 assign a position again.
        self.troop_link_into_location(ti, li);
    }

    // = seg000:858c troop_unlink_from_location_chain — take the troop out of
    // its location's chain (the head moves to its next, a middle entry's
    // predecessor takes it) and clear its next id. A still-moving troop
    // (occupation bit 6) is not in any chain.
    pub(crate) fn troop_unlink_from_location_chain(&mut self, ti: usize) {
        let id = self.troops[ti].troop_id;
        let li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        if self.troops[ti].occupation & 0x40 == 0 {
            if self.locations[li].troop_id == id {
                // = seg000:85c2 the head.
                self.locations[li].troop_id = self.troops[ti].next_troop_id;
            } else {
                // = seg000:85a4..85b7 the predecessor walk.
                let mut cur = self.locations[li].troop_id;
                while cur != 0 {
                    let tj = (cur - 1) as usize;
                    cur = self.troops[tj].next_troop_id;
                    if cur == id {
                        self.troops[tj].next_troop_id = self.troops[ti].next_troop_id;
                        break;
                    }
                }
            }
        }
        // = seg000:85ba/85bc.
        self.troops[ti].next_troop_id = 0;
    }

    // = seg000:66b1 troop_remove_from_play — unlink the troop, drop its map
    // icon, and park it at the troops-table terminator sentinel: gone from
    // the game but still a record.
    pub(crate) fn troop_remove_from_play(&mut self, ti: usize) {
        self.troop_unlink_from_location_chain(ti);
        if let Some(icon) = self.troop_find_icon(ti) {
            self.troop_icon_remove(icon);
        }
        // = seg000:66c0..66c9 the location word points at troops[67]+1
        //   (seg001:0fbc) — NOT a valid location record; occupation 0xa0
        //   keeps every walk away from it, so nothing converts the sentinel
        //   back to an index.
        self.troops[ti].offset_of_location = 0x0fbc;
        self.troops[ti].occupation = 0xa0;
        self.troops[ti].population = 0;
    }

    // = seg000:7f5f troop_location_register_troop_equipment_into_location_
    // equipment — fold the troop's held-equipment bits into the location's
    // equipment counts (bit 7 = harvesters .. bit 1 = bulbs).
    fn troop_register_equipment_into_location(&mut self, ti: usize, li: usize) {
        let eq = self.troops[ti].equipment;
        for slot in 0..7 {
            if eq & (0x80 >> slot) != 0 {
                let s = self.locations[li].equipment.slot_mut(slot);
                *s = s.wrapping_add(1);
            }
        }
    }

    // = seg000:83fd troop_location_083fd (+ callback_troop_08403) — the
    // arrival notification for the location's hired residents: spice miners
    // stop working, captured troops and those already defending are left
    // alone, everyone else switches to occupation 6 (defending).
    fn troop_location_notify_residents(&mut self, li: usize) {
        self.for_each_hired_troop_in_location(li, |s, tj| {
            let occ = s.troops[tj].occupation;
            if occ & 0x20 != 0 {
                return;
            }
            match occ & 0x0f {
                1 => s.troop_make_stop_working(tj),
                6 => {}
                _ => s.troop_set_occupation(tj, 6),
            }
        });
    }

    // = seg000:841f loc_0841f — the turn-around arrival (occupation low bits
    // 11b at a hostile location): away from home the troop picks up the
    // equipment it came for (the +0x0e word: slot low, troop equipment bit
    // high; only while the location has a free unit) and re-targets home
    // (+0x0c); at home it drops the mission bits and settles.
    fn troop_turn_around_home(&mut self, ti: usize) {
        let home = self.troops[ti].harvest_rate;
        let dest = self.troops[ti].offset_of_location;
        if dest != home {
            let li = locations::location_index_from_ptr(dest);
            // = seg000:8428 refresh the location's free equipment.
            self.compute_location_available_equipment(li);
            // = seg000:842c..8442 take a free unit: the location loses one,
            //   the troop gains the equipment bit, the word's high byte
            //   clears (consumed).
            let word = self.troops[ti].harvest_total;
            let slot = (word & 0xff) as usize;
            if slot < 7 && self.available_equipment.slot(slot) != 0 {
                let s = self.locations[li].equipment.slot_mut(slot);
                *s = s.wrapping_sub(1);
                self.troops[ti].equipment |= (word >> 8) as u8;
                self.troops[ti].harvest_total = word & 0xff;
            }
            // = seg000:8445..844b head home.
            self.troops[ti].offset_of_location = home;
            self.troop_refresh_icon(ti);
        } else {
            // = seg000:844d..8451 home: drop the mission bits and settle.
            self.troops[ti].occupation &= 0xfc;
            self.troop_settle_into_location(ti);
        }
    }

    // = seg000:84a6 troop_issue_move_order — order the troop to `dest_li`.
    // The prospector re-syncs against its destination queue and moves to its
    // head; a troop already moving just retargets; a stationed troop leaves
    // its chain (with the leaving side effects) and starts moving with a
    // 7-sub-step head start.
    pub(crate) fn troop_issue_move_order(&mut self, ti: usize, dest_li: usize) {
        let mut dest_ptr = locations::location_ptr_from_index(dest_li);
        // = seg000:84a6..84af the prospector (troops[2]) moves to its queue
        //   head instead; an empty queue is no order.
        if ti == 2 {
            let Some(head) = self.prospector_sync_destination_queue() else {
                return;
            };
            dest_ptr = head;
        }
        // = seg000:84b2..84c8 already moving: retarget; a fetch mission
        //   (occupation low bits 11b) drops its low bits; the icon follows
        //   the (possibly changed) facing.
        if self.troops[ti].occupation & 0x40 != 0 {
            self.troops[ti].offset_of_location = dest_ptr;
            if self.troops[ti].occupation & 3 == 3 {
                self.troops[ti].occupation &= 0xfc;
            }
            self.troop_refresh_icon(ti);
            return;
        }
        // = seg000:84ca call troop_06ebf — the hired troops left behind with
        //   the just-reassigned speech bit re-derive their occupation
        //   (callback_troop_06ecb).
        let old_li = locations::location_index_from_ptr(self.troops[ti].offset_of_location);
        self.for_each_hired_troop_in_location(old_li, |s, tj| {
            if s.troops[tj].dissatisfaction_and_speech & 0x10 != 0 {
                let nibble = s.troops[tj].occupation & 0x0f;
                s.troop_reinit_occupation(tj, nibble);
            }
        });
        // = seg000:84ce call troop_unlink_from_location_chain.
        self.troop_unlink_from_location_chain(ti);
        // = seg000:84d2..84fe a defending troop (occupation exactly 6)
        //   leaving:
        if self.troops[ti].occupation == 6 {
            // = seg000:84d8..84ee leaving a battle-flagged location as the
            //   last defender collapses the defence (location_do_
            //   accumulation_05098 + location_battle_lost_...). Not ported.
            //   TODO.
            if self.locations[old_li].status & 2 != 0 {
                println!(
                    "troop_issue_move_order: the last-defender battle-lost check (seg000:84d8) not ported"
                );
            }
            // = seg000:84f0..84fe heading for a peaceful sietch costs 3
            //   motivation; another battle destination aborts the order.
            if self.locations[dest_li].appearance < 0x28 {
                if self.locations[dest_li].status & 2 != 0 {
                    return;
                }
                self.troop_decrease_motivation(ti, 3);
            }
        }
        // = seg000:8501..850f the departure: location word = the destination
        //   (di takes the old one), the moving bit, position 0, the troop's
        //   equipment leaves the old location, the icon follows.
        self.troops[ti].offset_of_location = dest_ptr;
        self.troops[ti].occupation |= 0x40;
        self.troops[ti].position = 0;
        self.troop_unregister_equipment_from_location(ti, old_li);
        self.troop_refresh_icon(ti);
        // = seg000:8512..851b a hidden troop (bitfield_10 bit 4) stays put;
        //   everyone else gets a 7-sub-step head start (jmp loc_08313).
        if self.troops[ti].bitfield_10 & 0x10 == 0 {
            self.troop_travel_substeps(ti, 7);
        }
    }

    // = seg000:848f prospector_sync_destination_queue — drop leading queue
    // entries already reached (the head equals the prospector's current
    // destination while it is not moving); None = the queue is empty,
    // otherwise the head the move order goes to.
    fn prospector_sync_destination_queue(&mut self) -> Option<u16> {
        loop {
            let head = self.prospector_destinations[0];
            if head != 0
                && self.troops[2].offset_of_location == head
                && self.troops[2].occupation & 0x40 == 0
            {
                self.shift_prospector_destinations();
                continue;
            }
            // = seg000:84a3 or di,di — ZF for the caller.
            return if head == 0 { None } else { Some(head) };
        }
    }

    // = seg000:7f75 troop_location_unregister_troop_equipment_from_location_
    // equipment — the inverse of the register: subtract the troop's
    // held-equipment bits from the location's counts, clamped at 0.
    fn troop_unregister_equipment_from_location(&mut self, ti: usize, li: usize) {
        let eq = self.troops[ti].equipment;
        for slot in 0..7 {
            if eq & (0x80 >> slot) != 0 {
                let s = self.locations[li].equipment.slot_mut(slot);
                *s = s.saturating_sub(1);
            }
        }
    }

    // = seg000:6f93 troop_decrease_motivation — motivation -= `by`, clamped
    // at 0; dropping below 5 pins it at 4, stops the troop working and sets
    // the demotivated speech bit (0x20).
    pub(crate) fn troop_decrease_motivation(&mut self, ti: usize, by: u8) {
        let t = &mut self.troops[ti];
        t.motivation = t.motivation.saturating_sub(by);
        if t.motivation < 5 {
            t.motivation = 4;
            self.troop_make_stop_working(ti);
            self.troops[ti].dissatisfaction_and_speech |= 0x20;
        }
    }
}

impl GameState {
    // = seg000:66ce troop_rally_troop_066ce — rally the chief's troop to the
    // Atreides cause. Only an un-rallied Fremen troop qualifies (occupation
    // bit 7 set, bitfield_10 bit 7 clear): number_of_rallied_troops rises
    // (reaching the Leto-killed threshold advances the story to phase 0x4c),
    // charisma +1, the occupation becomes "rallied, awaiting orders"
    // ((occupation & 0x20) | 2), the ralliement clocks are stamped, and the
    // location gains a discovery phase.
    pub(crate) fn troop_rally_troop(&mut self, ti: usize) {
        let t = self.troops[ti];
        // = seg000:66ce/66d4 the qualification tests.
        if t.occupation & 0x80 == 0 || t.bitfield_10 & 0x80 != 0 {
            return;
        }
        // = seg000:66da..66ea the rally count + the Leto-killed threshold.
        self.number_of_rallied_troops = self.number_of_rallied_troops.wrapping_add(1);
        if self.number_of_rallied_troops >= self.number_of_rallied_troops_for_leto_killed {
            self.set_game_phase_and_trigger_callbacks(0x4c);
        }
        // = seg000:66ee/66f0 al = 1; call increase_charisma...
        self.increase_charisma(1);
        let game_time = self.game_time;
        let day = self.get_ingame_day_in_ax() as u8;
        {
            let t = &mut self.troops[ti];
            // = seg000:66f3/66f7 occupation = (occupation & 0x20) | 2.
            t.occupation = (t.occupation & 0x20) | 2;
            // = seg000:66fb troop_set_timePeriodOfRalliement_clear_troop_
            //   occupation_dependent_fields_06b25.
            t.time_period_of_ralliement = game_time;
            t.harvest_rate = 0;
            t.harvest_total = 0;
            // = seg000:66fe/6701 game_day_of_ralliement = the current day.
            t.game_day_of_ralliement = day;
        }
        // = seg000:6704..6711 a location without a discovery phase gets
        //   discoverable_at_phase = 2 and the map influence spread
        //   (location_0644e). TODO: the spread (self-modifying map walk) is
        //   not ported.
        let loc_index = locations::location_index_from_ptr(t.offset_of_location);
        if loc_index < self.locations.len() && self.locations[loc_index].discoverable_at_phase == 0
        {
            self.locations[loc_index].discoverable_at_phase = 2;
            println!("troop_rally_troop: location_0644e (map influence spread) unported");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use crate::{GameState, dat_file::DatFile};

    // A moving troop (occupation bit 6) travels toward its destination one
    // period at a time (seg000:8308, 8 sub-steps with an ornithopter) and on
    // arrival settles into the location (seg000:8357/83bc): chained in, a
    // position slot assigned, the moving bit dropped, its equipment folded
    // into the location and the occupation re-initialized. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored troop_moves
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn troop_moves_across_the_map_and_settles() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // An Atreides-held destination with an empty troop chain.
        let li = 20;
        game.locations[li].appearance = 0x28;
        game.locations[li].status = 0x08;
        game.locations[li].troop_id = 0;
        let dest_ptr = crate::locations::location_ptr_from_index(li);
        let (mx, my) = (game.locations[li].map_x, game.locations[li].map_y);
        // Troop 1: moving (bit 6) toward it, class 2 (awaiting orders), an
        // ornithopter aboard (8 sub-steps per period), starting 40 latitude
        // rows south on the destination's meridian.
        let ti = 0;
        game.troops[ti].occupation = 0x42;
        game.troops[ti].bitfield_10 = 0;
        game.troops[ti].dissatisfaction_and_speech = 0;
        game.troops[ti].equipment = 0x40;
        game.troops[ti].offset_of_location = dest_ptr;
        game.troops[ti].gps_coordinates_1 = mx as u16;
        game.troops[ti].gps_coordinates_2 = my.wrapping_sub(40) as u16;
        let orni_before = game.locations[li].equipment.ornithopters;

        // One period: 8 rows closer, still en route.
        game.run_events_for_n_time_periods(1);
        assert_eq!(
            game.troops[ti].gps_coordinates_2,
            my.wrapping_sub(32) as u16,
            "8 latitude rows per period with an ornithopter"
        );
        assert_ne!(game.troops[ti].occupation & 0x40, 0, "still moving");

        // Four more periods reach the arrival radius (|dlat| < 7) and settle.
        game.run_events_for_n_time_periods(4);
        while rx.try_recv().is_ok() {}
        assert_eq!(game.troops[ti].occupation, 2, "settled, moving bit gone");
        assert_eq!(
            (
                game.troops[ti].gps_coordinates_1,
                game.troops[ti].gps_coordinates_2
            ),
            (mx as u16, my as u16),
            "the gps snapped onto the location"
        );
        assert_eq!(
            game.locations[li].troop_id, game.troops[ti].troop_id,
            "chained in as the head"
        );
        assert_eq!(game.troops[ti].next_troop_id, 0);
        assert_eq!(game.troops[ti].position, 1, "the first free slot");
        assert_eq!(
            game.locations[li].equipment.ornithopters,
            orni_before + 1,
            "the ornithopter folded into the location"
        );
    }

    // One time period of spice mining (seg000:6fe5): the troop's running total
    // grows by this period's rate, a tenth of it reaches the player's stock
    // with the remainder carried, and the location's spice density is bitten
    // into at a rate set by its spice_amount. Asset-gated:
    //   cargo test -p dune --bin dune -- --ignored spice_mining
    #[test]
    #[ignore = "needs assets/DUNE.DAT"]
    fn spice_mining_accumulates_stock_and_depletes_the_field() {
        let dat_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/DUNE.DAT");
        let Ok(dat_file) = DatFile::open(dat_path) else {
            eprintln!("skipping: {dat_path} not found");
            return;
        };
        let (tx, rx) = mpsc::sync_channel(64);
        let mut game = GameState::new(dat_file, tx);
        game.set_headless();
        game.start(true);
        while rx.try_recv().is_ok() {}

        // Troop 1 mining a prospected, spice-bearing field with a harvester.
        let ti = 0;
        let li = crate::locations::location_index_from_ptr(game.troops[ti].offset_of_location);
        game.troops[ti].occupation = 0;
        game.troops[ti].equipment |= 0x80;
        game.troops[ti].bitfield_10 = 0;
        game.troops[ti].dissatisfaction_and_speech = 0;
        game.troops[ti].harvest_rate = 0;
        game.troops[ti].harvest_total = 0;
        game.locations[li].spice_density = 0x80;
        game.locations[li].spice_amount = 4;
        game.locations[li].field_13 = 0;
        // = troop_location_test_spice_mining_viable: prospected (status bit 6
        //   set) and not exhausted (bit 0 clear).
        game.locations[li].status = (game.locations[li].status | 0x40) & !1;
        game.spice_in_stock = 0;
        game.spice_harvest_remainder = 0;

        let density_before = game.locations[li].spice_density;
        game.run_troop_occupation_events();

        // The rate landed in harvest_rate (the "Current: N kgs/h" readout) and in
        // the running total harvest_total, and the troop counts as working.
        let rate = game.troops[ti].harvest_rate;
        assert!(rate > 0, "the troop mined something");
        assert_eq!(
            game.troops[ti].harvest_total, rate,
            "the total is this period's"
        );
        assert_eq!(
            game.troops[ti].bitfield_10 & 0x100,
            0x100,
            "the troop is working"
        );
        assert_eq!(
            game.troops[ti].occupation & 0x10,
            0,
            "and not marked stopped"
        );
        // A tenth of it reached the stock, the rest carried.
        assert_eq!(game.spice_in_stock, rate / 10, "a tenth into the stock");
        assert_eq!(game.spice_harvest_remainder, rate % 10, "the rest carried");
        // The field gave up rate / spice_amount of density, the remainder held
        // in field_13.
        let consumed = (rate + 0) / 4;
        assert_eq!(
            game.locations[li].spice_density,
            density_before.saturating_sub(consumed.min(0xff) as u8),
            "the field is depleted at spice_amount per density point"
        );
        assert_eq!(game.locations[li].field_13 as u16, rate % 4, "the carry");

        // A second period adds to both, and the carried remainders roll over
        // rather than being lost.
        let stock_after_one = game.spice_in_stock;
        game.run_troop_occupation_events();
        assert!(
            game.troops[ti].harvest_total > rate,
            "the running total keeps growing"
        );
        assert!(
            game.spice_in_stock >= stock_after_one,
            "the stock keeps growing"
        );

        // An exhausted field (status bit 0) stops the troop: both accumulators
        // clear and occupation bit 4 goes on (loc_0707b).
        game.locations[li].status |= 1;
        game.run_troop_occupation_events();
        assert_eq!(game.troops[ti].occupation & 0x10, 0x10, "stopped working");
        assert_eq!(game.troops[ti].harvest_rate, 0, "no rate to show");
        assert_eq!(game.troops[ti].harvest_total, 0, "no total to average");
    }
}
