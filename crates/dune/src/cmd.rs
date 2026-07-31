//! Named ids for the COMMAND.BIN strings — the values
//! `get_phrase_or_command_string` (seg000:cf70) resolves when bit 0x800 is
//! clear (set selects a PHRASE.BIN dialogue string instead). They are the
//! ids the seg001 command-menu records carry in their `text_id` word and the
//! ids the panel/label draws pass by hand.
//!
//! The id is 1-based: `text_id - 1` indexes COMMAND.BIN's offset table, so a
//! constant here matches its line number in COMMAND1.TXT. The doc comment on
//! each constant is the English (COMMAND1) text; `{…}` placeholders are the
//! live-number substitutions `format_interpolated_string` fills from the
//! staged CONDIT block.

// Generated from COMMAND1.csv by generate_command1_consts.py.

/// " Arrakeen"
pub(crate) const ARRAKEEN: u16 = 0x1;
/// "Carthag"
pub(crate) const CARTHAG: u16 = 0x2;
/// "Tuono"
pub(crate) const TUONO: u16 = 0x3;
/// "Habbanya"
pub(crate) const HABBANYA: u16 = 0x4;
/// "Oxtyn"
pub(crate) const OXTYN: u16 = 0x5;
/// "Tsympo"
pub(crate) const TSYMPO: u16 = 0x6;
/// "Bledan"
pub(crate) const BLEDAN: u16 = 0x7;
/// "Ergsun"
pub(crate) const ERGSUN: u16 = 0x8;
/// "Haga"
pub(crate) const HAGA: u16 = 0x9;
/// "Cielago"
pub(crate) const CIELAGO: u16 = 0xa;
/// "Sihaya"
pub(crate) const SIHAYA: u16 = 0xb;
/// "Celimyn"
pub(crate) const CELIMYN: u16 = 0xc;
/// "(Atreides)"
pub(crate) const ATREIDES_D: u16 = 0xd;
/// "(Harkonnen)"
pub(crate) const HARKONNEN: u16 = 0xe;
/// "Tabr"
pub(crate) const TABR: u16 = 0xf;
/// "Timin"
pub(crate) const TIMIN: u16 = 0x10;
/// "Tuek"
pub(crate) const TUEK: u16 = 0x11;
/// "Harg"
pub(crate) const HARG: u16 = 0x12;
/// "Clam"
pub(crate) const CLAM: u16 = 0x13;
/// "Tsymyn"
pub(crate) const TSYMYN: u16 = 0x14;
/// "Siet"
pub(crate) const SIET: u16 = 0x15;
/// "Pyons"
pub(crate) const PYONS: u16 = 0x16;
/// "Pyort"
pub(crate) const PYORT: u16 = 0x17;
/// "Spice Mining"
pub(crate) const SPICE_MINING_18: u16 = 0x18;
/// "."
pub(crate) const STR_19: u16 = 0x19;
/// "."
pub(crate) const STR_1A: u16 = 0x1a;
/// "."
pub(crate) const STR_1B: u16 = 0x1b;
/// "Military Training"
pub(crate) const MILITARY_TRAINING_1C: u16 = 0x1c;
/// "Espionage"
pub(crate) const ESPIONAGE_1D: u16 = 0x1d;
/// "."
pub(crate) const STR_1E: u16 = 0x1e;
/// "."
pub(crate) const STR_1F: u16 = 0x1f;
/// "Irrigation & Tree Care"
pub(crate) const IRRIGATION_TREE_CARE_20: u16 = 0x20;
/// "Wind-trap Assembly"
pub(crate) const WIND_TRAP_ASSEMBLY_21: u16 = 0x21;
/// "Bulb growing"
pub(crate) const BULB_GROWING_22: u16 = 0x22;
/// "."
pub(crate) const STR_23: u16 = 0x23;
/// "Spice Mining"
pub(crate) const SPICE_MINING_24: u16 = 0x24;
/// "Spice Prospecting"
pub(crate) const SPICE_PROSPECTING: u16 = 0x25;
/// "Awaiting orders"
pub(crate) const AWAITING_ORDERS: u16 = 0x26;
/// "Search for Equipment"
pub(crate) const SEARCH_FOR_EQUIPMENT_27: u16 = 0x27;
/// "Military Training"
pub(crate) const MILITARY_TRAINING_28: u16 = 0x28;
/// "Espionage"
pub(crate) const ESPIONAGE_29: u16 = 0x29;
/// "Attacks"
pub(crate) const ATTACKS: u16 = 0x2a;
/// "Search for Equipment"
pub(crate) const SEARCH_FOR_EQUIPMENT_2B: u16 = 0x2b;
/// "Irrigation & Tree Care"
pub(crate) const IRRIGATION_TREE_CARE_2C: u16 = 0x2c;
/// "Wind-trap Assembly"
pub(crate) const WIND_TRAP_ASSEMBLY_2D: u16 = 0x2d;
/// "Bulb growing"
pub(crate) const BULB_GROWING_2E: u16 = 0x2e;
/// "Search for Equipment"
pub(crate) const SEARCH_FOR_EQUIPMENT_2F: u16 = 0x2f;
/// "{word_A0h}0 kgs"
pub(crate) const STR_30: u16 = 0x30;
/// "{word_AEh}0 kgs\n (+{word_B0h}0)"
pub(crate) const STR_31: u16 = 0x31;
/// "{word_AEh}0 kgs\n (-{word_B2h}0)"
pub(crate) const STR_32: u16 = 0x32;
/// "{word_B4h}0 kgs"
pub(crate) const STR_33: u16 = 0x33;
/// "{word_B6h}0 kgs"
pub(crate) const STR_34: u16 = 0x34;
/// "{word_B8h}0 kgs"
pub(crate) const STR_35: u16 = 0x35;
/// "{word_BAh}0 kgs"
pub(crate) const STR_36: u16 = 0x36;
/// "{word_BCh}0 kg"
pub(crate) const STR_37: u16 = 0x37;
/// " "
pub(crate) const STR_38: u16 = 0x38;
/// " "
pub(crate) const STR_39: u16 = 0x39;
/// "Settled in"
pub(crate) const SETTLED_IN: u16 = 0x3a;
/// "Going to"
pub(crate) const GOING_TO: u16 = 0x3b;
/// "{str_4h}\n{byte_3Ch}0 men  Motiv. {byte_36h}%"
pub(crate) const MEN_AND_MOTIVATION: u16 = 0x3c;
/// "Average: {word_4Ah} kgs/h\nCurrent: {word_48h} kgs/h"
pub(crate) const SPICE_RATES: u16 = 0x3d;
/// "Fremen lost: {word_46h}0\nHarkonnen killed: {word_44h}0"
pub(crate) const BATTLE_LOSSES: u16 = 0x3e;
/// "Repairing"
pub(crate) const REPAIRING: u16 = 0x3f;
/// "Inactive"
pub(crate) const INACTIVE: u16 = 0x40;
/// "Captured"
pub(crate) const CAPTURED: u16 = 0x41;
/// "Freed Prisoner"
pub(crate) const FREED_PRISONER: u16 = 0x42;
/// "Covered Area: {word_46h}%"
pub(crate) const COVERED_AREA: u16 = 0x43;
/// "Sietch: "
pub(crate) const SIETCH: u16 = 0x44;
/// "Palace: "
pub(crate) const PALACE: u16 = 0x45;
/// "Village: "
pub(crate) const VILLAGE: u16 = 0x46;
/// "Fort: "
pub(crate) const FORT: u16 = 0x47;
/// "a sietch"
pub(crate) const A_SIETCH: u16 = 0x48;
/// "a palace"
pub(crate) const A_PALACE: u16 = 0x49;
/// "a village"
pub(crate) const A_VILLAGE: u16 = 0x4a;
/// "a fortress"
pub(crate) const A_FORTRESS: u16 = 0x4b;
/// "Battle:"
pub(crate) const BATTLE: u16 = 0x4c;
/// "ASK FOR MORE INFORMATION"
pub(crate) const ASK_FOR_MORE_INFORMATION: u16 = 0x4d;
/// "MODIFY EQUIPMENT"
pub(crate) const MODIFY_EQUIPMENT: u16 = 0x4e;
/// "CHANGE TROOP OCCUPATION"
pub(crate) const CHANGE_TROOP_OCCUPATION: u16 = 0x4f;
/// "MOVE TROOP"
pub(crate) const MOVE_TROOP: u16 = 0x50;
/// "NEXT TROOP"
pub(crate) const NEXT_TROOP: u16 = 0x51;
/// "NO MORE ORDERS"
pub(crate) const NO_MORE_ORDERS: u16 = 0x52;
/// "CUT CONTACT"
pub(crate) const CUT_CONTACT: u16 = 0x53;
/// "Show me where you want me to go..."
pub(crate) const SHOW_ME_WHERE_YOU_WANT_ME_TO_GO: u16 = 0x54;
/// "Show me 3 sietchs where you want me to go next..."
pub(crate) const SHOW_ME_3_SIETCHS_WHERE_YOU_WANT_ME_TO_G: u16 = 0x55;
/// "SELECT TROOP OCCUPATION"
pub(crate) const SELECT_TROOP_OCCUPATION: u16 = 0x56;
/// "SELECT DESTINATION ON MAP"
pub(crate) const SELECT_DESTINATION_ON_MAP: u16 = 0x57;
/// "CHANGE DESTINATION"
pub(crate) const CHANGE_DESTINATION: u16 = 0x58;
/// "GO THERE FLYING AN ORNI"
pub(crate) const GO_THERE_FLYING_AN_ORNI: u16 = 0x59;
/// "GO THERE RIDING A WORM"
pub(crate) const GO_THERE_RIDING_A_WORM: u16 = 0x5a;
/// "ADD A DESTINATION"
pub(crate) const ADD_A_DESTINATION: u16 = 0x5b;
/// "GIVE NEW DESTINATIONS"
pub(crate) const GIVE_NEW_DESTINATIONS: u16 = 0x5c;
/// "GO & SEARCH FOR EQUIPMENT"
pub(crate) const GO_SEARCH_FOR_EQUIPMENT: u16 = 0x5d;
/// " "
pub(crate) const STR_5E: u16 = 0x5e;
/// "ESPIONAGE"
pub(crate) const ESPIONAGE_5F: u16 = 0x5f;
/// "ATTACK"
pub(crate) const ATTACK: u16 = 0x60;
/// "ASSEMBLY WIND-TRAP"
pub(crate) const ASSEMBLY_WIND_TRAP: u16 = 0x61;
/// "CONTACT FREMEN TROOPS"
pub(crate) const CONTACT_FREMEN_TROOPS: u16 = 0x62;
/// "EXIT MAPS"
pub(crate) const EXIT_MAPS: u16 = 0x63;
/// "SEE SPICE DENSITY"
pub(crate) const SEE_SPICE_DENSITY: u16 = 0x64;
/// "  SPICE DENSITY  "
pub(crate) const SPICE_DENSITY: u16 = 0x65;
/// "no troops"
pub(crate) const NO_TROOPS: u16 = 0x66;
/// "FIND PROSPECTORS"
pub(crate) const FIND_PROSPECTORS: u16 = 0x67;
/// "  TROOP OCCUPATION  "
pub(crate) const TROOP_OCCUPATION: u16 = 0x68;
/// "none"
pub(crate) const NONE: u16 = 0x69;
/// "Spice: "
pub(crate) const SPICE_6A: u16 = 0x6a;
/// "unprospected"
pub(crate) const UNPROSPECTED: u16 = 0x6b;
/// "water: "
pub(crate) const WATER: u16 = 0x6c;
/// "No wind-trap"
pub(crate) const NO_WIND_TRAP: u16 = 0x6d;
/// "Equipment:"
pub(crate) const EQUIPMENT: u16 = 0x6e;
/// "  Sietch:\n(unused eqp.)"
pub(crate) const SIETCH_N_UNUSED_EQP: u16 = 0x6f;
/// "for a very short time"
pub(crate) const FOR_A_VERY_SHORT_TIME: u16 = 0x70;
/// "for a few hours"
pub(crate) const FOR_A_FEW_HOURS: u16 = 0x71;
/// "for 1 day"
pub(crate) const FOR_1_DAY: u16 = 0x72;
/// "for 12 days"
pub(crate) const FOR_12_DAYS: u16 = 0x73;
/// "but our job is finished"
pub(crate) const BUT_OUR_JOB_IS_FINISHED: u16 = 0x74;
/// "SPECIALIZE IN SPICE"
pub(crate) const SPECIALIZE_IN_SPICE: u16 = 0x75;
/// "SPECIALIZE IN ARMY"
pub(crate) const SPECIALIZE_IN_ARMY: u16 = 0x76;
/// "SPECIALIZE IN ECOLOGY"
pub(crate) const SPECIALIZE_IN_ECOLOGY: u16 = 0x77;
/// "DUKE LETO ATREIDES"
pub(crate) const DUKE_LETO_ATREIDES_78: u16 = 0x78;
/// "JESSICA"
pub(crate) const JESSICA: u16 = 0x79;
/// "Thufir HAWAT"
pub(crate) const THUFIR_HAWAT_7A: u16 = 0x7a;
/// "Duncan IDAHO"
pub(crate) const DUNCAN_IDAHO_7B: u16 = 0x7b;
/// "Gurney HALLECK"
pub(crate) const GURNEY_HALLECK_7C: u16 = 0x7c;
/// "STILGAR, Fremen leader"
pub(crate) const STILGAR_FREMEN_LEADER: u16 = 0x7d;
/// "KYNES, planetary ecologist"
pub(crate) const KYNES_PLANETARY_ECOLOGIST: u16 = 0x7e;
/// "CHANI"
pub(crate) const CHANI_7F: u16 = 0x7f;
/// "HARAH"
pub(crate) const HARAH_80: u16 = 0x80;
/// "BARON VLADIMIR HARKONNEN"
pub(crate) const BARON_VLADIMIR_HARKONNEN: u16 = 0x81;
/// "FEYD-RAUTHA HARKONNEN"
pub(crate) const FEYD_RAUTHA_HARKONNEN: u16 = 0x82;
/// "EMPEROR SHADDAM IV"
pub(crate) const EMPEROR_SHADDAM_IV: u16 = 0x83;
/// "Harkonnen Captain"
pub(crate) const HARKONNEN_CAPTAIN: u16 = 0x84;
/// "Smuggler"
pub(crate) const SMUGGLER: u16 = 0x85;
/// "Fremen"
pub(crate) const FREMEN: u16 = 0x86;
/// "Fremen Chief"
pub(crate) const FREMEN_CHIEF: u16 = 0x87;
/// "2nd Fremen Chief"
pub(crate) const _2ND_FREMEN_CHIEF: u16 = 0x88;
/// "3rd Fremen Chief"
pub(crate) const _3RD_FREMEN_CHIEF: u16 = 0x89;
/// "4th Fremen Chief"
pub(crate) const _4TH_FREMEN_CHIEF: u16 = 0x8a;
/// "5th Fremen Chief"
pub(crate) const _5TH_FREMEN_CHIEF: u16 = 0x8b;
/// "6th Fremen Chief"
pub(crate) const _6TH_FREMEN_CHIEF: u16 = 0x8c;
/// "7th Fremen Chief"
pub(crate) const _7TH_FREMEN_CHIEF: u16 = 0x8d;
/// "8th Fremen Chief"
pub(crate) const _8TH_FREMEN_CHIEF: u16 = 0x8e;
/// "Prospector Chief"
pub(crate) const PROSPECTOR_CHIEF: u16 = 0x8f;
/// "   >>>>  TALK TO ME  <<<<"
pub(crate) const TALK_TO_ME_90: u16 = 0x90;
/// "" COME WITH ME ""
pub(crate) const COME_WITH_ME: u16 = 0x91;
/// "" STAY HERE ""
pub(crate) const STAY_HERE: u16 = 0x92;
/// "GIVE ORDERS TO TROOP"
pub(crate) const GIVE_ORDERS_TO_TROOP: u16 = 0x93;
/// "STOP TALKING"
pub(crate) const STOP_TALKING: u16 = 0x94;
/// "" WHAT ? ""
pub(crate) const WHAT: u16 = 0x95;
/// "" WORK FOR ME ""
pub(crate) const WORK_FOR_ME: u16 = 0x96;
/// "" RALLY ME ""
pub(crate) const RALLY_ME: u16 = 0x97;
/// "SEE DUNE MAP"
pub(crate) const SEE_DUNE_MAP: u16 = 0x98;
/// "LOOK AT MIRROR"
pub(crate) const LOOK_AT_MIRROR: u16 = 0x99;
/// "MASSIVE ATTACK"
pub(crate) const MASSIVE_ATTACK: u16 = 0x9a;
/// "FIGHT FOR A WHOLE DAY"
pub(crate) const FIGHT_FOR_A_WHOLE_DAY: u16 = 0x9b;
/// "" OVERPOWER THE PRISONER ""
pub(crate) const OVERPOWER_THE_PRISONER: u16 = 0x9c;
/// "Look away from the mirror"
pub(crate) const LOOK_AWAY_FROM_THE_MIRROR: u16 = 0x9d;
/// "Mixer Panel"
pub(crate) const MIXER_PANEL: u16 = 0x9e;
/// "" TALK TO ME ""
pub(crate) const TALK_TO_ME_9F: u16 = 0x9f;
/// "  Others..."
pub(crate) const OTHERS: u16 = 0xa0;
/// "  Done"
pub(crate) const DONE: u16 = 0xa1;
/// " Continue..."
pub(crate) const CONTINUE: u16 = 0xa2;
/// "  Cancel"
pub(crate) const CANCEL: u16 = 0xa3;
/// "DESERT"
pub(crate) const DESERT: u16 = 0xa4;
/// "WAIT FOR EVENING"
pub(crate) const WAIT_FOR_EVENING: u16 = 0xa5;
/// "WAIT FOR MORNING"
pub(crate) const WAIT_FOR_MORNING: u16 = 0xa6;
/// "TAKE AN ORNITHOPTER"
pub(crate) const TAKE_AN_ORNITHOPTER: u16 = 0xa7;
/// "CALL A WORM"
pub(crate) const CALL_A_WORM: u16 = 0xa8;
/// "SKIP TO DESTINATION"
pub(crate) const SKIP_TO_DESTINATION: u16 = 0xa9;
/// "TOWARDS NEAREST PLACE"
pub(crate) const TOWARDS_NEAREST_PLACE: u16 = 0xaa;
/// "RESUME FLIGHT"
pub(crate) const RESUME_FLIGHT: u16 = 0xab;
/// "BACK TO STARTING POINT"
pub(crate) const BACK_TO_STARTING_POINT: u16 = 0xac;
/// "GO TOWARDS THIS PLACE"
pub(crate) const GO_TOWARDS_THIS_PLACE: u16 = 0xad;
/// "IGNORE WARNING"
pub(crate) const IGNORE_WARNING: u16 = 0xae;
/// "ATREIDES"
pub(crate) const ATREIDES_AF: u16 = 0xaf;
/// "HARKONNENS"
pub(crate) const HARKONNENS: u16 = 0xb0;
/// "SEE RESULTS"
pub(crate) const SEE_RESULTS: u16 = 0xb1;
/// "STANDARD VISION"
pub(crate) const STANDARD_VISION: u16 = 0xb2;
/// "SAVE GAME"
pub(crate) const SAVE_GAME: u16 = 0xb3;
/// "LOAD GAME"
pub(crate) const LOAD_GAME: u16 = 0xb4;
/// "."
pub(crate) const STR_B5: u16 = 0xb5;
/// "SEE MAP OF THIS AREA"
pub(crate) const SEE_MAP_OF_THIS_AREA: u16 = 0xb6;
/// "EXIT GLOBE"
pub(crate) const EXIT_GLOBE: u16 = 0xb7;
/// "YES I WANT TO EXIT GAME"
pub(crate) const YES_I_WANT_TO_EXIT_GAME: u16 = 0xb8;
/// "NO I WISH TO CONTINUE"
pub(crate) const NO_I_WISH_TO_CONTINUE: u16 = 0xb9;
/// "RESTART GAME"
pub(crate) const RESTART_GAME: u16 = 0xba;
/// "EXIT GAME"
pub(crate) const EXIT_GAME: u16 = 0xbb;
/// "Ah ah! One day, this Paul Atreides went flying over us. We shot down his ornithopter.\nHe died somewhere in the desert."
pub(crate) const AH_AH_ONE_DAY_THIS_PAUL_ATREIDES_WENT_FL: u16 = 0xbc;
/// "Paul Atreides died as he tried to drink the Water of Life. Many now say that he was too hasty. But I miss him. He was the only man able to send these Harkonnens back."
pub(crate) const PAUL_ATREIDES_DIED_AS_HE_TRIED_TO_DRINK: u16 = 0xbd;
/// "You know what?... Paul Atreides, he simply arrived one day at one of our fortresses. Needless to say, he was immediately shot! I've seen his body rotten under the sun... Ah ah!"
pub(crate) const YOU_KNOW_WHAT_PAUL_ATREIDES_HE_SIMPLY_AR: u16 = 0xbe;
/// "  ****  WARNING  ****\n\nENTERING HARKONNEN ZONE"
pub(crate) const WARNING_N_NENTERING_HARKONNEN_ZONE: u16 = 0xbf;
/// "Winning a battle, one of my captains captured Paul Atreides.\nAh! It was a real pleasure to leave him in plain desert without any protection."
pub(crate) const WINNING_A_BATTLE_ONE_OF_MY_CAPTAINS_CAPT: u16 = 0xc0;
/// "As Paul Atreides failed to respond to my spice demands, my Sardaukars terror troops took control on the planet.\nThat was the end of the Atreides."
pub(crate) const AS_PAUL_ATREIDES_FAILED_TO_RESPOND_TO_MY: u16 = 0xc1;
/// "  2nd day on DUNE"
pub(crate) const _2ND_DAY_ON_DUNE: u16 = 0xc2;
/// "CHARISMA = 129"
pub(crate) const CHARISMA_129: u16 = 0xc3;
/// "  0%"
pub(crate) const STR_C4: u16 = 0xc4;
/// "  0%"
pub(crate) const STR_C5: u16 = 0xc5;
/// "CONTROLLED AREAS"
pub(crate) const CONTROLLED_AREAS: u16 = 0xc6;
/// "     0 "
pub(crate) const STR_C7: u16 = 0xc7;
/// "     0 "
pub(crate) const STR_C8: u16 = 0xc8;
/// "SPICE PRODUCTION"
pub(crate) const SPICE_PRODUCTION: u16 = 0xc9;
/// "     0 "
pub(crate) const STR_CA: u16 = 0xca;
/// "     0 "
pub(crate) const STR_CB: u16 = 0xcb;
/// " NUMBER OF MEN"
pub(crate) const NUMBER_OF_MEN: u16 = 0xcc;
/// " Insufficient STANDARD MEMORY to run DUNE"
pub(crate) const INSUFFICIENT_STANDARD_MEMORY_TO_RUN_DUNE: u16 = 0xcd;
/// "on the left"
pub(crate) const ON_THE_LEFT: u16 = 0xce;
/// "ahead"
pub(crate) const AHEAD: u16 = 0xcf;
/// "on the right"
pub(crate) const ON_THE_RIGHT: u16 = 0xd0;
/// "On trial"
pub(crate) const ON_TRIAL: u16 = 0xd1;
/// "Novice"
pub(crate) const NOVICE: u16 = 0xd2;
/// "Average"
pub(crate) const AVERAGE: u16 = 0xd3;
/// "Efficient"
pub(crate) const EFFICIENT: u16 = 0xd4;
/// "Skilled"
pub(crate) const SKILLED: u16 = 0xd5;
/// "Expert"
pub(crate) const EXPERT: u16 = 0xd6;
/// "VIEW NEW MESSAGES"
pub(crate) const VIEW_NEW_MESSAGES: u16 = 0xd7;
/// "Messages already seen"
pub(crate) const MESSAGES_ALREADY_SEEN: u16 = 0xd8;
/// " Viewed"
pub(crate) const VIEWED: u16 = 0xd9;
/// "northwards"
pub(crate) const NORTHWARDS: u16 = 0xda;
/// "north-eastwards"
pub(crate) const NORTH_EASTWARDS: u16 = 0xdb;
/// "eastwards"
pub(crate) const EASTWARDS: u16 = 0xdc;
/// "south-eastwards"
pub(crate) const SOUTH_EASTWARDS: u16 = 0xdd;
/// "southwards"
pub(crate) const SOUTHWARDS: u16 = 0xde;
/// "south-westwards"
pub(crate) const SOUTH_WESTWARDS: u16 = 0xdf;
/// "westwards"
pub(crate) const WESTWARDS: u16 = 0xe0;
/// "north-westwards"
pub(crate) const NORTH_WESTWARDS: u16 = 0xe1;
/// "           DUNE  MAP\n* Map to command rallied troops *\n\n  Number of rallied troops =   0"
pub(crate) const DUNE_MAP_HEADER: u16 = 0xe2;
/// "ALL TOPICS"
pub(crate) const ALL_TOPICS: u16 = 0xe3;
/// " Close book"
pub(crate) const CLOSE_BOOK: u16 = 0xe4;
/// "TOPIC: PAUL ON DUNE"
pub(crate) const TOPIC_PAUL_ON_DUNE: u16 = 0xe5;
/// "TOPIC: SPICE"
pub(crate) const TOPIC_SPICE: u16 = 0xe6;
/// "TOPIC: THE FREMEN"
pub(crate) const TOPIC_THE_FREMEN: u16 = 0xe7;
/// "a spice-harvester"
pub(crate) const A_SPICE_HARVESTER: u16 = 0xe8;
/// "an orni"
pub(crate) const AN_ORNI: u16 = 0xe9;
/// "some krys"
pub(crate) const SOME_KRYS: u16 = 0xea;
/// "several laser-guns"
pub(crate) const SEVERAL_LASER_GUNS: u16 = 0xeb;
/// "weirding modules"
pub(crate) const WEIRDING_MODULES: u16 = 0xec;
/// "atomics weapons"
pub(crate) const ATOMICS_WEAPONS: u16 = 0xed;
/// "some bulbs"
pub(crate) const SOME_BULBS: u16 = 0xee;
/// "ACCEPT"
pub(crate) const ACCEPT: u16 = 0xef;
/// "REFUSE"
pub(crate) const REFUSE: u16 = 0xf0;
/// "ARGUE"
pub(crate) const ARGUE: u16 = 0xf1;
/// "On which page of the manual can you see this picture ?  "
pub(crate) const MANUAL_PAGE_QUESTION: u16 = 0xf2;
/// "And the Duke said to Paul:\n"
pub(crate) const AND_THE_DUKE_SAID_TO_PAUL_N: u16 = 0xf3;
/// "Paul's mother, Jessica, explains:\n"
pub(crate) const PAUL_S_MOTHER_JESSICA_EXPLAINS_N: u16 = 0xf4;
/// "Strategic master, Thufir Hawat, gave Paul this advice:\n"
pub(crate) const STRATEGIC_MASTER_THUFIR_HAWAT_GAVE_PAUL: u16 = 0xf5;
/// "Duncan Idaho told Paul:\n"
pub(crate) const DUNCAN_IDAHO_TOLD_PAUL_N: u16 = 0xf6;
/// "And Gurney Halleck said to Paul:\n"
pub(crate) const AND_GURNEY_HALLECK_SAID_TO_PAUL_N: u16 = 0xf7;
/// "Stilgar, the Fremen leader, told Paul:\n"
pub(crate) const STILGAR_THE_FREMEN_LEADER_TOLD_PAUL_N: u16 = 0xf8;
/// " "
pub(crate) const STR_F9: u16 = 0xf9;
/// "And Chani said to Paul:\n"
pub(crate) const AND_CHANI_SAID_TO_PAUL_N: u16 = 0xfa;
/// " "
pub(crate) const STR_FB: u16 = 0xfb;
/// " "
pub(crate) const STR_FC: u16 = 0xfc;
/// " "
pub(crate) const STR_FD: u16 = 0xfd;
/// " "
pub(crate) const STR_FE: u16 = 0xfe;
/// " "
pub(crate) const STR_FF: u16 = 0xff;
/// " "
pub(crate) const STR_100: u16 = 0x100;
/// "One Fremen told Paul:\n"
pub(crate) const ONE_FREMEN_TOLD_PAUL_N: u16 = 0x101;
/// "A Fremen talks:\n"
pub(crate) const A_FREMEN_TALKS_N: u16 = 0x102;
/// " "
pub(crate) const STR_103: u16 = 0x103;
/// "Paul Atreides"
pub(crate) const PAUL_ATREIDES_104: u16 = 0x104;
/// "Spice"
pub(crate) const SPICE_105: u16 = 0x105;
/// "The Fremen"
pub(crate) const THE_FREMEN: u16 = 0x106;
/// " "
pub(crate) const STR_107: u16 = 0x107;
/// "Paul Atreides"
pub(crate) const PAUL_ATREIDES_108: u16 = 0x108;
/// "Muad'Dib"
pub(crate) const MUAD_DIB: u16 = 0x109;
/// "MUSIC ON (CD-STYLE)"
pub(crate) const MUSIC_ON_CD_STYLE: u16 = 0x10a;
/// "MUSIC ON (GAME RELATIVE)"
pub(crate) const MUSIC_ON_GAME_RELATIVE: u16 = 0x10b;
/// "SHUFFLE"
pub(crate) const SHUFFLE: u16 = 0x10c;
/// "STANDARD ORDER"
pub(crate) const STANDARD_ORDER: u16 = 0x10d;
/// "MUSIC OFF"
pub(crate) const MUSIC_OFF: u16 = 0x10e;
/// "Log 1: DAY  0 / 12.00 a.m."
pub(crate) const LOG_1_DAY_0_12_00_A_M: u16 = 0x10f;
/// "Log 2: DAY  0 / 12.00 a.m."
pub(crate) const LOG_2_DAY_0_12_00_A_M: u16 = 0x110;
/// "LAST ENTERING INTO A PLACE"
pub(crate) const LAST_ENTERING_INTO_A_PLACE: u16 = 0x111;
/// "LAST ENTERING NEW SIETCH"
pub(crate) const LAST_ENTERING_NEW_SIETCH: u16 = 0x112;
/// " SAVE SUCCESSFUL"
pub(crate) const SAVE_SUCCESSFUL: u16 = 0x113;
/// " *** SAVE ERROR "
pub(crate) const SAVE_ERROR: u16 = 0x114;
/// "GAME  PAUSED"
pub(crate) const GAME_PAUSED: u16 = 0x115;
/// " <ESC> removes this window\nAny other key resumes game"
pub(crate) const ESC_REMOVES_THIS_WINDOW_NANY_OTHER_KEY_R: u16 = 0x116;
/// " 4.30 a.m. 6.00 a.m. 7.30 a.m. 9.00 a.m.10.30 a.m.12.00 p.m. 1.30 p.m. 3.00 p.m. 4.30 p.m. 6.00 p.m. 7.30 p.m. 9.00 p.m.10.30 p.m.12.00 a.m. 1.30 a.m. 3.00 a.m."
pub(crate) const TIME_OF_DAY_TABLE: u16 = 0x117;
/// "In these times of the future, man has explored many worlds, travelling  through space by the use of the SPICE."
pub(crate) const IN_THESE_TIMES_OF_THE_FUTURE_MAN_HAS_EXP: u16 = 0x118;
/// "SPICE is the most precious substance, it can be found only on one planet in the whole universe.\nThat planet is Arrakis, better known as DUNE."
pub(crate) const SPICE_IS_THE_MOST_PRECIOUS_SUBSTANCE_IT: u16 = 0x119;
/// "It's a dry desolate planet with vast deserts. There's never a drop of  rain on Dune."
pub(crate) const IT_S_A_DRY_DESOLATE_PLANET_WITH_VAST_DES: u16 = 0x11a;
/// "You are PAUL ATREIDES, son of Duke Leto Atreides."
pub(crate) const YOU_ARE_PAUL_ATREIDES_SON_OF_DUKE_LETO_A: u16 = 0x11b;
/// "The HARKONNENS, long time enemies of your family, have come on Dune  to control the Spice production, in their brutal way."
pub(crate) const THE_HARKONNENS_LONG_TIME_ENEMIES_OF_YOUR: u16 = 0x11c;
/// "But the Emperor of the Universe has just allowed you and the Atreides family  to go on Dune too."
pub(crate) const BUT_THE_EMPEROR_OF_THE_UNIVERSE_HAS_JUST: u16 = 0x11d;
/// "You are determined to use this opportunity to drive the  Harkonnens out of Dune, with the help of the few natives, the FREMEN."
pub(crate) const YOU_ARE_DETERMINED_TO_USE_THIS_OPPORTUNI: u16 = 0x11e;
/// "The story begins as you've just arrived on Dune, in an empty palace located  at a safe distance from the Harkonnen fortresses."
pub(crate) const THE_STORY_BEGINS_AS_YOU_VE_JUST_ARRIVED: u16 = 0x11f;
/// " "
pub(crate) const STR_120: u16 = 0x120;
/// " "
pub(crate) const STR_121: u16 = 0x121;
/// "Paul Atreides"
pub(crate) const PAUL_ATREIDES_122: u16 = 0x122;
/// "Duke Leto Atreides"
pub(crate) const DUKE_LETO_ATREIDES_123: u16 = 0x123;
/// "Jessica Atreides"
pub(crate) const JESSICA_ATREIDES: u16 = 0x124;
/// "Gurney Halleck"
pub(crate) const GURNEY_HALLECK_125: u16 = 0x125;
/// "Duncan Idaho"
pub(crate) const DUNCAN_IDAHO_126: u16 = 0x126;
/// "Shaddam IV"
pub(crate) const SHADDAM_IV: u16 = 0x127;
/// "Harah"
pub(crate) const HARAH_128: u16 = 0x128;
/// "Thufir Hawat"
pub(crate) const THUFIR_HAWAT_129: u16 = 0x129;
/// "Stilgar"
pub(crate) const STILGAR: u16 = 0x12a;
/// "Baron Harkonnen"
pub(crate) const BARON_HARKONNEN: u16 = 0x12b;
/// "Feyd Rautha"
pub(crate) const FEYD_RAUTHA: u16 = 0x12c;
/// "Chani"
pub(crate) const CHANI_12D: u16 = 0x12d;
/// "Liet Kynes"
pub(crate) const LIET_KYNES: u16 = 0x12e;
/// " "
pub(crate) const STR_12F: u16 = 0x12f;
/// " "
pub(crate) const STR_130: u16 = 0x130;
/// "."
pub(crate) const STR_131: u16 = 0x131;
/// "SUPER FREMEN HERE"
pub(crate) const SUPER_FREMEN_HERE: u16 = 0x132;
/// "PHASE LOC KNOWN"
pub(crate) const PHASE_LOC_KNOWN: u16 = 0x133;
/// "ALL SIETCHS KNOWN"
pub(crate) const ALL_SIETCHS_KNOWN: u16 = 0x134;
/// "RALLY ALL FREMEN/SIETCH"
pub(crate) const RALLY_ALL_FREMEN_SIETCH: u16 = 0x135;
/// " ALL LOC PROSPECTED"
pub(crate) const ALL_LOC_PROSPECTED: u16 = 0x136;
/// " MUAD'DIB + 10"
pub(crate) const MUAD_DIB_10: u16 = 0x137;
/// "  SHOW COORDS/SMALL MAP"
pub(crate) const SHOW_COORDS_SMALL_MAP: u16 = 0x138;
/// " TIME VERY FAST"
pub(crate) const TIME_VERY_FAST: u16 = 0x139;
/// " TIME NORMAL"
pub(crate) const TIME_NORMAL: u16 = 0x13a;
/// " VEGET EVERYWHERE"
pub(crate) const VEGET_EVERYWHERE: u16 = 0x13b;
/// "  SHOW TRAVEL ANGLES"
pub(crate) const SHOW_TRAVEL_ANGLES: u16 = 0x13c;
/// "  SHOW VARIABLE"
pub(crate) const SHOW_VARIABLE: u16 = 0x13d;
/// "   BACK TO SCR"
pub(crate) const BACK_TO_SCR: u16 = 0x13e;
/// "     ALL TEXTS"
pub(crate) const ALL_TEXTS: u16 = 0x13f;
/// "  SHOW TIME AND SPEED"
pub(crate) const SHOW_TIME_AND_SPEED: u16 = 0x140;
/// "   BUF TO SCR"
pub(crate) const BUF_TO_SCR: u16 = 0x141;
/// "   ALL LOC KNOWN"
pub(crate) const ALL_LOC_KNOWN: u16 = 0x142;
/// "   NO "TOO FAR...""
pub(crate) const NO_TOO_FAR: u16 = 0x143;
/// "    GOTO PHASE 80"
pub(crate) const GOTO_PHASE_80: u16 = 0x144;
/// "    INCPHASE"
pub(crate) const INCPHASE: u16 = 0x145;
/// "    PHASE 123"
pub(crate) const PHASE_123: u16 = 0x146;
/// "    GO->GAME END"
pub(crate) const GO_GAME_END: u16 = 0x147;
/// "     HARKO ATTACK"
pub(crate) const HARKO_ATTACK: u16 = 0x148;
/// "     NOT KILLED"
pub(crate) const NOT_KILLED: u16 = 0x149;
/// "     9 PERSOS HERE"
pub(crate) const _9_PERSOS_HERE: u16 = 0x14a;
/// "     ALL PERSOS"
pub(crate) const ALL_PERSOS: u16 = 0x14b;
/// "     CD: 150k/sec"
pub(crate) const CD_150K_SEC: u16 = 0x14c;
/// "     MOVIE"
pub(crate) const MOVIE: u16 = 0x14d;
