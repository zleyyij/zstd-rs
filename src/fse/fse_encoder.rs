use crate::encoding::bit_writer::BitWriter;
use alloc::vec::Vec;

pub struct FSEEncoder {
    pub(super) table: FSETable,
    writer: BitWriter,
}

impl FSEEncoder {
    pub fn new(table: FSETable) -> Self {
        FSEEncoder {
            table,
            writer: BitWriter::new(),
        }
    }

    pub fn encode(&mut self, data: &[u8]) -> Vec<u8> {
        self.write_table();

        let mut state = &self.table.states[data[data.len() - 1] as usize].states[0];
        for x in data[0..data.len() - 1].iter().rev().copied() {
            let next = self.table.next_state(x, state.index);
            let diff = state.index - next.baseline;
            self.writer.write_bits(diff as u64, next.num_bits as usize);
            state = next;
        }
        self.writer
            .write_bits(state.index as u64, self.acc_log() as usize);

        let mut writer = BitWriter::new();
        core::mem::swap(&mut self.writer, &mut writer);
        let bits_to_fill = writer.misaligned();
        if bits_to_fill == 0 {
            writer.write_bits(1u32, 8);
        } else {
            writer.write_bits(1u32, bits_to_fill);
        }
        writer.dump()
    }

    pub fn encode_interleaved(&mut self, data: &[u8]) -> Vec<u8> {
        self.write_table();

        let mut state_1 = &self.table.states[data[data.len() - 1] as usize].states[0];
        let mut state_2 = &self.table.states[data[data.len() - 2] as usize].states[0];

        let mut idx = data.len() - 4;
        loop {
            {
                let state = state_1;
                let x = data[idx + 1];
                let next = self.table.next_state(x, state.index);
                let diff = state.index - next.baseline;
                self.writer.write_bits(diff as u64, next.num_bits as usize);
                state_1 = next;
            }
            {
                let state = state_2;
                let x = data[idx];
                let next = self.table.next_state(x, state.index);
                let diff = state.index - next.baseline;
                self.writer.write_bits(diff as u64, next.num_bits as usize);
                state_2 = next;
            }

            if idx < 2 {
                break;
            }
            idx -= 2;
        }
        if idx == 1 {
            let state = state_1;
            let x = data[0];
            let next = self.table.next_state(x, state.index);
            let diff = state.index - next.baseline;
            self.writer.write_bits(diff as u64, next.num_bits as usize);
            state_1 = next;

            self.writer
                .write_bits(state_2.index as u64, self.acc_log() as usize);
            self.writer
                .write_bits(state_1.index as u64, self.acc_log() as usize);
        } else {
            self.writer
                .write_bits(state_1.index as u64, self.acc_log() as usize);
            self.writer
                .write_bits(state_2.index as u64, self.acc_log() as usize);
        }

        let mut writer = BitWriter::new();
        core::mem::swap(&mut self.writer, &mut writer);
        let bits_to_fill = writer.misaligned();
        if bits_to_fill == 0 {
            writer.write_bits(1u32, 8);
        } else {
            writer.write_bits(1u32, bits_to_fill);
        }
        writer.dump()
    }

    fn write_table(&mut self) {
        self.writer.write_bits(self.acc_log() - 5, 4);
        let mut probability_counter = 0usize;
        let probability_sum = 1 << self.acc_log();

        let mut prob_idx = 0;
        while probability_counter < probability_sum {
            let max_remaining_value = probability_sum - probability_counter + 1;
            let bits_to_write = max_remaining_value.ilog2() + 1;
            let low_threshold = ((1 << bits_to_write) - 1) - (max_remaining_value);
            let mask = (1 << (bits_to_write - 1)) - 1;

            let prob = self.table.states[prob_idx].probability;
            prob_idx += 1;
            let value = (prob + 1) as u32;
            if value < low_threshold as u32 {
                self.writer.write_bits(value, bits_to_write as usize - 1);
            } else if value > mask {
                self.writer
                    .write_bits(value + low_threshold as u32, bits_to_write as usize);
            } else {
                self.writer.write_bits(value, bits_to_write as usize);
            }

            if prob == -1 {
                probability_counter += 1;
            } else if prob > 0 {
                probability_counter += prob as usize;
            } else {
                let mut zeros = 0u8;
                while self.table.states[prob_idx].probability == 0 {
                    zeros += 1;
                    prob_idx += 1;
                    if zeros == 3 {
                        self.writer.write_bits(3u8, 2);
                        zeros = 0;
                    }
                }
                self.writer.write_bits(zeros, 2);
            }
        }
        self.writer.write_bits(0u8, self.writer.misaligned());
    }

    pub(super) fn acc_log(&self) -> u8 {
        self.table.table_size.ilog2() as u8
    }
}

#[derive(Debug)]
pub struct FSETable {
    /// Indexed by symbol
    pub(super) states: [SymbolStates; 256],
    table_size: usize,
}

impl FSETable {
    fn next_state(&self, symbol: u8, idx: usize) -> &State {
        let states = &self.states[symbol as usize];
        states.get(idx)
    }
}

#[derive(Debug)]
pub(super) struct SymbolStates {
    /// Sorted by baseline
    pub(super) states: Vec<State>,
    pub(super) probability: i32,
}

