use std::io::{self, Cursor, Write};

use azalea_buf::{AzBuf, AzBufVar, BufReadError};
use azalea_chat::FormattedText;
use azalea_protocol_macros::ClientboundGamePacket;
use azalea_registry::builtin::MenuKind;

#[derive(ClientboundGamePacket, Clone, Debug, PartialEq)]
pub struct ClientboundOpenScreen {
    pub container_id: i32,
    /// The menu's type; [`MenuKind::Generic9x1`] for a mod's menu, whose id
    /// is then `modded_type`.
    pub menu_type: MenuKind,
    pub modded_type: Option<u32>,
    pub title: FormattedText,
}

impl AzBuf for ClientboundOpenScreen {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, BufReadError> {
        use azalea_registry::Registry;
        let container_id = i32::azalea_read_var(buf)?;
        let raw = u32::azalea_read_var(buf)?;
        let (menu_type, modded_type) = match MenuKind::from_u32(raw) {
            Some(kind) => (kind, None),
            None => (MenuKind::Generic9x1, Some(raw)),
        };
        Ok(ClientboundOpenScreen {
            container_id,
            menu_type,
            modded_type,
            title: FormattedText::azalea_read(buf)?,
        })
    }

    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        use azalea_registry::Registry;
        self.container_id.azalea_write_var(buf)?;
        self.modded_type
            .unwrap_or_else(|| self.menu_type.to_u32())
            .azalea_write_var(buf)?;
        self.title.azalea_write(buf)
    }
}
