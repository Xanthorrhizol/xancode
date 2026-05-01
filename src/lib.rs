pub use bytes::Bytes;
pub use xancode_macros::Codec;

pub trait Codec {
    type Error;

    fn encode(&self) -> Bytes;

    fn decode(data: &Bytes) -> Result<Self, Self::Error>
    where
        Self: Sized;
}
