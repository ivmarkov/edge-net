pub use rumqttc::*;

#[cfg(feature = "embedded-svc")]
pub use embedded_svc_compat::*;

#[cfg(feature = "embedded-svc")]
mod embedded_svc_compat {
    use core::fmt::{Debug, Display};

    use embedded_svc::mqtt::client::asynch::{
        Client, Connection, Details, ErrorType, Event, EventPayload, MessageId, Publish, QoS,
    };

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

    pub struct MqttEvent(rumqttc::Event);

    impl MqttEvent {
        fn payload(&self) -> Option<EventPayload<'_>> {
            match &self.0 {
                rumqttc::Event::Incoming(incoming) => match incoming {
                    rumqttc::Packet::Connect(_) => Some(EventPayload::BeforeConnect),
                    rumqttc::Packet::ConnAck(_) => Some(EventPayload::Connected(true)),
                    rumqttc::Packet::Disconnect => Some(EventPayload::Disconnected),
                    rumqttc::Packet::PubAck(PubAck { pkid, .. }) => {
                        Some(EventPayload::Published(*pkid as _))
                    }
                    rumqttc::Packet::SubAck(SubAck { pkid, .. }) => {
                        Some(EventPayload::Subscribed(*pkid as _))
                    }
                    rumqttc::Packet::UnsubAck(UnsubAck { pkid, .. }) => {
                        Some(EventPayload::Unsubscribed(*pkid as _))
                    }
                    rumqttc::Packet::Publish(rumqttc::Publish {
                        pkid,
                        topic,
                        payload,
                        ..
                    }) => Some(EventPayload::Received {
                        id: *pkid as _,
                        topic: Some(topic.as_str()),
                        data: payload,
                        details: Details::Complete,
                    }),
                    _ => None,
                },
                rumqttc::Event::Outgoing(_) => None,
            }
        }
    }

    impl Event for MqttEvent {
        fn payload(&self) -> EventPayload<'_> {
            MqttEvent::payload(self).unwrap()
        }
    }

    pub struct MqttConnection(EventLoop);

    impl MqttConnection {
        pub const fn new(event_loop: EventLoop) -> Self {
            Self(event_loop)
        }
    }

    impl ErrorType for MqttConnection {
        type Error = MqttError;
    }

    impl Connection for MqttConnection {
        type Event<'a> = MqttEvent where Self: 'a;

        async fn next(&mut self) -> Result<Option<Self::Event<'_>>, Self::Error> {
            loop {
                let event = self.0.poll().await?;
                trace!("Got event: {:?}", event);

                let event = MqttEvent(event);

                if event.payload().is_some() {
                    break Ok(Some(event));
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
