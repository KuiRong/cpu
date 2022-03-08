extern crate env_logger;
extern crate futures;
extern crate thrussh;
extern crate thrussh_keys;
extern crate tokio;
use anyhow::Result;
use futures::Future;
use std::io::{Read, Write};
use std::sync::Arc;
use thrussh::*;
// use thrussh::server::{Auth, Session};
use thrussh_keys::*;

struct Client {}

impl client::Handler for Client {
    type Error = thrussh::Error;
    type FutureUnit = futures::future::Ready<Result<(Self, client::Session), Self::Error>>;
    type FutureBool = futures::future::Ready<Result<(Self, bool), Self::Error>>;

    fn finished_bool(self, b: bool) -> Self::FutureBool {
        futures::future::ready(Ok((self, b)))
    }
    fn finished(self, session: client::Session) -> Self::FutureUnit {
        println!("FINISHED");
        futures::future::ready(Ok((self, session)))
    }
    fn check_server_key(self, server_public_key: &key::PublicKey) -> Self::FutureBool {
        println!(
            "check_server_key: {:?}",
            server_public_key.public_key_base64()
        );
        // TODO: compare against preshared key?
        self.finished_bool(true)
    }
    /*
    // FIXME: this here makes the session hang
    fn channel_open_confirmation(
        self,
        channel: ChannelId,
        max_packet_size: u32,
        window_size: u32,
        session: client::Session,
    ) -> Self::FutureUnit {
        println!("channel_open_confirmation: {:?}", channel);
        self.finished(session)
    }
    fn data(self, channel: ChannelId, data: &[u8], session: client::Session) -> Self::FutureUnit {
        println!(
            "data on channel {:?}: {:?}",
            channel,
            std::str::from_utf8(data)
        );
        self.finished(session)
    }
    */
}

// from https://nest.pijul.com/pijul/thrussh/discussions/20
pub struct Session {
    session: client::Handle<Client>,
}

impl Session {
    async fn connect(
        key_file: &str,
        user: impl Into<String>,
        addr: impl std::net::ToSocketAddrs,
    ) -> Result<Self> {
        let key_pair = thrussh_keys::load_secret_key(key_file, None)?;
        // TODO: import openssl for RSA key support
        /*
        let key_pair = key::KeyPair::RSA {
            key: openssl::rsa::Rsa::private_key_from_pem(pem)?,
            hash: key::SignatureHash::SHA2_512,
        };
        */
        let config = client::Config::default();
        let config = Arc::new(config);
        let sh = Client {};
        let mut agent = agent::client::AgentClient::connect_env().await?;
        agent.add_identity(&key_pair, &[]).await?;
        let mut identities = agent.request_identities().await?;
        let mut session = client::connect(config, addr, sh).await?;
        let pubkey = identities.pop().unwrap();
        let (_, auth_res) = session.authenticate_future(user, pubkey, agent).await;
        let _auth_res = auth_res?;
        Ok(Self { session })
    }

    async fn call(&mut self, command: &str) -> Result<CommandResult> {
        let mut channel = self.session.channel_open_session().await?;
        channel.exec(true, command).await?;
        let mut output = Vec::new();
        let mut code = None;
        while let Some(msg) = channel.wait().await {
            match msg {
                thrussh::ChannelMsg::Data { ref data } => {
                    output.write_all(&data).unwrap();
                }
                thrussh::ChannelMsg::ExitStatus { exit_status } => {
                    code = Some(exit_status);
                }
                _ => {}
            }
        }
        Ok(CommandResult { output, code })
    }

    async fn close(&mut self) -> Result<()> {
        self.session
            .disconnect(Disconnect::ByApplication, "", "English")
            .await?;
        Ok(())
    }
}

struct CommandResult {
    output: Vec<u8>,
    code: Option<u32>,
}

impl CommandResult {
    fn output(&self) -> String {
        String::from_utf8_lossy(&self.output).into()
    }

    fn success(&self) -> bool {
        self.code == Some(0)
    }
}

#[tokio::main]
pub async fn ssh() {
    let host = "localhost:22";
    let key_file = "/home/dama/.ssh/id_ed25519";

    let user: String;
    match std::env::var("USER") {
        Ok(val) => user = val,
        Err(e) => {
            user = "root".to_string();
            println!("No USER set({}); going with {}", e, user);
        }
    }

    let mut ssh = Session::connect(key_file, user, host).await.unwrap();
    let r = ssh.call("whoami").await.unwrap();
    assert!(r.success());
    println!("Who am I, anyway? {:?}", r.output());
    ssh.close().await.unwrap();
}