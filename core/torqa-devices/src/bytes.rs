//! Little-endian reading of Bluetooth characteristic values.

/// A characteristic value could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The value ended before all fields announced by its flags were read.
    #[error("value too short: needed {needed} bytes, got {actual}")]
    TooShort {
        /// Bytes required by the announced fields.
        needed: usize,
        /// Bytes actually received.
        actual: usize,
    },
}

pub(crate) struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let end = self.pos + N;
        let bytes = self.data.get(self.pos..end).ok_or(ParseError::TooShort {
            needed: end,
            actual: self.data.len(),
        })?;
        self.pos = end;
        Ok(bytes.try_into().expect("slice has length N"))
    }

    pub(crate) fn skip(&mut self, n: usize) -> Result<(), ParseError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(ParseError::TooShort {
                needed: end,
                actual: self.data.len(),
            });
        }
        self.pos = end;
        Ok(())
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ParseError> {
        Ok(self.take::<1>()?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, ParseError> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    pub(crate) fn i16(&mut self) -> Result<i16, ParseError> {
        Ok(i16::from_le_bytes(self.take()?))
    }
}
