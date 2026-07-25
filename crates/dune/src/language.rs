use crate::{Font, GameState, container};

impl GameState {
    // = seg000:cf70 get_phrase_or_command_string
    // entries with bit 0x800 set are PHRASE.BIN dialogue strings; the rest index
    // COMMAND.BIN.
    pub fn get_phrase_or_command_string(&self, index: u16) -> &[u8] {
        // = seg000:cf71 dec si; test si,800h.
        let index = index - 1;
        if index & 0x800 != 0 {
            // = seg000:cf78..cf85 the PHRASE bank path: load_PHRASExx_HSQ
            //   must have loaded the record's bank (the port hoists that call
            //   to show_voice_subtitle, since this getter is immutable);
            //   index & 0x7ff selects the entry.
            if self.phrase_bin.is_empty() {
                return &[];
            }
            let index = index & 0x7ff;
            if index >= container::entry_count(&self.phrase_bin) {
                return &[];
            }
            return container::entry(&self.phrase_bin, index);
        }

        container::entry(&self.command_bin, index)
    }

    // = seg000:d03c find_last_numeric_digit_in_str_at_es_si + seg000:e2e3
    // string_replace_number_ending_at_es_si as DOS chains them (seg000:32fb..
    // 3307, seg000:5bc6): patch `value` in decimal over the three characters
    // ending at the COMMAND string's first run of digits, in place in the
    // resource buffer, so every later expansion of that id reads the new
    // number.
    pub(crate) fn command_string_replace_number(&mut self, id: u16, value: u16) {
        let (ofs, end) = container::entry_byte_range(&self.command_bin, id - 1);
        let s = &mut self.command_bin[ofs as usize..end as usize];
        // = seg000:d03c..d04d si = one past the first digit run.
        let Some(first) = s.iter().position(u8::is_ascii_digit) else {
            return;
        };
        let si = s[first..]
            .iter()
            .position(|c| !c.is_ascii_digit())
            .map_or(s.len(), |n| first + n);
        if si < 3 {
            return;
        }
        // = seg000:e2ea..e315 the fixed 3-digit field at [si-3..si], clamped to
        //   999, with the leading zeros written as spaces.
        let value = value.min(999);
        let digits = [value / 100, value / 10 % 10, value % 10];
        let mut leading = true;
        for (i, d) in digits.iter().enumerate() {
            leading &= *d == 0;
            s[si - 3 + i] = if leading && i < 2 {
                b' '
            } else {
                b'0' + *d as u8
            };
        }
    }

    // = seg000:cf88..cf8f the [_word_23C64_phrases_bin_last_entry] load — the
    // PHRASE bank's LAST entry is the phrase-token dictionary
    // expand_phrase_tokens indexes.
    pub(crate) fn phrase_dictionary(&self) -> &[u8] {
        if self.phrase_bin.is_empty() {
            return &[];
        }
        let count = container::entry_count(&self.phrase_bin);
        if count == 0 {
            return &[];
        }
        container::entry(&self.phrase_bin, count - 1)
    }

    // = seg000:d00f load_PHRASExx_HSQ — pick and (re)load the PHRASE bank for
    // the current dialogue record: records before the offset stored at
    // DIALOGUE entry 0x30 (persons 0..5) read bank 1 (resource 0x93 +
    // language = PHRASE{lang+1}1.HSQ), later ones bank 2 (0x9a + language =
    // PHRASE{lang+1}2.HSQ). A bank already resident is kept.
    pub(crate) fn load_phrasexx_hsq(&mut self) {
        // = seg000:d012 cmp [dialogue_current_record_ptr],
        //   [dialogue_phrase12_first_record_ptr] (= the word at DIALOGUE+0x60).
        let threshold = container::entry_offset(&self.dialogue, 0x30);
        let bank = if self.dialogue_current_record_ptr < threshold {
            1
        } else {
            2
        };
        // = seg000:d01c add al,[language_setting]; d020 already loaded?
        let id = if bank == 1 { 0x93 } else { 0x9a } + self.language_setting;
        if id == self.current_phrase_bin_id {
            return;
        }
        self.current_phrase_bin_id = id;
        let name = format!("PHRASE{}{}.HSQ", self.language_setting + 1, bank);
        // = seg000:d032 open_resource_by_index_si_into_esdi + the
        //   adjust_sub_resource_pointers relocation (the port's container
        //   reads the offset table in place).
        match self.dat_file.read(&name) {
            Ok(data) => self.phrase_bin = data.into_vec(),
            Err(e) => {
                eprintln!("load_phrasexx_hsq: failed to read {name}: {e}");
                self.phrase_bin = Vec::new();
            }
        }
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
    // The PHRASE bank reloads lazily: dropping current_phrase_bin_id makes the
    // next load_phrasexx_hsq fetch the new language's bank.
    pub(crate) fn settings_ui_reload_language(&mut self) {
        self.current_phrase_bin_id = 0;
        self.phrase_bin = Vec::new();
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
