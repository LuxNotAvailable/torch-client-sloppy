pub(super) enum Frame {
    Binary(Vec<u8>),
    Text(String),
    Close(Option<String>),
    Other,
}

pub(super) use native::{Rx, Socket, Tx};

mod native {
    use futures_util::{
        SinkExt, StreamExt,
        stream::{SplitSink, SplitStream},
    };
    use tokio_tungstenite::tungstenite::Message;

    use super::Frame;

    type Stream = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    pub(in crate::eagler) struct Socket(Stream);
    pub(in crate::eagler) struct Tx(SplitSink<Stream, Message>);
    pub(in crate::eagler) struct Rx(SplitStream<Stream>);

    impl Socket {
        pub(in crate::eagler) async fn connect(url: &str) -> eyre::Result<Self> {
            crate::install_crypto_provider();
            match tokio_tungstenite::connect_async(url).await {
                Ok((socket, _response)) => Ok(Self(socket)),
                Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                    eyre::bail!(
                        "{url} answered with HTTP {} instead of upgrading to a websocket, so it \
                         is not an Eaglercraft endpoint. The endpoint is often on a subdomain or \
                         a path rather than the site root.",
                        response.status()
                    )
                }
                Err(e) => Err(e.into()),
            }
        }

        pub(in crate::eagler) async fn send_binary(
            &mut self,
            payload: Vec<u8>,
        ) -> eyre::Result<()> {
            self.0.send(Message::Binary(payload.into())).await?;
            Ok(())
        }

        pub(in crate::eagler) async fn send_text(&mut self, text: String) -> eyre::Result<()> {
            self.0.send(Message::Text(text.into())).await?;
            Ok(())
        }

        pub(in crate::eagler) async fn recv(&mut self) -> eyre::Result<Option<Frame>> {
            match self.0.next().await {
                Some(message) => Ok(Some(convert(message?))),
                None => Ok(None),
            }
        }

        pub(in crate::eagler) fn split(self) -> (Tx, Rx) {
            let (sink, stream) = self.0.split();
            (Tx(sink), Rx(stream))
        }
    }

    impl Tx {
        pub(in crate::eagler) async fn send_binary(
            &mut self,
            payload: Vec<u8>,
        ) -> eyre::Result<()> {
            self.0.send(Message::Binary(payload.into())).await?;
            Ok(())
        }
    }

    impl Rx {
        pub(in crate::eagler) async fn recv(&mut self) -> eyre::Result<Option<Frame>> {
            match self.0.next().await {
                Some(message) => Ok(Some(convert(message?))),
                None => Ok(None),
            }
        }
    }

    fn convert(message: Message) -> Frame {
        match message {
            Message::Binary(payload) => Frame::Binary(payload.to_vec()),
            Message::Text(text) => Frame::Text(text.to_string()),
            Message::Close(reason) => {
                Frame::Close(reason.map(|reason| format!("{} ({})", reason.reason, reason.code)))
            }
            _ => Frame::Other,
        }
    }
}
