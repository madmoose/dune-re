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
/// "Equipment:"
pub(crate) const EQUIPMENT: u16 = 0x6e;
/// "GIVE ORDERS TO TROOP"
pub(crate) const GIVE_ORDERS_TO_TROOP: u16 = 0x93;
/// "SEE DUNE MAP"
pub(crate) const SEE_DUNE_MAP: u16 = 0x98;
/// "  Cancel"
pub(crate) const CANCEL: u16 = 0xa3;
/// "TAKE AN ORNITHOPTER"
pub(crate) const TAKE_AN_ORNITHOPTER: u16 = 0xa7;
/// "           DUNE  MAP\n* Map to command rallied troops *\n\n  Number of
/// rallied troops =   0" — the count digits are overwritten in place with the
/// live number_of_rallied_troops.
pub(crate) const DUNE_MAP_RALLIED_TROOPS: u16 = 0xe2;
