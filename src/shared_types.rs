use bitcode::{Encode, Decode};
use std::io::Read;
use tokio::io::AsyncReadExt;

#[derive(Clone, Debug, Encode, Decode, Eq, PartialEq, Copy)]
pub enum RequestType {
    READ,
    WRITE,
}

#[derive(Encode, Decode, Clone, Debug)]
pub struct ClientRequest {
    pub request_id: u32,
    pub request_type: RequestType,
    pub key: u32,
    pub value: Vec<u8>,
}

#[derive(Encode, Decode, Debug)]
pub struct ClientResponse {
    pub request_id: u32,
    pub request_type: RequestType,
    pub key: u32,
    pub value: Vec<u8>,
}

pub trait Shared{
    fn to_bytes(&self) -> Vec<u8>;
    fn read_one<T: Read>(reader: &mut T) -> Result<Self, std::io::Error> where Self: Sized;
    #[allow(async_fn_in_trait)]
    async fn read_one_async<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Self, std::io::Error> where Self: Sized;
}

impl<T> Shared for T where T: Encode + for<'de> Decode<'de> {
    fn to_bytes(&self) -> Vec<u8> {
        // result structure: [4 bytes length, serialized bytes]
        let mut bytes = bitcode::encode(self);
        let mut bytes_w_len = Vec::with_capacity(bytes.len() + 4);
        bytes_w_len.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        bytes_w_len.append(&mut bytes);
        bytes_w_len
    }

    fn read_one<R: Read>(reader: &mut R) -> Result<Self, std::io::Error> {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        let length = u32::from_be_bytes(buf);
        let mut buf = vec![0u8; length as usize];
        reader.read_exact(&mut buf)?;
        bitcode::decode(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    async fn read_one_async<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Self, std::io::Error> {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).await?;
        let length = u32::from_be_bytes(buf);
        let mut buf = vec![0u8; length as usize];
        reader.read_exact(&mut buf).await?;
        bitcode::decode(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