impl SymbolStates {
    fn get(&self, idx: usize) -> &State {
        // TODO we can do better, we can determin the correct state from the index with a bit of math
        self.states
            .iter()
            .find(|state| state.contains(idx))
            .unwrap()
    }
}

#[derive(Debug)]
pub(super) struct State {
    pub(super) num_bits: u8,
    pub(super) baseline: usize,
    pub(super) last_index: usize,
    /// Index of this state in the decoding table
    pub(super) index: usize,
}

impl State {
    fn contains(&self, idx: usize) -> bool {
        self.baseline <= idx && self.last_index >= idx
    }
}

pub fn build_table_from_data(data: &[u8], max_log: usize, avoid_0_numbit: bool) -> FSETable {
    let mut counts = [0; 256];
    for x in data {
        counts[*x as usize] += 1;
    }
    build_table_from_counts(&counts, max_log, avoid_0_numbit)
}

fn build_table_from_counts(counts: &[usize], max_log: usize, avoid_0_numbit: bool) -> FSETable {
    let mut probs = [0; 256];
    let mut min_count = 0;
    for (idx, count) in counts.iter().copied().enumerate() {
        probs[idx] = count as i32;
        if count > 0 && (count < min_count || min_count == 0) {
            min_count = count;
        }
    }

    // shift all probabilities down so that the lowest are 1
    min_count -= 1;
    for prob in probs.iter_mut() {
        if *prob > 0 {
            *prob -= min_count as i32;
        }
    }

    // normalize probabilities to a 2^x
    let sum = probs.iter().sum::<i32>();
    assert!(sum > 0);
    let sum = sum as usize;
    let acc_log = (sum.ilog2() as u8 + 1).max(5);
    assert!(acc_log < max_log as u8); // TODO implement logic to decrease some counts until this fits

    // just raise the maximum probability as much as possible
    // TODO is this optimal?
    let diff = (1 << acc_log) - sum;
    let max = probs.iter_mut().max().unwrap();
    *max += diff as i32;

    if avoid_0_numbit && *max > 1 << (acc_log - 1) {
        let redistribute = *max - (1 << (acc_log - 1));
        *max -= redistribute;
        let max = *max;
        let second_max = probs.iter_mut().filter(|x| **x != max).max().unwrap();
        *second_max += redistribute;
        assert!(*second_max <= max);
    }

    build_table_from_probabilities(&probs, acc_log)
}

pub(super) fn build_table_from_probabilities(probs: &[i32], acc_log: u8) -> FSETable {
    let mut states = core::array::from_fn::<SymbolStates, 256, _>(|_| SymbolStates {
        states: Vec::new(),
        probability: 0,
    });

    // distribute -1 symbols
    let mut negative_idx = (1 << acc_log) - 1;
    for (symbol, _prob) in probs
        .iter()
        .copied()
        .enumerate()
        .filter(|prob| prob.1 == -1)
    {
        states[symbol].states.push(State {
            num_bits: acc_log,
            baseline: 0,
            last_index: (1 << acc_log) - 1,
            index: negative_idx,
        });
        states[symbol].probability = -1;
        negative_idx -= 1;
    }

    // distribute other symbols
    let mut idx = 0;
    for (symbol, prob) in probs.iter().copied().enumerate() {
        if prob <= 0 {
            continue;
        }
        states[symbol].probability = prob;
        let states = &mut states[symbol].states;
        for _ in 0..prob {
            states.push(State {
                num_bits: 0,
                baseline: 0,
                last_index: 0,
                index: idx,
            });

            idx = next_position(idx, 1 << acc_log);
            while idx > negative_idx {
                idx = next_position(idx, 1 << acc_log);
            }
        }
        assert_eq!(states.len(), prob as usize);
    }

    for (symbol, prob) in probs.iter().copied().enumerate() {
        if prob <= 0 {
            continue;
        }
        let prob = prob as u32;
        let state = &mut states[symbol];
        state.states.sort_by(|l, r| l.index.cmp(&r.index));

        let prob_log = if prob.is_power_of_two() {
            prob.ilog2()
        } else {
            prob.ilog2() + 1
        };
        let rounded_up = 1u32 << prob_log;
        let double_states = rounded_up - prob;
        let single_states = prob - double_states;
        let num_bits = acc_log - prob_log as u8;
        let mut baseline = (single_states as usize * (1 << (num_bits))) % (1 << acc_log);
        for (idx, state) in state.states.iter_mut().enumerate() {
            if (idx as u32) < double_states {
                let num_bits = num_bits + 1;
                state.baseline = baseline;
                state.num_bits = num_bits;
                state.last_index = baseline + ((1 << num_bits) - 1);

                baseline += 1 << num_bits;
                baseline %= 1 << acc_log;
            } else {
                state.baseline = baseline;
                state.num_bits = num_bits;
                state.last_index = baseline + ((1 << num_bits) - 1);
                baseline += 1 << num_bits;
            }
        }
        state.states.sort_by(|l, r| l.baseline.cmp(&r.baseline));
    }

    FSETable {
        table_size: 1 << acc_log,
        states,
    }
}

/// Calculate the position of the next entry of the table given the current
/// position and size of the table.
fn next_position(mut p: usize, table_size: usize) -> usize {
    p += (table_size >> 1) + (table_size >> 3) + 3;
    p &= table_size - 1;
    p
}
