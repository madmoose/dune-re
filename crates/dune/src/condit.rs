use std::io::Cursor;

use bytes_ext::ReadBytesExt;

use crate::{GameState, container};

fn condit_var_name(addr: u16) -> Option<(&'static str, bool)> {
    Some(match addr {
        0x00 => ("rand_bits", true),
        0x0b => ("current_room", false),
        0x0c => ("pending_destination_room", false),
        0x0d => ("previous_room", false),
        0x0e => ("persons_met", true),
        0x10 => ("persons_travelling_with", true),
        0x12 => ("persons_in_room", true),
        // 0x23 => ("data_00023", false),
        0x25 => ("number_of_sietches_visited", false),
        0x26 => ("entering_new_sietch", false),
        0x28 => ("number_of_rallied_troops", false),
        0x2a => ("game_phase", false),
        0x2b => ("night_attack_stage", false),
        0xf4 => ("desert_walk_counter", false),
        // 0xfc => ("data_000fc", true),
        _ => return None,
    })
}

impl GameState {
    fn condit_ds_byte(&self, addr: u16) -> Option<u8> {
        Some(match addr {
            // = seg001:000b current_room.
            0x0b => self.current_room,
            // = seg001:000c pending_destination_room — condition 0x1c (Leto's
            // room-leave line) tests it == 4.
            0x0c => self.pending_destination_room,
            // = seg001:000d previous_room.
            0x0d => self.previous_room,
            // = seg001:0023 data_00023 — the room-leave / dialogue-scan state;
            // condition 0x1c tests it == 1.
            0x23 => self.data_00023,
            // = seg001:0025 number_of_sietches_visited / 0026
            // entering_new_sietch — the first-visit state ui_click_move_room
            // maintains.
            0x25 => self.number_of_sietches_visited,
            0x26 => self.entering_new_sietch,
            // = seg001:0028 number_of_rallied_troops — the troop-rally system
            // is not ported; 0 in a new game. TODO: a real field when it lands
            // (conditions 4/5/7 gate early-game Leto lines on it).
            0x28 => 0,
            // = seg001:002a game_phase.
            0x2a => self.game_phase,
            // = seg001:002b night_attack_stage.
            0x2b => self.night_attack_stage,
            // = seg001:00f4 desert_walk_counter.
            0xf4 => self.desert_walk_counter,
            // = seg001:00fc data_000fc.
            0xfc => self.data_000fc,
            _ => return None,
        })
    }

    fn condit_ds_word(&self, addr: u16) -> Option<u16> {
        Some(match addr {
            // = seg001:0000 rand_bits — the rolling random-bit word (conditions
            // 0x25..0x28 pick a branch off its low bits).
            0x00 => self.rand_bits,
            // = seg001:000e persons_met / 0010 persons_travelling_with / 0012
            // persons_in_room — the person bitmasks several conditions test.
            0x0e => self.persons_met,
            0x10 => self.persons_travelling_with,
            0x12 => self.persons_in_room,
            _ => return None,
        })
    }

    fn condit_ds_read(&self, addr: u16, word: bool) -> u16 {
        let value = if word {
            self.condit_ds_word(addr)
        } else {
            self.condit_ds_byte(addr).map(u16::from)
        };
        value.unwrap_or_else(|| {
            let width = if word { "word" } else { "byte" };
            eprintln!("CONDIT: read of unmodelled {width} ds:[{addr:#04x}]");
            0
        })
    }

    // = seg000:a30b read_condit_operand.
    fn read_condit_operand(&self, c: &mut Cursor<&[u8]>) -> u16 {
        let b = c.read_u8().unwrap();

        if b < 0x80 {
            // = seg000:a311 — second byte is the ds offset of the variable.
            let addr = c.read_u8().unwrap() as u16;
            // = seg000:a31c mov ax,[bx] (16-bit var, b != 1) / seg000:a322
            // mov al,[bx]; xor ah,ah (8-bit var, b == 1).
            self.condit_ds_read(addr, b != 1)
        } else if b == 0x80 {
            // = seg000:a32c es:lodsb; xor ah,ah — 8-bit immediate.
            c.read_u8().unwrap() as u16
        } else {
            // = seg000:a331 es:lodsw — 16-bit immediate.
            c.read_le_u16().unwrap()
        }
    }

    // = seg000:a396 evaluate_condition.
    fn evaluate_condition(&self, index: u16) -> u16 {
        if index == 0 {
            return 0;
        }

        // = seg000:a39d les si,[res_condit]; add si,index*2; mov si,es:[si-2] —
        let entry = container::entry(&self.condit, index - 1);
        let mut c = Cursor::new(entry);

        // The scratch stack of (value, operator) frames the loose (0x80)
        // operators push (seg000:a3c0).
        let mut stack: Vec<(u16, u16)> = Vec::new();

        // = seg000:a3a7 — read the left operand into dx.
        let mut value = self.read_condit_operand(&mut c);

        // = seg000:a3ac loop — consume operator/operand pairs until 0xff.
        loop {
            let opcode = c.read_u8().unwrap();

            // = seg000:a3ae cmp al,0ffh; jz — end of expression.
            if opcode == 0xff {
                break;
            }

            if opcode & 0x80 != 0 {
                // = seg000:a3c0 — loose operator: push the accumulated value and
                // the operator, then start a fresh tight chain.
                stack.push((value, opcode as u16));
                value = self.read_condit_operand(&mut c);
            } else {
                // = seg000:a3b6 — tight operator: apply it immediately.
                let ax = self.read_condit_operand(&mut c);
                value = apply_operator(opcode as u16, value, ax);
            }
        }

        // = seg000:a3cb.
        if let Some(&(first, _)) = stack.first() {
            let mut acc = first;
            for i in 0..stack.len() {
                let op = stack[i].1;
                let rhs = stack.get(i + 1).map(|f| f.0).unwrap_or(value);
                acc = apply_operator(op, acc, rhs);
            }
            value = acc;
        }

        value
    }

