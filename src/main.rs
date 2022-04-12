extern crate tokio;
extern crate anyhow;
extern crate bollard;
extern crate tar;
extern crate flate2;
extern crate async_channel;

use anyhow::Result;
use async_channel::RecvError;
use futures::stream::FusedStream;
use crate::docker::Docker;

mod docker;
mod maven;

#[tokio::main]
async fn main() -> Result<()> {
    let docker = Docker::new();
    let (s, r) = async_channel::bounded(32);

    tokio::task::spawn(async move {
        loop {
            let msg = r.recv().await;
            match msg {
                Ok(str) => {
                    println!("{}", str);
                }
                Err(e) => {
                    dbg!("receive error: {:?}", e);
                    if r.is_terminated() {
                        dbg!("channel is terminated");
                        break;
                    }
                }
            }
        }
    });

    docker.build("demo".to_string(), Some(s.clone())).await;
    docker.run("alpine:3.15".to_string(), vec!["echo".to_string(), "in container".to_string()], Some(s.clone())).await;
    drop(s);
    Ok(())
}

