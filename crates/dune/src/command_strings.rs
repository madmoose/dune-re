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
/// "Select the destination of the troop."
pub(crate) const SELECT_DESTINATION_OF_TROOP: u16 = 0x54;
/// "Select the destinations of the prospectors."
pub(crate) const SELECT_DESTINATIONS_OF_PROSPECTORS: u16 = 0x55;
/// "SELECT TROOP OCCUPATION"
pub(crate) const SELECT_TROOP_OCCUPATION: u16 = 0x56;
/// "CHANGE DESTINATION"
pub(crate) const CHANGE_DESTINATION: u16 = 0x58;
/// "ADD A DESTINATION"
pub(crate) const ADD_A_DESTINATION: u16 = 0x5b;
/// "GIVE NEW DESTINATIONS"
pub(crate) const GIVE_NEW_DESTINATIONS: u16 = 0x5c;
/// "GO & SEARCH FOR EQUIPMENT"
pub(crate) const GO_AND_SEARCH_FOR_EQUIPMENT: u16 = 0x5d;
/// "ESPIONAGE"
pub(crate) const ESPIONAGE: u16 = 0x5f;
/// "ATTACK"
pub(crate) const ATTACK: u16 = 0x60;
/// "ASSEMBLY WIND-TRAP"
pub(crate) const ASSEMBLY_WIND_TRAP: u16 = 0x61;
/// "GO THERE FLYING AN ORNI"
pub(crate) const GO_THERE_FLYING_AN_ORNI: u16 = 0x59;
/// "GO THERE RIDING A WORM"
pub(crate) const GO_THERE_RIDING_A_WORM: u16 = 0x5a;
/// "CONTACT FREMEN TROOPS"
pub(crate) const CONTACT_FREMEN_TROOPS: u16 = 0x62;
/// "EXIT MAPS"
pub(crate) const EXIT_MAPS: u16 = 0x63;
/// "SEE SPICE DENSITY"
pub(crate) const SEE_SPICE_DENSITY: u16 = 0x64;
/// "FIND PROSPECTORS"
pub(crate) const FIND_PROSPECTORS: u16 = 0x67;
/// "none"
pub(crate) const NONE: u16 = 0x69;
/// "SPECIALIZE IN SPICE"
pub(crate) const SPECIALIZE_IN_SPICE: u16 = 0x75;
/// "SPECIALIZE IN ARMY"
pub(crate) const SPECIALIZE_IN_ARMY: u16 = 0x76;
/// "SPECIALIZE IN ECOLOGY"
pub(crate) const SPECIALIZE_IN_ECOLOGY: u16 = 0x77;
/// "Equipment:"
pub(crate) const EQUIPMENT: u16 = 0x6e;
/// "GIVE ORDERS TO TROOP"
pub(crate) const GIVE_ORDERS_TO_TROOP: u16 = 0x93;
/// "SEE DUNE MAP"
pub(crate) const WHAT: u16 = 0x95;
/// " WHAT ? "
pub(crate) const SEE_DUNE_MAP: u16 = 0x98;
/// "Done"
pub(crate) const DONE: u16 = 0xa1;
/// " Continue..."
pub(crate) const CONTINUE: u16 = 0xa2;
/// "  Cancel"
pub(crate) const CANCEL: u16 = 0xa3;
/// "TAKE AN ORNITHOPTER"
pub(crate) const TAKE_AN_ORNITHOPTER: u16 = 0xa7;
/// "           DUNE  MAP\n* Map to command rallied troops *\n\n  Number of
/// rallied troops =   0" — the count digits are overwritten in place with the
/// live number_of_rallied_troops.
pub(crate) const DUNE_MAP_RALLIED_TROOPS: u16 = 0xe2;
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
