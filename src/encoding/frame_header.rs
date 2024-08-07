//! Utilities and representations for a frame header.
use crate::encoding::{
    bit_writer::BitWriter,
    util::{find_min_size, minify_val},
};
use crate::frame;
use std::vec::Vec;

/// A header for a single Zstandard frame.
///
/// https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#frame_header
pub struct FrameHeader {
    /// Optionally, the original (uncompressed) size of the data within the frame in bytes.
    /// If not present, `window_size` must be set.
    pub frame_content_size: Option<u64>,
    /// If set to true, data must be regenerated within a single
    /// continuous memory segment.
    pub single_segment: bool,
    /// If set to true, a 32 bit content checksum will be present
    /// at the end of the frame.
    pub content_checksum: bool,
    /// If a dictionary ID is provided, the ID of that dictionary.
    pub dictionary_id: Option<u64>,
    /// The minimum memory buffer required to compress a frame. If not present,
    /// `single_segment` will be set to true. If present, this value must be greater than 1KB
    /// and less than 3.75TB. Encoders should not generate a frame that requires a window size larger than
    /// 8mb.
    pub window_size: Option<u64>,
}

#[derive(Debug)]
pub enum FrameHeaderError {
    SingleSegmentMissingContentSize,
    NoSingleSegmentMissingWindowSize,
}

impl core::fmt::Display for FrameHeaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FrameHeaderError::SingleSegmentMissingContentSize => {
                write!(
                    f,
                    "If `single_segment` is true, the `frame_content_size` field must be set."
                )
            }
            FrameHeaderError::NoSingleSegmentMissingWindowSize => {
                write!(
                    f,
                    "If `single_segment` is false, the `window_size` field must be set."
                )
            }
        }
    }
}

impl FrameHeader {
    /// Returns the serialized frame header.
    ///
    /// The returned header *does include* a frame header descriptor.
    pub fn serialize(self) -> Result<Vec<u8>, FrameHeaderError> {
        // https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#frame_header
        let mut output: Vec<u8> = Vec::with_capacity(14);

        // Magic Number:
        output.extend_from_slice(&frame::MAGIC_NUM.to_le_bytes());

        // `Frame_Header_Descriptor`:
        output.push(self.descriptor()?);

        // `Window_Descriptor
        // TODO: https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#window_descriptor
        if !self.single_segment {
            unimplemented!(
                "Support for using window size over frame content size is not implemented"
            );
        }

        if let Some(id) = self.dictionary_id {
            output.extend(minify_val(id));
        }

        if let Some(frame_content_size) = self.frame_content_size {
            output.extend(minify_val(frame_content_size));
        }

        Ok(output)
    }

    /// Generate a serialized frame header descriptor for the frame header.
    ///
    /// https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#frame_header_descriptor
    fn descriptor(&self) -> Result<u8, FrameHeaderError> {
        let mut bw = BitWriter::new();
        // A frame header starts with a frame header descriptor.
        // It describes what other fields are present
        // https://github.com/facebook/zstd/blob/dev/doc/zstd_compression_format.md#frame_header_descriptor
        // Writing the frame header descriptor:
        // `Frame_Content_Size_flag`:
        // The Frame_Content_Size_flag specifies if
        // the Frame_Content_Size field is provided within the header.
        // TODO: The Frame_Content_Size field isn't set at all, we should prefer to include it always.
        // If the `Single_Segment_flag` is set and this value is zero,
        // the size of the FCS field is 1 byte.
        // Otherwise, the FCS field is omitted.
        // | Value | Size of field (Bytes)
        // | 0     | 0 or 1
        // | 1     | 2
        // | 2     | 4
        // | 3     | 8
        if let Some(frame_content_size) = self.frame_content_size {
            let field_size = find_min_size(frame_content_size);
            let flag_value: u8 = match field_size {
                1 => 0,
                2 => 1,
                4 => 2,
                3 => 8,
                _ => panic!(),
            };
            bw.write_bits(&[flag_value], 2);
        } else {
            // `Frame_Content_Size` was not provided
            bw.write_bits(&[0], 2);
        }

        // `Single_Segment_flag`:
        // If this flag is set, data must be regenerated within a single continuous memory segment,
        // and the `Frame_Content_Size` field must be present in the header.
        // If this flag is not set, the `Window_Descriptor` field must be present in the frame header.
        if self.single_segment {
            if self.frame_content_size.is_none() {
                return Err(FrameHeaderError::SingleSegmentMissingContentSize);
            }
            bw.write_bits(&[1], 1);
        } else {
            if self.window_size.is_none() {
                return Err(FrameHeaderError::NoSingleSegmentMissingWindowSize);
            }
            bw.write_bits(&[0], 1);
        }

        // `Unused_bit`:
        // An encoder compliant with this spec must set this bit to zero
        bw.write_bits(&[0], 1);

        // `Reserved_bit`:
        // This value must be zero
        bw.write_bits(&[0], 1);

        // `Content_Checksum_flag`:
        if self.content_checksum {
            bw.write_bits(&[1], 1);
        } else {
            bw.write_bits(&[0], 1);
        }

        // `Dictionary_ID_flag`:
        if let Some(id) = self.dictionary_id {
            let flag_value: u8 = match find_min_size(id) {
                0 => 0,
                1 => 1,
                2 => 2,
                4 => 3,
                _ => panic!(),
            };
            bw.write_bits(&[flag_value], 2);
        } else {
            // A `Dictionary_ID` was not provided
            bw.write_bits(&[0], 2);
        }

        Ok(bw
            .dump()
            .expect("The frame header descriptor should always be exactly one byte.")[0])
    }
}

#[cfg(test)]
mod tests {
    use super::FrameHeader;
    use crate::frame::{read_frame_header, FrameDescriptor};

    #[test]
    fn frame_header_descriptor_decode() {
        let header = FrameHeader {
            frame_content_size: Some(1),
            single_segment: true,
            content_checksum: false,
            dictionary_id: None,
            window_size: None,
        };
        let descriptor = header.descriptor().unwrap();
        let decoded_descriptor = FrameDescriptor(descriptor);
        assert_eq!(decoded_descriptor.frame_content_size_bytes().unwrap(), 1);
        assert!(!decoded_descriptor.content_checksum_flag());
        assert_eq!(decoded_descriptor.dictionary_id_bytes().unwrap(), 0);
    }

    #[test]
    fn frame_header_decode() {
        // TODO: more test headers, maybe fuzz this?
        let header = FrameHeader {
            frame_content_size: Some(1),
            single_segment: true,
            content_checksum: false,
            dictionary_id: None,
            window_size: None,
        };

        let serialized_header = header.serialize();
        let parsed_header = read_frame_header(serialized_header.unwrap().as_slice())
            .unwrap()
            .0
            .header;
        assert!(parsed_header.dictionary_id().is_none());
        assert_eq!(parsed_header.frame_content_size(), 1);
    }
}
