pub use rumqttc::*;

#[cfg(all(feature = "nightly", feature = "embedded-svc"))]
pub use embedded_svc_compat::*;

#[cfg(all(feature = "nightly", feature = "embedded-svc"))]
mod embedded_svc_compat {
    use core::fmt::{Debug, Display};
    use core::marker::PhantomData;

    use embedded_svc::mqtt::client::asynch::{
        Client, Connection, Details, ErrorType, Event, Message, MessageId, Publish, QoS,
    };
    use embedded_svc::mqtt::client::MessageImpl;

    use log::trace;

    use rumqttc::{AsyncClient, ClientError, ConnectionError, EventLoop, PubAck, SubAck, UnsubAck};

    #[derive(Debug)]
    pub enum MqttError {
        ClientError(ClientError),
        ConnectionError(ConnectionError),
    }

    impl Display for MqttError {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> Result<(), core::fmt::Error> {
            match self {
                MqttError::ClientError(error) => write!(f, "ClientError: {error}"),
                MqttError::ConnectionError(error) => write!(f, "ConnectionError: {error}"),
            }
        }
    }

    #[cfg(feature = "std")]
    impl std::error::Error for MqttError {}

    impl From<ClientError> for MqttError {
        fn from(value: ClientError) -> Self {
            Self::ClientError(value)
        }
    }

    impl From<ConnectionError> for MqttError {
        fn from(value: ConnectionError) -> Self {
            Self::ConnectionError(value)
        }
    }

    pub struct MqttClient(AsyncClient);

    impl MqttClient {
        pub const fn new(client: AsyncClient) -> Self {
            Self(client)
        }
    }

    impl ErrorType for MqttClient {
        type Error = MqttError;
    }

    impl Client for MqttClient {
        async fn subscribe(&mut self, topic: &str, qos: QoS) -> Result<MessageId, Self::Error> {
            self.0.subscribe(topic, to_qos(qos)).await?;

            Ok(0)
        }

        async fn unsubscribe(&mut self, topic: &str) -> Result<MessageId, Self::Error> {
            self.0.unsubscribe(topic).await?;

            Ok(0)
        }
    }

    impl Publish for MqttClient {
        async fn publish(
            &mut self,
            topic: &str,
            qos: embedded_svc::mqtt::client::QoS,
            retain: bool,
            payload: &[u8],
        ) -> Result<MessageId, Self::Error> {
            self.0.publish(topic, to_qos(qos), retain, payload).await?;

            Ok(0)
        }
    }

    pub struct MessageRef<'a>(&'a rumqttc::Publish);

    impl<'a> MessageRef<'a> {
        pub fn into_message_impl(&self) -> Option<MessageImpl> {
            Some(MessageImpl::new(self))
        }
    }

    impl<'a> Message for MessageRef<'a> {
        fn id(&self) -> MessageId {
            self.0.pkid as _
        }

        fn topic(&self) -> Option<&'_ str> {
            Some(&self.0.topic)
        }

        fn data(&self) -> &'_ [u8] {
            &self.0.payload
        }

        fn details(&self) -> &Details {
            &Details::Complete
        }
    }

    pub struct MqttConnection<F, M>(EventLoop, F, PhantomData<fn() -> M>);

    impl<F, M> MqttConnection<F, M> {
        pub const fn new(event_loop: EventLoop, message_converter: F) -> Self {
            Self(event_loop, message_converter, PhantomData)
        }
    }

    impl<F, M> ErrorType for MqttConnection<F, M> {
        type Error = MqttError;
    }

    impl<F, M> Connection for MqttConnection<F, M>
    where
        F: FnMut(&MessageRef) -> Option<M> + Send,
        M: Send,
    {
        type Message<'a> = M where Self: 'a;

        async fn next(&mut self) -> Option<Result<Event<Self::Message<'_>>, Self::Error>> {
            loop {
                let event = self.0.poll().await;
                trace!("Got event: {:?}", event);

                match event {
                    Ok(event) => {
                        let event = match event {
                            rumqttc::Event::Incoming(incoming) => match incoming {
                                rumqttc::Packet::Connect(_) => Some(Event::BeforeConnect),
                                rumqttc::Packet::ConnAck(_) => Some(Event::Connected(true)),
                                rumqttc::Packet::Disconnect => Some(Event::Disconnected),
                                rumqttc::Packet::PubAck(PubAck { pkid, .. }) => {
                                    Some(Event::Published(pkid as _))
                                }
                                rumqttc::Packet::SubAck(SubAck { pkid, .. }) => {
                                    Some(Event::Subscribed(pkid as _))
                                }
                                rumqttc::Packet::UnsubAck(UnsubAck { pkid, .. }) => {
                                    Some(Event::Unsubscribed(pkid as _))
                                }
                                rumqttc::Packet::Publish(publish) => {
                                    (self.1)(&MessageRef(&publish)).map(Event::Received)
                                }
                                _ => None,
                            },
                            rumqttc::Event::Outgoing(_) => None,
                        };

                        if let Some(event) = event {
                            return Some(Ok(event));
                        }
                    }
                    Err(err) => return Some(Err(MqttError::ConnectionError(err))),
                }
            }
        }
    }

    fn to_qos(qos: QoS) -> rumqttc::QoS {
        match qos {
            QoS::AtMostOnce => rumqttc::QoS::AtMostOnce,
            QoS::AtLeastOnce => rumqttc::QoS::AtLeastOnce,
            QoS::ExactlyOnce => rumqttc::QoS::ExactlyOnce,
        }
    }
}
