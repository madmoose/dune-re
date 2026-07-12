use crate::{Font, GameState, container};

impl GameState {
    // = seg000:cf70 get_phrase_or_command_string
    // entries with bit 0x800 set are PHRASE.BIN dialogue strings; the rest index
    // COMMAND.BIN.
    pub fn get_phrase_or_command_string(&self, index: u16) -> &[u8] {
        // = seg000:cf71 dec si; test si,800h.
        let index = index - 1;
        if index & 0x800 != 0 {
            // = seg000:cf78 PHRASE.BIN path — TODO (dialogue text system).
            return &[];
        }

        container::entry(&self.command_bin, index)
    }

    // = seg000:cfa0 check_amr_or_eng_language.
    pub(crate) fn check_amr_or_eng_language(&mut self) {
        // TODO: = seg000:cfa0.
    }

    // = seg000:cfe4 settings_ui_reload_language — reload the language-dependent
    // resources for the freshly selected language_setting so all on-screen text
    // (the command/verb strip, menus, indicators) switches language. DOS reopens
    // three resources by index; the port reads each by name from the DAT:
    //   - the DNCHAR glyph font: index 0xbb (DNCHAR.BIN), or 0xc7 (DNCHAR2.BIN) for
    //     the Fremen / DUT language (language_setting == 6), which carries that
    //     language's accented glyphs.
    //   - COMMAND.BIN: index 0xc0 + language = COMMAND{language+1}.HSQ — the verb /
    //     command string table get_phrase_or_command_string reads.
    //
    // DOS also calls adjust_sub_resource_pointers after each load to repoint the
    // resource's internal offsets; the port needs no equivalent because
    // command_string_at reads the blob's word-offset table directly each lookup.
    //
    // TODO: the PHRASE.BIN reload (= seg000:d01a..d03b, index 0x9a + language) is
    // not ported — the dialogue-text system that consumes PHRASE.BIN (the
    // get_phrase_or_command_string PHRASE branch) is itself stubbed, so there is
    // nothing to repoint yet.
    pub(crate) fn settings_ui_reload_language(&mut self) {
        // = seg000:cfe7..cff6 reload the glyph font (open_spritesheet_si_into_
        //   esdi into the 0ceec font buffer). si = 0xbb normally, 0xc7 for Fremen.
        let font_name = if self.language_setting == 6 {
            // = seg000:cfee si = 0xc7 — DNCHAR2.BIN (Fremen / DUT).
            "DNCHAR2.BIN"
        } else {
            "DNCHAR.BIN"
        };
        let font_data = self
            .dat_file
            .read(font_name)
            .unwrap_or_else(|e| panic!("settings_ui_reload_language: read {font_name}: {e}"));
        self.font = Font::new(&font_data);

        // = seg000:cffb..d00a si = 0xc0 + language; open_spritesheet_si_into_
        //   esdi into COMMANDx_BIN. COMMAND1.HSQ is language 0 (American).
        let command_name = format!("COMMAND{}.HSQ", self.language_setting + 1);
        self.command_bin = self
            .dat_file
            .read(&command_name)
            .unwrap_or_else(|e| panic!("settings_ui_reload_language: read {command_name}: {e}"));
    }
}