    /// True when condition `index` holds (DOS: the `or dx,dx` non-zero test).
    /// With CONDIT not loaded, conditions read as always-true, matching the
    /// prior always-first-entry dialogue stub.
    pub(crate) fn condition_holds(&self, index: u16) -> bool {
        let holds = self.evaluate_condition(index) != 0;
        println!(
            "CONDITION {:3} {}: {}",
            index,
            if holds { "HOLDS" } else { "FAILS" },
            self.format_condition(index)
        );
        holds
    }

    pub fn format_condition(&self, index: u16) -> String {
        if index == 0 {
            return "<condition 0: inert, always 0>".into();
        }

        fn operand(c: &mut Cursor<&[u8]>) -> String {
            let b = c.read_u8().unwrap();
            if b < 0x80 {
                let addr = c.read_u8().unwrap() as u16;

                // = seg000:a318 — type byte 1 reads a byte, every other b <
                // 0x80 a word.
                let is_word = b != 1;
                let width = if is_word { "word" } else { "byte" };
                match condit_var_name(addr) {
                    Some((name, _)) => format!("{width}[{addr:04x}:{name}]"),
                    None => format!("{width}[{addr:#04x}]"),
                }
            } else if b == 0x80 {
                let v = c.read_u8().unwrap();
                if v <= 9 {
                    format!("{v}")
                } else {
                    format!("{v:#x}")
                }
            } else {
                let v = c.read_le_u16().unwrap();
                if v <= 9 {
                    format!("{v}")
                } else {
                    format!("{v:#x}")
                }
            }
        }

        fn op_symbol(opcode: u8) -> String {
            let sym = match opcode & 0x1f {
                0x00 => "==",
                0x02 => "<",
                0x04 => ">",
                0x06 => "!=",
                0x08 => "<=",
                0x0a => ">=",
                0x0c => "+",
                0x0e => "-",
                0x10 => "&",
                0x12 => "|",
                // The 0x14..0x1e slots fall into condit_operator_return_0.
                _ => return format!("?{:#04x}", opcode),
            };
            if opcode & 0x80 != 0 {
                format!("{sym}.")
            } else {
                sym.into()
            }
        }

        let entry = container::entry(&self.condit, index - 1);
        let mut c = Cursor::new(entry);

        let mut chains: Vec<(String, usize)> = Vec::new(); // (text, term count)
        let mut loose_ops: Vec<String> = Vec::new();
        let mut chain = operand(&mut c);
        let mut terms = 1usize;

        loop {
            let opcode = c.read_u8().unwrap();

            if opcode == 0xff {
                break;
            }
            if opcode & 0x80 != 0 {
                chains.push((std::mem::take(&mut chain), terms));
                loose_ops.push(op_symbol(opcode));
                chain = operand(&mut c);
                terms = 1;
            } else {
                chain = format!("{chain} {} {}", op_symbol(opcode), operand(&mut c));
                terms += 1;
            }
        }
        chains.push((chain, terms));

        let parenthesize = chains.len() > 1;
        let mut out = String::new();
        for (i, (text, terms)) in chains.iter().enumerate() {
            if i > 0 {
                out.push_str(&format!(" {} ", loose_ops[i - 1]));
            }
            if parenthesize && *terms >= 2 {
                out.push_str(&format!("({text})"));
            } else {
                out.push_str(text);
            }
        }
        out
    }
}

// = seg000:a334 evaluate_operator_bx_on_dx_and_ax.
fn apply_operator(op: u16, a: u16, b: u16) -> u16 {
    const TRUE: u16 = 0xffff;
    const FALSE: u16 = 0;
    match op & 0x1f {
        // = seg000:a348 cmpeq (jz).
        0x00 => {
            if a == b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a34f cmple (jb — unsigned below).
        0x02 => {
            if a < b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a356 cmpge (ja — unsigned above).
        0x04 => {
            if a > b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a35d cmpne (jnz).
        0x06 => {
            if a != b {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a364 cmplt (jle — signed less-or-equal).
        0x08 => {
            if (a as i16) <= (b as i16) {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a36b cmpgt (jge — signed greater-or-equal).
        0x0a => {
            if (a as i16) >= (b as i16) {
                TRUE
            } else {
                FALSE
            }
        }
        // = seg000:a33c addition.
        0x0c => a.wrapping_add(b),
        // = seg000:a33f subtraction.
        0x0e => a.wrapping_sub(b),
        // = seg000:a342 and.
        0x10 => a & b,
        // = seg000:a345 or.
        0x12 => a | b,
        // = seg000:a36f condit_operator_return_0 (codes 0x14..0x1e).
        _ => 0,
    }
}
