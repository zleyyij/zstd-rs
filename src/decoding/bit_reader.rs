pub struct BitReader<'s> {
    idx: usize, //index counts bits already read
    source: &'s [u8],
}

impl<'s> BitReader<'s> {
    pub fn new(source: &'s [u8]) -> BitReader {
        BitReader {
            idx: 0,
            source: source,
        }
    }

    pub fn bits_left(&self) -> usize {
        self.source.len()*8 - self.idx
    }

    pub fn return_bits(&mut self, n: usize) {
        if n > self.idx {
            panic!("Cant return this many bits");
        }
        self.idx -= n;
    }

    pub fn get_bits(&mut self, n: usize) -> Result<u64, String> {
        if (self.idx + n) / 8 >= self.source.len() {
            return Err(format!(
                "Cant read n: {} bits. Bits left: {}",
                n,
                self.source.len() * 8 - self.idx
            ));
        }

        let mut value: u64;

        let bits_left_in_current_byte = 8 - (self.idx % 8);
        let bits_not_needed_in_current_byte = 8 - bits_left_in_current_byte;

        //collect bits from the currently pointed to byte
        value = (self.source[self.idx / 8] >> bits_not_needed_in_current_byte) as u64;

        if bits_left_in_current_byte >= n {
            //no need for fancy stuff

            //just mask all but the needed n bit
            value &= (1 << n) - 1;
            self.idx += n;
        } else {
            self.idx += bits_left_in_current_byte;

            //n spans over multiple bytes
            let full_bytes_needed = (n - bits_left_in_current_byte) / 8;
            let bits_in_last_byte_needed = n - bits_left_in_current_byte - full_bytes_needed * 8;

            assert!(
                bits_left_in_current_byte + full_bytes_needed * 8 + bits_in_last_byte_needed == n
            );

            let mut bit_shift = bits_left_in_current_byte; //this many bits are already set in value

            assert!(self.idx % 8 == 0);

            //collect full bytes
            for _ in 0..full_bytes_needed {
                value |= (self.source[self.idx / 8] << bit_shift) as u64;
                self.idx += 8;
                bit_shift += 8;
            }

            let val_las_byte =
                (self.source[self.idx / 8] as u64) & (1 << bits_in_last_byte_needed) - 1;
            value |= val_las_byte << bit_shift;
            self.idx += bits_in_last_byte_needed;
        }

        Ok(value)
    }

    pub fn reset(&mut self, new_source: &'s [u8]) {
        self.idx = 0;
        self.source = new_source;
    }
}