#[derive(Debug)]
pub enum Error {
    BufferOverflow,
    DataUnderflow,
    InvalidFormat,
}

pub struct BytesIn<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> BytesIn<'a> {
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.offset == self.data.len()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn byte(&mut self) -> Result<u8, Error> {
        self.arr::<1>().map(|arr| arr[0])
    }

    pub fn slice(&mut self, len: usize) -> Result<&'a [u8], Error> {
        if len > self.data.len() - self.offset {
            Err(Error::DataUnderflow)
        } else {
            let data = &self.data[self.offset..self.offset + len];
            self.offset += len;

            Ok(data)
        }
    }

    pub fn arr<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let slice = self.slice(N)?;

        let mut data = [0; N];
        data.copy_from_slice(slice);

        Ok(data)
    }

    pub fn remaining(&mut self) -> &'a [u8] {
        let data = self.slice(self.data.len() - self.offset).unwrap();

        self.offset = self.data.len();

        data
    }

    pub fn remaining_byte(&mut self) -> Result<u8, Error> {
        Ok(self.remaining_arr::<1>()?[0])
    }

    pub fn remaining_arr<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        if self.data.len() - self.offset > N {
            Err(Error::InvalidFormat)
        } else {
            self.arr::<N>()
        }
    }
}

pub struct BytesOut<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> BytesOut<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.offset
    }

    pub fn byte(&mut self, data: u8) -> Result<&mut Self, Error> {
        self.push(&[data])
    }

    pub fn push(&mut self, data: &[u8]) -> Result<&mut Self, Error> {
        if data.len() > self.buf.len() - self.offset {
            Err(Error::BufferOverflow)
        } else {
            self.buf[self.offset..self.offset + data.len()].copy_from_slice(data);
            self.offset += data.len();

            Ok(self)
        }
    }
}
