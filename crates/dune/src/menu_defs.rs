use crate::{cmd, game_state::GameState};

/// The cleanup func a menu-stack slot stores alongside its menu
/// (= the DOS `bx` at screen_element_stack_push, kept in the slot's `[si+2]`
/// and run when the element leaves the stack).
pub(crate) type MenuCleanupFn = fn(&mut GameState);

/// The click callback a MenuItem stores (= the DOS `jmp bx` at
/// dispatch_command_menu_slot, seg000:d451): `text_id` is the clicked
/// record's text id (DOS ax), `index` its slot in the menu (DOS cx, loaded
/// by the per-row trampolines loc_0d443..d42f).
pub(crate) type MenuItemCallback = fn(state: &mut GameState, text_id: u16, index: usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MenuRef {
    CommandMenuBuf,
    MenuNpcActions,
    MenuGoTowardsThisPlace,
    MenuDestinationWarning,
    MenuProspectorContinue,
    MenuContinue,
    MenuDynamic,
    MenuCommsRoomMessagesViewed,
    MenuArgueAcceptRefuse,
    MenuDone,
    MenuMixerPanel,
    MenuBook,
    MenuGlobe,
    MenuGlobeDefaultClickOnGlobe,
    MenuMusic,
    MenuSaveGame,
    MenuLoadGame,
    MenuRestartLoadExitGame,
    MenuExitGameConfirmation,
    MenuPalaceMirrorRoom,
    MenuGoThereFlyingAnOrni,
    MenuGoThereRidingAWorm,
    MenuMapTroops,
    MenuTroopDialog,
    MenuNextTroop,
    MenuCancel,
    MenuMoveProspectors,
    MenuChangeTroopDestination,
    MenuSelectTroopOccupation,
    MenuOccupationForSpiceTroop,
    MenuOccupationForArmyTroop,
    MenuOccupationForEspionageTroop,
    MenuOccupationForEcologyTroop,
}

pub(crate) struct Menu {
    pub priority: u8,
    pub records: Vec<MenuItem>,
}

impl From<MenuDef> for Menu {
    fn from(def: MenuDef) -> Self {
        Self {
            priority: def.priority,
            records: def.records.to_vec(),
        }
    }
}

pub(crate) struct MenuDef {
    pub priority: u8,
    pub records: &'static [MenuItem],
}

#[derive(Copy, Clone)]
pub(crate) struct MenuItem {
    pub text_id: u16,
    pub handler: u16,
    pub callback: MenuItemCallback,
}

const fn menu(priority: u8, records: &'static [MenuItem]) -> MenuDef {
    MenuDef { priority, records }
}

pub(crate) const fn item(text_id: u16, handler: u16, callback: MenuItemCallback) -> MenuItem {
    MenuItem {
        text_id,
        handler,
        callback,
    }
}

impl MenuItem {
    pub(crate) fn set_grayed_if(&mut self, gray: bool) {
        if gray {
            self.text_id |= CMD_GREY;
        } else {
            self.text_id &= !CMD_GREY;
        }
    }

    /// The builder-style companion of set_grayed_if: the same record with the
    /// CMD_GREY bit set or cleared.
    pub(crate) const fn grayed_if(mut self, gray: bool) -> Self {
        if gray {
            self.text_id |= CMD_GREY;
        } else {
            self.text_id &= !CMD_GREY;
        }
        self
    }
}

const fn gray(text_id: u16, handler: u16, callback: MenuItemCallback) -> MenuItem {
    item(text_id | CMD_GREY, handler, callback)
}

const fn highlight(text_id: u16, handler: u16, callback: MenuItemCallback) -> MenuItem {
    item(text_id | CMD_HIGHLIGHT, handler, callback)
}

pub(crate) const CMD_GREY: u16 = 0x4000;
pub(crate) const CMD_HIGHLIGHT: u16 = 0x8000;

/// = seg001:1f0e command_menu_buf.
pub(crate) const COMMAND_MENU_BUF: MenuDef = menu(0xff, &[]);

/// = seg001:1f7e menu_npc_actions
#[rustfmt::skip]
pub(crate) const MENU_NPC_ACTIONS: MenuDef = menu(0xfc, &[
    item(cmd::TALK_TO_ME_9F, 0x9472, GameState::menu_callback_choice_talk_to_me),
    item(cmd::COME_WITH_ME,  0x95e2, GameState::menu_callback_choice_come_with_me),
    item(cmd::WHAT,          0x9ed5, GameState::menu_callback_choice_what),
    item(cmd::STOP_TALKING,  0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:1f92 menu_go_towards_this_place
#[rustfmt::skip]
pub(crate) const MENU_GO_TOWARDS_THIS_PLACE: MenuDef = menu(0xfc, &[
    item(cmd::GO_TOWARDS_THIS_PLACE, 0xd2e2, GameState::menu_callback_choice_exit_menu),
    item(cmd::WHAT,                  0x9ed5, GameState::menu_callback_choice_what),
]);

/// = seg001:1f9e menu_change_destination_ignore_warning
#[rustfmt::skip]
pub(crate) const MENU_DESTINATION_WARNING: MenuDef = menu(0xf8, &[
    item(cmd::CHANGE_DESTINATION, 0x497a, GameState::menu_callback_choice_change_destination),
    item(cmd::IGNORE_WARNING,     0xd2e2, GameState::menu_callback_choice_exit_menu),
    item(cmd::WHAT,               0x9ed5, GameState::menu_callback_choice_what),
]);

/// = seg001:1fae menu_prospector_troop_after_specializing_in_spice
#[rustfmt::skip]
pub(crate) const MENU_PROSPECTOR_CONTINUE: MenuDef = menu(0xfc, &[
    item(cmd::CONTINUE, 0x1707, GameState::menu_callback_choice_continue_for_sequence),
    item(cmd::WHAT,     0x9ed5, GameState::menu_callback_choice_what),
]);

/// = seg001:1fba menu_multiple_provide_continue_option
#[rustfmt::skip]
pub(crate) const MENU_CONTINUE: MenuDef = menu(0xfc, &[
    item(cmd::CONTINUE, 0x1707, GameState::menu_callback_choice_continue_for_sequence),
]);

/// = seg001:1fc2 menu_dynamic — a zero-filled three-record template the game
/// fills in at runtime (the EXE bytes are all zero; the listing renders the
/// callback words as `start` because seg000:0000 is the entry point).
#[rustfmt::skip]
pub(crate) const MENU_DYNAMIC: MenuDef = menu(0xfc, &[
    item(0, 0, |_, _, _| {}),
    item(0, 0, |_, _, _| {}),
    item(0, 0, |_, _, _| {}),
]);

/// = seg001:1ff2 menu_comms_room_messages_viewed
#[rustfmt::skip]
pub(crate) const MENU_COMMS_ROOM_MESSAGES_VIEWED: MenuDef = menu(0xfb, &[
    item(cmd::VIEWED, 0x2993, |_, _, _| println!("menu: Viewed (seg000:2993) not ported")),
    item(cmd::WHAT,   0x9ed5, GameState::menu_callback_choice_what),
]);

/// = seg001:1ffe menu_argue_accept_refuse
#[rustfmt::skip]
pub(crate) const MENU_ARGUE_ACCEPT_REFUSE: MenuDef = menu(0xfb, &[
    item(cmd::ARGUE,  0x2453, |_, _, _| println!("menu: ARGUE (seg000:2453) not ported")),
    item(cmd::ACCEPT, 0x241a, |_, _, _| println!("menu: ACCEPT (seg000:241a) not ported")),
    item(cmd::REFUSE, 0x2432, |_, _, _| println!("menu: REFUSE (seg000:2432) not ported")),
    item(cmd::WHAT,   0x9ed5, GameState::menu_callback_choice_what),
]);

/// = seg001:2012 menu_done
#[rustfmt::skip]
pub(crate) const MENU_DONE: MenuDef = menu(0xf8, &[
    item(cmd::DONE, 0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:201a menu_mixer_panel
#[rustfmt::skip]
pub(crate) const MENU_MIXER_PANEL: MenuDef = menu(0xf8, &[
    item(cmd::MUSIC_OFF,              0xaeaf, GameState::menu_callback_choice_music_off),
    item(cmd::MUSIC_ON_GAME_RELATIVE, 0xac6e, GameState::menu_callback_choice_music_on_game_relative),
    item(cmd::MUSIC_ON_CD_STYLE,      0xac7e, GameState::menu_callback_choice_music_on_cd_style),
    item(cmd::EXIT_GAME,              0x0e3e, GameState::menu_callback_choice_exit_game),
    item(cmd::DONE,                   0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:2032 menu_book — the topic verbs share one ported entry point
/// (menu_callback_choice_book_topic) that DOS splits over four trampolines;
/// the slot index selects the topic-bit pair.
#[rustfmt::skip]
pub(crate) const MENU_BOOK: MenuDef = menu(0xff, &[
    item(cmd::ALL_TOPICS,         0xaf58, GameState::menu_callback_choice_book_topic),
    item(cmd::TOPIC_PAUL_ON_DUNE, 0xaf60, GameState::menu_callback_choice_book_topic),
    item(cmd::TOPIC_SPICE,        0xaf68, GameState::menu_callback_choice_book_topic),
    item(cmd::TOPIC_THE_FREMEN,   0xaf70, GameState::menu_callback_choice_book_topic),
    item(cmd::CLOSE_BOOK,         0xb18b, |state, _, _| state.callback_ui_element_book_close()),
]);

/// = seg001:204a menu_globe — the LOAD/SAVE rows carry text 0xb4/0xb3 and the
/// matching load/save callbacks; the .chani inline comments on data_02054/
/// data_02058 have the two labels swapped.
#[rustfmt::skip]
pub(crate) const MENU_GLOBE: MenuDef = menu(0xff, &[
    item(cmd::EXIT_GLOBE,  0xbc81, |state, _, _| state.callback_ui_element_globe_exit()),
    item(cmd::SEE_RESULTS, 0xb96b, |_, _, _| println!("menu: SEE RESULTS (seg000:b96b) not ported")),
    item(cmd::LOAD_GAME,   0xb29e, GameState::menu_callback_choice_mirror_room_load_game),
    item(cmd::SAVE_GAME,   0xb28c, GameState::menu_callback_choice_mirror_room_save_game),
    item(cmd::EXIT_GAME,   0x0e3e, GameState::menu_callback_choice_exit_game),
]);

/// = seg001:2062 menu_globe_default_click_on_globe — the single highlighted
/// SEE MAP OF THIS AREA row; its callback is nullsub_00f66 (a plain ret: the
/// globe click itself does the work).
#[rustfmt::skip]
pub(crate) const MENU_GLOBE_DEFAULT_CLICK_ON_GLOBE: MenuDef = menu(0xff, &[
    highlight(cmd::SEE_MAP_OF_THIS_AREA, 0x0f66, |_, _, _| {}),
]);

/// = seg001:206a menu_globe_music
#[rustfmt::skip]
pub(crate) const MENU_MUSIC: MenuDef = menu(0xf6, &[
    item(cmd::STANDARD_ORDER, 0xac97, GameState::menu_callback_choice_music_cd_order_standard),
    item(cmd::SHUFFLE,        0xac90, GameState::menu_callback_choice_music_cd_order_shuffle),
    item(cmd::CANCEL,         0xd2df, GameState::menu_callback_choice_music_cd_order_cancel),
]);

/// = seg001:207a menu_globe_save_game — the slot rows share one callback: the
/// clicked index is DOS's cx (the save slot), the text id DOS's ax (the label
/// loc_0b2cd stamps the day/time into).
#[rustfmt::skip]
pub(crate) const MENU_SAVE_GAME: MenuDef = menu(0xfe, &[
    item(cmd::LOG_1_DAY_0_12_00_A_M, 0xb35a, GameState::menu_callback_choice_globe_save_game),
    item(cmd::LOG_2_DAY_0_12_00_A_M, 0xb35a, GameState::menu_callback_choice_globe_save_game),
    item(cmd::CANCEL,                0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:208a menu_globe_load_game — the two manual slots plus the two
/// autosave slots, sharing one callback: the clicked index is DOS's cx (the
/// load slot).
#[rustfmt::skip]
pub(crate) const MENU_LOAD_GAME: MenuDef = menu(0xfe, &[
    item(cmd::LOG_1_DAY_0_12_00_A_M,      0xb3b0, GameState::menu_callback_choice_globe_load_game),
    item(cmd::LOG_2_DAY_0_12_00_A_M,      0xb3b0, GameState::menu_callback_choice_globe_load_game),
    item(cmd::LAST_ENTERING_INTO_A_PLACE, 0xb3b0, GameState::menu_callback_choice_globe_load_game),
    item(cmd::LAST_ENTERING_NEW_SIETCH,   0xb3b0, GameState::menu_callback_choice_globe_load_game),
    item(cmd::CANCEL,                     0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:20a2 menu_restart_load_exit_game
#[rustfmt::skip]
pub(crate) const MENU_RESTART_LOAD_EXIT_GAME: MenuDef = menu(0xff, &[
    item(cmd::RESTART_GAME, 0x0e47, |_, _, _| println!("menu: RESTART GAME (seg000:0e47) not ported")),
    item(cmd::LOAD_GAME,    0xb29e, GameState::menu_callback_choice_mirror_room_load_game),
    item(cmd::EXIT_GAME,    0x0e3e, GameState::menu_callback_choice_exit_game),
    item(cmd::WHAT,         0x9ed5, GameState::menu_callback_choice_what),
]);

/// = seg001:20b6 menu_exit_game_confirmation — YES jumps to exit_to_dos; the
/// closure bridges the port's diverging `fn(&mut self) -> !` signature.
#[rustfmt::skip]
pub(crate) const MENU_EXIT_GAME_CONFIRMATION: MenuDef = menu(0xf6, &[
    item(cmd::YES_I_WANT_TO_EXIT_GAME, 0x003a, |state, _, _| state.exit_to_dos()),
    item(cmd::NO_I_WISH_TO_CONTINUE,   0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:20c2 menu_palace_mirror_room — the "Look away from the mirror"
/// row's callback pops the mirror overlay before restoring the room.
#[rustfmt::skip]
pub(crate) const MENU_PALACE_MIRROR_ROOM: MenuDef = menu(0xff, &[
    item(cmd::RESTART_GAME,              0x0e47, |_, _, _| println!("menu: RESTART GAME (seg000:0e47) not ported")),
    item(cmd::LOAD_GAME,                 0xb29e, GameState::menu_callback_choice_mirror_room_load_game),
    item(cmd::SAVE_GAME,                 0xb28c, GameState::menu_callback_choice_mirror_room_save_game),
    item(cmd::EXIT_GAME,                 0x0e3e, GameState::menu_callback_choice_exit_game),
    item(cmd::LOOK_AWAY_FROM_THE_MIRROR, 0x0eb9, GameState::menu_callback_choice_palace_look_away_from_mirror),
]);

/// = seg001:20da menu_multiple_move_to_location_flying_an_orni
#[rustfmt::skip]
pub(crate) const MENU_GO_THERE_FLYING_AN_ORNI: MenuDef = menu(0xfc, &[
    item(cmd::GO_THERE_FLYING_AN_ORNI, 0x50db, GameState::menu_callback_choice_move_to_location_orni),
    item(cmd::CANCEL,                  0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:20e6 menu_multiple_move_to_location_riding_a_worm
#[rustfmt::skip]
pub(crate) const MENU_GO_THERE_RIDING_A_WORM: MenuDef = menu(0xfc, &[
    item(cmd::GO_THERE_RIDING_A_WORM, 0x50ea, GameState::menu_callback_choice_move_to_location_worm),
    item(cmd::CANCEL,                 0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:20f2 menu_map_main
#[rustfmt::skip]
pub(crate) const MENU_MAP_TROOPS: MenuDef = menu(0xff, &[
    item(cmd::EXIT_MAPS,             0x186b, |state, _, _| state.ui_toggle_room_view()),
    item(cmd::CONTACT_FREMEN_TROOPS, 0x86cc, GameState::menu_callback_choice_map_main_contact_fremen_troops),
    item(cmd::SEE_SPICE_DENSITY,     0x53f1, GameState::menu_callback_choice_map_main_see_spice_density),
    item(cmd::TAKE_AN_ORNITHOPTER,   0x42d9, GameState::menu_callback_choice_map_main_take_an_ornithopter),
    item(cmd::FIND_PROSPECTORS,      0x5b1e, |_, _, _| println!("menu: FIND PROSPECTORS (seg000:5b1e) not ported")),
]);

/// = seg001:210a menu_map_troop_dialog
#[rustfmt::skip]
pub(crate) const MENU_TROOP_DIALOG: MenuDef = menu(0xfc, &[
    item(cmd::ASK_FOR_MORE_INFORMATION, 0x7bed, GameState::menu_callback_choice_map_troop_dialogue_ask_for_more_information),
    item(cmd::CHANGE_TROOP_OCCUPATION,  0x69b3, GameState::menu_callback_choice_map_troop_dialogue_change_troop_occupation),
    item(cmd::MODIFY_EQUIPMENT,         0x7cbb, |_, _, _| println!("menu: MODIFY EQUIPMENT (seg000:7cbb) not ported")),
    item(cmd::MOVE_TROOP,               0x8064, GameState::menu_callback_choice_multiple_move_troop),
    item(cmd::NO_MORE_ORDERS,           0x8763, GameState::menu_callback_choice_multiple_no_more_orders),
]);

/// = seg001:2122 menu_map_troop_contact_cycle_troops
#[rustfmt::skip]
pub(crate) const MENU_NEXT_TROOP: MenuDef = menu(0xfc, &[
    item(cmd::NEXT_TROOP,     0x86fa, GameState::menu_callback_choice_map_troop_contact_next_troop),
    item(cmd::NO_MORE_ORDERS, 0x8770, GameState::menu_callback_choice_map_troop_contact_no_more_orders),
]);

/// = seg001:212e menu_multiple_cancel
#[rustfmt::skip]
pub(crate) const MENU_CANCEL: MenuDef = menu(0xf8, &[
    item(cmd::CANCEL, 0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:2136 menu_map_move_prospectors — ADD A DESTINATION's DOS handler
/// (seg000:80c7) is a plain ret: the map click does the adding, the slot is a
/// label.
#[rustfmt::skip]
pub(crate) const MENU_MOVE_PROSPECTORS: MenuDef = menu(0xf8, &[
    item(cmd::ADD_A_DESTINATION,     0x80c7, |_, _, _| {}),
    item(cmd::GIVE_NEW_DESTINATIONS, 0x80d9, GameState::menu_callback_choice_map_move_prospectors_give_new_destinations),
    item(cmd::DONE,                  0x8214, GameState::menu_callback_choice_map_move_prospectors_done),
    item(cmd::CANCEL,                0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:214a menu_map_troop_moving_change_destination_next_troop
#[rustfmt::skip]
pub(crate) const MENU_CHANGE_TROOP_DESTINATION: MenuDef = menu(0xfc, &[
    item(cmd::CHANGE_DESTINATION, 0x8064, GameState::menu_callback_choice_multiple_move_troop),
    item(cmd::NEXT_TROOP,         0x86fa, GameState::menu_callback_choice_map_troop_contact_next_troop),
    item(cmd::CANCEL,             0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:215a menu_map_select_troop_occupation — the EXE bytes carry the
/// grey flag on SPECIALIZE IN ECOLOGY (id 0x4077).
#[rustfmt::skip]
pub(crate) const MENU_SELECT_TROOP_OCCUPATION: MenuDef = menu(0xf8, &[
    item(cmd::SPECIALIZE_IN_SPICE,   0x6a71, GameState::menu_callback_choice_troop_occupation_specialize_in_spice),
    item(cmd::SPECIALIZE_IN_ARMY,    0x6a83, GameState::menu_callback_choice_troop_occupation_specialize_in_army),
    gray(cmd::SPECIALIZE_IN_ECOLOGY, 0x6a87, GameState::menu_callback_choice_troop_occupation_specialize_in_ecology),
    item(cmd::CANCEL,                0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:216e menu_map_troop_change_troop_occupation_for_spice_troop
#[rustfmt::skip]
pub(crate) const MENU_OCCUPATION_FOR_SPICE_TROOP: MenuDef = menu(0xf8, &[
    item(cmd::GO_SEARCH_FOR_EQUIPMENT, 0x776d, |_, _, _| println!("menu: GO & SEARCH FOR EQUIPMENT (seg000:776d) not ported")),
    item(cmd::SPECIALIZE_IN_ARMY,      0x6a83, GameState::menu_callback_choice_troop_occupation_specialize_in_army),
    item(cmd::SPECIALIZE_IN_ECOLOGY,   0x6a87, GameState::menu_callback_choice_troop_occupation_specialize_in_ecology),
    item(cmd::CANCEL,                  0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:2182 menu_map_troop_change_troop_occupation_for_army_troop.
#[rustfmt::skip]
pub(crate) const MENU_OCCUPATION_FOR_ARMY_TROOP: MenuDef = menu(0xf8, &[
    item(cmd::GO_SEARCH_FOR_EQUIPMENT, 0x7734, |_, _, _| println!("menu: GO & SEARCH FOR EQUIPMENT (seg000:7734) not ported")),
    gray(cmd::ESPIONAGE_5F,            0x6a45, |_, _, _| println!("menu: ESPIONAGE (seg000:6a45) not ported")),
    item(cmd::SPECIALIZE_IN_SPICE,     0x6a71, GameState::menu_callback_choice_troop_occupation_specialize_in_spice),
    item(cmd::SPECIALIZE_IN_ECOLOGY,   0x6a87, GameState::menu_callback_choice_troop_occupation_specialize_in_ecology),
    item(cmd::CANCEL,                  0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:219a menu_map_troop_change_troop_occupation_for_army_troop_doing_espionage_at_harkonnen_fortress
#[rustfmt::skip]
pub(crate) const MENU_OCCUPATION_FOR_ESPIONAGE_TROOP: MenuDef = menu(0xf8, &[
    item(cmd::ATTACK, 0x6a2f, |_, _, _| println!("menu: ATTACK (seg000:6a2f) not ported")),
    item(cmd::CANCEL, 0xd2e2, GameState::menu_callback_choice_exit_menu),
]);

/// = seg001:21a6 menu_map_troop_change_troop_occupation_for_ecology_troop
#[rustfmt::skip]
pub(crate) const MENU_OCCUPATION_FOR_ECOLOGY_TROOP: MenuDef = menu(0xf8, &[
    item(cmd::GO_SEARCH_FOR_EQUIPMENT, 0x775c, |_, _, _| println!("menu: GO & SEARCH FOR EQUIPMENT (seg000:775c) not ported")),
    item(cmd::ASSEMBLY_WIND_TRAP,      0x6a2b, GameState::menu_callback_choice_troop_occupation_assembly_wind_trap),
    item(cmd::SPECIALIZE_IN_SPICE,     0x6a71, GameState::menu_callback_choice_troop_occupation_specialize_in_spice),
    item(cmd::SPECIALIZE_IN_ARMY,      0x6a83, GameState::menu_callback_choice_troop_occupation_specialize_in_army),
    item(cmd::CANCEL,                  0xd2e2, GameState::menu_callback_choice_exit_menu),
]);
