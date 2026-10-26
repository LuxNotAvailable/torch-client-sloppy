use azalea_buf::AzBuf;
use azalea_protocol_macros::ServerboundConfigPacket;
use uuid::Uuid;

#[derive(AzBuf, Clone, Debug, PartialEq, ServerboundConfigPacket)]
pub struct ServerboundResourcePack {
    pub id: Uuid,
    pub action: Action,
}

#[derive(AzBuf, Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    SuccessfullyLoaded = 0,
    Declined = 1,
    FailedDownload = 2,
    Accepted = 3,
    Downloaded = 4,
    InvalidUrl = 5,
    FailedReload = 6,
    Discarded = 7,
}
